//! Header-only indexing of DICOM files into patients' studies and series.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use log::{debug, warn};
use rayon::prelude::*;
use summrise_i18n::{tr, tr_n};
use summrise_imaging::{AnatomicalPlane, Calibration, ImagePlane};

use crate::acquisition::Acquisition;
use crate::attrs;
use crate::discovery::{DiscoverySource, discover};
use crate::documents::DocumentRef;
use crate::error::{DicomError, Result};
use crate::fidelity::Fidelity;
use crate::media::Media;
use crate::pet::PetHeader;
use crate::progress::Progress;
use crate::stacks::FrameValues;

/// Everything found on a disc or in a folder.
#[derive(Debug, Clone)]
pub struct StudyIndex {
    pub root: PathBuf,
    pub source: DiscoverySource,
    pub studies: Vec<Study>,
    pub stats: IndexStats,
    pub warnings: Vec<String>,
    /// Where the files were found and every later read gets them from: the
    /// folder, or the ZIP archive or disc image that `root` is. The paths of
    /// the instances are paths in this media.
    pub media: Media,
    /// Report files on the medium. They are not tied to a study and are
    /// listed with every study.
    pub loose_documents: Vec<DocumentRef>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexStats {
    /// Files that were examined.
    pub files: usize,
    /// Image instances indexed.
    pub images: usize,
    /// Reports and documents indexed.
    pub documents: usize,
    /// DICOM objects that are not shown (presentation states, key objects…).
    pub non_images: usize,
    /// Files that could not be parsed.
    pub unreadable: usize,
    /// Duplicate SOP instances (same image stored twice).
    pub duplicates: usize,
}

/// Raw DICOM values; use [`crate::format`] for display.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patient {
    pub name: Option<String>,
    pub id: Option<String>,
    pub birth_date: Option<String>,
    pub sex: Option<String>,
    pub age: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Study {
    pub uid: String,
    pub patient: Patient,
    pub date: Option<String>,
    pub time: Option<String>,
    pub description: Option<String>,
    pub accession_number: Option<String>,
    pub institution: Option<String>,
    pub referring_physician: Option<String>,
    pub series: Vec<Series>,
    /// Structured reports, dose reports and PDFs stored in DICOM.
    pub documents: Vec<DocumentRef>,
}

impl Study {
    /// Distinct modalities, in series order.
    pub fn modalities(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for s in &self.series {
            if !out.contains(&s.modality.as_str()) {
                out.push(&s.modality);
            }
        }
        out
    }

    pub fn image_count(&self) -> usize {
        self.series.iter().map(Series::frame_count).sum()
    }

    pub fn title(&self) -> String {
        if self.description.is_none() && self.series.is_empty() {
            // Translators: the name of a study of which the disc holds only reports.
            return tr!("Study without images").to_owned();
        }
        // Translators: the name of a study without a description, such as "CT/MR study".
        self.description.clone().unwrap_or_else(|| tr!("{modalities} study", modalities = self.modalities().join("/")))
    }
}

#[derive(Debug, Clone)]
pub struct Series {
    pub uid: String,
    pub number: Option<i32>,
    pub description: Option<String>,
    pub modality: String,
    pub body_part: Option<String>,
    /// How the images were made, as recorded in the first file indexed.
    pub acquisition: Acquisition,
    /// Frame of Reference UID (0020,0052): series that share it have their
    /// positions in one patient coordinate system. `None` when not every
    /// file of the series records the same one.
    pub frame_of_reference: Option<String>,
    /// For PET series (modality PT): what the SUV calculation needs.
    pub pet: Option<PetHeader>,
    pub instances: Vec<InstanceHeader>,
}

impl Series {
    pub fn frame_count(&self) -> usize {
        self.instances.iter().map(|i| i.frames as usize).sum()
    }

    pub fn title(&self) -> String {
        match (&self.description, self.number) {
            (Some(d), _) => d.clone(),
            (None, Some(n)) => tr!("Series {number}", number = n),
            (None, None) => tr!("Unnamed series").to_owned(),
        }
    }

    /// Anatomical plane shared by all frames, `None` when mixed or unknown.
    pub fn plane(&self) -> Option<AnatomicalPlane> {
        let mut planes = self
            .instances
            .iter()
            .flat_map(|i| i.frame_planes.iter())
            .map(|p| p.map(|p| AnatomicalPlane::from_normal(p.normal())));
        let first = planes.next()??;
        planes.all(|p| p == Some(first)).then_some(first)
    }

    /// Whether frames carry geometry but in different orientations
    /// (e.g. a three-plane localizer).
    pub fn has_mixed_planes(&self) -> bool {
        self.plane().is_none() && self.instances.iter().any(|i| i.frame_planes.iter().any(Option::is_some))
    }

    /// (columns, rows) of the first image.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.instances.first().map(|i| (i.cols, i.rows))
    }

    /// The instance used for thumbnails and default metadata.
    pub fn representative(&self) -> Option<&InstanceHeader> {
        self.instances.get(self.instances.len() / 2)
    }

    /// Adds what one more file of the series records: the Frame of Reference
    /// is kept only while every file records the same one, and a PET series
    /// keeps the earliest acquisition time of its files.
    fn absorb(&mut self, frame_of_reference: Option<String>, acquired_s: Option<f64>) {
        if self.frame_of_reference != frame_of_reference {
            self.frame_of_reference = None;
        }
        if let (Some(pet), Some(acquired)) = (self.pet.as_mut(), acquired_s) {
            pet.earliest_acquisition_s = Some(pet.earliest_acquisition_s.map_or(acquired, |t| t.min(acquired)));
        }
    }

    /// Transfer syntaxes this build cannot decode.
    pub fn unsupported_transfer_syntaxes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in &self.instances {
            if !crate::format::transfer_syntax_supported(&i.transfer_syntax_uid)
                && !out.contains(&i.transfer_syntax_uid)
            {
                out.push(i.transfer_syntax_uid.clone());
            }
        }
        out
    }
}

/// Header of one image instance (single- or multi-frame).
#[derive(Debug, Clone)]
pub struct InstanceHeader {
    pub path: PathBuf,
    pub sop_instance_uid: String,
    pub sop_class_uid: String,
    pub transfer_syntax_uid: String,
    pub instance_number: Option<i32>,
    pub frames: u32,
    pub rows: u32,
    pub cols: u32,
    pub samples_per_pixel: u16,
    pub photometric: String,
    pub pixel_spacing: Option<[f64; 2]>,
    /// Where `pixel_spacing` comes from, and so what a distance on the image means.
    pub calibration: Calibration,
    /// Geometry per frame (length == `frames`); entries are `None` for
    /// projection images (X-ray, secondary capture…).
    pub frame_planes: Vec<Option<ImagePlane>>,
    pub image_type: Vec<String>,
    /// Bits Stored (0028,0101).
    pub bits_stored: u16,
    /// Pixel Representation (0028,0103) is 1: samples are two's complement.
    pub signed: bool,
    /// Modality LUT (never empty): one entry for the whole file, or one per
    /// frame when the frames are rescaled individually.
    pub rescale: Vec<Rescale>,
    /// Rescale Type (0028,1054), e.g. "HU".
    pub rescale_type: Option<String>,
    /// What tells this file's frames apart from other images at the same
    /// slice positions (time point, echo, b-value …): one entry per frame for
    /// Enhanced objects with per-frame values, else one for all frames;
    /// empty when nothing was read.
    pub frame_values: Vec<FrameValues>,
    /// Lossy compression, burned-in text, inverted presentation and the
    /// output range of a Modality LUT (see [`crate::fidelity`]).
    pub fidelity: Fidelity,
}

/// Linear Modality LUT (PS3.3 C.11.1): value = stored value × slope + intercept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rescale {
    pub slope: f64,
    pub intercept: f64,
}

impl Rescale {
    pub const IDENTITY: Self = Self { slope: 1.0, intercept: 0.0 };

    pub fn apply(&self, stored: f64) -> f64 {
        stored * self.slope + self.intercept
    }
}

impl InstanceHeader {
    /// Whether the image is in colour: three samples per pixel, or palette colour.
    pub fn is_color(&self) -> bool {
        self.samples_per_pixel == 3 || self.photometric == "PALETTE COLOR"
    }

    /// Range of modality values this image can hold, from Bits Stored, Pixel
    /// Representation and the rescale values (or the output of its Modality
    /// LUT). Known before any pixel data is read; the values actually present
    /// usually span less. `None` for colour images.
    pub fn value_range_estimate(&self) -> Option<(f64, f64)> {
        if self.samples_per_pixel != 1 || self.is_color() {
            return None;
        }
        if let Some(range) = self.fidelity.modality_lut_range {
            return Some(range);
        }
        let bits = i32::from(self.bits_stored.clamp(1, 32));
        let (lo, hi) = if self.signed {
            (-(2f64.powi(bits - 1)), 2f64.powi(bits - 1) - 1.0)
        } else {
            (0.0, 2f64.powi(bits) - 1.0)
        };
        self.rescale
            .iter()
            .flat_map(|r| [r.apply(lo), r.apply(hi)])
            .filter(|v| v.is_finite())
            .fold(None, |range, v| Some(range.map_or((v, v), |(a, b): (f64, f64)| (a.min(v), b.max(v)))))
    }
}

/// Discovers and indexes all images below `root`.
pub fn scan(root: &Path, progress: &Progress) -> Result<StudyIndex> {
    let discovery = discover(root, progress)?;
    let mut index = index_files(&discovery.media, &discovery.files, progress)?;
    index.root = root.to_path_buf();
    index.source = discovery.source;
    index.warnings.splice(0..0, discovery.warnings);
    index.loose_documents = discovery.documents;
    if let Some(archive) = index.media.archive() {
        debug!("indexed from {} of the {} bytes of the archive", archive.bytes_read(), archive.len());
    }
    if index.studies.is_empty() {
        // An archive that holds images which cannot be read says so.
        return Err(match discovery.blocker {
            Some(problem) => DicomError::Archive { path: root.to_path_buf(), problem },
            None => DicomError::NothingFound(root.to_path_buf()),
        });
    }
    Ok(index)
}

struct HeaderRecord {
    study_uid: String,
    study: Study,
    series_uid: String,
    series: Series,
    instance: Option<InstanceHeader>,
    /// The document this object is, for an object without images.
    document: Option<DocumentRef>,
}

/// Reads the headers of `files` in `media` in parallel and groups them by
/// Study/Series Instance UID.
pub fn index_files(media: &Media, files: &[PathBuf], progress: &Progress) -> Result<StudyIndex> {
    progress.begin(tr!("Reading image headers"), files.len());
    let records: Vec<std::result::Result<HeaderRecord, String>> = files
        .par_iter()
        .map(|path| {
            if progress.is_cancelled() {
                return Err("cancelled".to_owned());
            }
            let record =
                crate::panics::catch(|| read_header(media, path)).unwrap_or_else(|_| Err("parser panicked".to_owned()));
            progress.advance();
            record.map_err(|e| {
                debug!("skipping {}: {e}", path.display());
                e
            })
        })
        .collect();
    if progress.is_cancelled() {
        return Err(DicomError::Cancelled);
    }

    let mut stats = IndexStats { files: files.len(), ..IndexStats::default() };
    let mut studies: Vec<Study> = Vec::new();
    let mut study_pos: HashMap<String, usize> = HashMap::new();
    let mut series_pos: HashMap<(usize, String), usize> = HashMap::new();
    let mut seen_sops: HashMap<String, ()> = HashMap::new();

    for record in records {
        let Ok(rec) = record else {
            stats.unreadable += 1;
            continue;
        };
        let Some(instance) = rec.instance else {
            let Some(document) = rec.document else {
                stats.non_images += 1;
                continue;
            };
            if document.sop_instance_uid.as_ref().is_some_and(|uid| seen_sops.insert(uid.clone(), ()).is_some()) {
                stats.duplicates += 1;
                continue;
            }
            stats.documents += 1;
            let si = *study_pos.entry(rec.study_uid.clone()).or_insert_with(|| {
                studies.push(rec.study);
                studies.len() - 1
            });
            studies[si].documents.push(document);
            continue;
        };
        if seen_sops.insert(instance.sop_instance_uid.clone(), ()).is_some() {
            stats.duplicates += 1;
            continue;
        }
        stats.images += 1;
        let si = *study_pos.entry(rec.study_uid.clone()).or_insert_with(|| {
            studies.push(rec.study);
            studies.len() - 1
        });
        let study = &mut studies[si];
        let file =
            (rec.series.frame_of_reference.clone(), rec.series.pet.as_ref().and_then(|p| p.earliest_acquisition_s));
        let ri = *series_pos.entry((si, rec.series_uid.clone())).or_insert_with(|| {
            study.series.push(rec.series);
            study.series.len() - 1
        });
        study.series[ri].absorb(file.0, file.1);
        study.series[ri].instances.push(instance);
    }

    for study in &mut studies {
        study.series.retain(|s| !s.instances.is_empty());
        crate::documents::sort(&mut study.documents);
        for series in &mut study.series {
            series
                .instances
                .sort_by(|a, b| a.instance_number.cmp(&b.instance_number).then_with(|| a.path.cmp(&b.path)));
        }
        study.series.sort_by(|a, b| {
            a.number
                .unwrap_or(i32::MAX)
                .cmp(&b.number.unwrap_or(i32::MAX))
                .then_with(|| a.description.cmp(&b.description))
                // Whatever order the file system lists the files in, the same order here.
                .then_with(|| a.uid.cmp(&b.uid))
        });
    }
    studies.sort_by(|a, b| (&a.date, &a.time, &a.uid).cmp(&(&b.date, &b.time, &b.uid)));

    let mut warnings = Vec::new();
    if stats.unreadable > 0 {
        warnings.push(tr_n!(
            "{n} file could not be read and was skipped",
            "{n} files could not be read and were skipped",
            stats.unreadable
        ));
    }
    if stats.non_images > 0 {
        warnings.push(tr_n!(
            "{n} object that is not an image (such as an annotation) is not shown",
            "{n} objects that are not images (such as annotations) are not shown",
            stats.non_images
        ));
    }
    if stats.unreadable > 0 {
        warn!("{} unreadable files during indexing", stats.unreadable);
    }
    let media = media.clone();
    Ok(StudyIndex {
        root: PathBuf::new(),
        source: DiscoverySource::FolderScan,
        studies,
        stats,
        warnings,
        media,
        loose_documents: Vec::new(),
    })
}

fn read_header(media: &Media, path: &Path) -> std::result::Result<HeaderRecord, String> {
    // Up to the pixel data and no further: from an archive, only that much of the entry is read.
    let file = media.open_object(path, Some(tags::PIXEL_DATA))?;
    let transfer_syntax_uid = file.meta().transfer_syntax().trim_end_matches('\0').to_owned();
    let media_sop_class = file.meta().media_storage_sop_class_uid().trim_end_matches('\0').to_owned();
    let obj: &InMemDicomObject = &file;

    if media_sop_class == dicom_dictionary_std::uids::MEDIA_STORAGE_DIRECTORY_STORAGE {
        return Err("DICOMDIR is not an image".to_owned());
    }

    let study_uid = attrs::text(obj, tags::STUDY_INSTANCE_UID)
        .unwrap_or_else(|| format!("unknown-study:{}", path.parent().unwrap_or(path).display()));
    let series_uid = attrs::text(obj, tags::SERIES_INSTANCE_UID)
        .unwrap_or_else(|| format!("unknown-series:{}", path.parent().unwrap_or(path).display()));

    let study = Study {
        uid: study_uid.clone(),
        patient: Patient {
            name: attrs::text(obj, tags::PATIENT_NAME),
            id: attrs::text(obj, tags::PATIENT_ID),
            birth_date: attrs::text(obj, tags::PATIENT_BIRTH_DATE),
            sex: attrs::text(obj, tags::PATIENT_SEX),
            age: attrs::text(obj, tags::PATIENT_AGE),
        },
        date: attrs::text(obj, tags::STUDY_DATE),
        time: attrs::text(obj, tags::STUDY_TIME),
        description: attrs::text(obj, tags::STUDY_DESCRIPTION),
        accession_number: attrs::text(obj, tags::ACCESSION_NUMBER),
        institution: attrs::text(obj, tags::INSTITUTION_NAME),
        referring_physician: attrs::text(obj, tags::REFERRING_PHYSICIAN_NAME),
        series: Vec::new(),
        documents: Vec::new(),
    };
    let series = Series {
        uid: series_uid.clone(),
        number: attrs::int(obj, tags::SERIES_NUMBER),
        description: attrs::text(obj, tags::SERIES_DESCRIPTION),
        modality: attrs::text(obj, tags::MODALITY).unwrap_or_else(|| "OT".to_owned()),
        body_part: attrs::text(obj, tags::BODY_PART_EXAMINED),
        acquisition: Acquisition::read(obj),
        frame_of_reference: attrs::text(obj, tags::FRAME_OF_REFERENCE_UID),
        pet: PetHeader::of_file(obj),
        instances: Vec::new(),
    };

    let rows = attrs::uint(obj, tags::ROWS).unwrap_or(0);
    let cols = attrs::uint(obj, tags::COLUMNS).unwrap_or(0);
    let document =
        if rows > 0 && cols > 0 { None } else { crate::documents::classify(obj, media, path, &media_sop_class) };
    let instance = (rows > 0 && cols > 0).then(|| {
        let frames = attrs::uint(obj, tags::NUMBER_OF_FRAMES).unwrap_or(1).max(1);
        let (pixel_spacing, calibration, frame_planes) = geometry(obj, rows, cols, frames);
        let signed = attrs::uint(obj, tags::PIXEL_REPRESENTATION) == Some(1);
        let fidelity = crate::fidelity::read(obj, &transfer_syntax_uid, signed);
        InstanceHeader {
            path: path.to_path_buf(),
            sop_instance_uid: attrs::text(obj, tags::SOP_INSTANCE_UID).unwrap_or_else(|| path.display().to_string()),
            sop_class_uid: attrs::text(obj, tags::SOP_CLASS_UID).unwrap_or(media_sop_class),
            transfer_syntax_uid,
            instance_number: attrs::int(obj, tags::INSTANCE_NUMBER),
            frames,
            rows,
            cols,
            samples_per_pixel: attrs::uint(obj, tags::SAMPLES_PER_PIXEL).unwrap_or(1) as u16,
            photometric: attrs::text(obj, tags::PHOTOMETRIC_INTERPRETATION).unwrap_or_default(),
            pixel_spacing,
            calibration,
            frame_planes,
            image_type: attrs::texts(obj, tags::IMAGE_TYPE),
            bits_stored: attrs::uint(obj, tags::BITS_STORED)
                .filter(|&bits| bits > 0)
                .or_else(|| attrs::uint(obj, tags::BITS_ALLOCATED))
                .map_or(16, |bits| bits.min(32) as u16),
            signed,
            rescale: rescale(obj),
            rescale_type: attrs::text(obj, tags::RESCALE_TYPE).or_else(|| crate::fidelity::modality_lut_type(obj)),
            frame_values: crate::stacks::read_frame_values(obj, frames),
            fidelity,
        }
    });
    Ok(HeaderRecord { study_uid, study, series_uid, series, instance, document })
}

/// Rescale Slope/Intercept from the classic attributes, else from the
/// per-frame or shared functional groups of Enhanced multi-frame objects
/// (the order the pixel decoder uses); identity when absent.
fn rescale(obj: &InMemDicomObject) -> Vec<Rescale> {
    let values = |tag: Tag| -> Vec<f64> {
        let in_group = |group: &InMemDicomObject| {
            attrs::functional(group, tags::PIXEL_VALUE_TRANSFORMATION_SEQUENCE).and_then(|m| attrs::float(m, tag))
        };
        if let Some(value) = attrs::float(obj, tag) {
            return vec![value];
        }
        let per_frame: Vec<f64> = attrs::per_frame_groups(obj).iter().filter_map(in_group).collect();
        if !per_frame.is_empty() {
            return per_frame;
        }
        // In the shared group the values are sometimes stored flat instead of in their own sequence.
        let shared = attrs::shared_group(obj);
        shared.and_then(in_group).or_else(|| shared.and_then(|group| attrs::float(group, tag))).into_iter().collect()
    };
    let slopes = values(tags::RESCALE_SLOPE);
    let intercepts = values(tags::RESCALE_INTERCEPT);
    let pick = |values: &[f64], i: usize, default: f64| values.get(i).or(values.first()).copied().unwrap_or(default);
    (0..slopes.len().max(intercepts.len()).max(1))
        .map(|i| Rescale { slope: pick(&slopes, i, 1.0), intercept: pick(&intercepts, i, 0.0) })
        .collect()
}

/// Pixel spacing, where it comes from and per-frame geometry, from classic
/// attributes or the Enhanced multi-frame functional groups (PS3.3 C.7.6.16).
fn geometry(
    obj: &InMemDicomObject,
    rows: u32,
    cols: u32,
    frames: u32,
) -> (Option<[f64; 2]>, Calibration, Vec<Option<ImagePlane>>) {
    let shared = attrs::shared_group(obj);
    let from_group = |group: Option<&InMemDicomObject>| {
        let ipp = group
            .and_then(|g| attrs::functional(g, tags::PLANE_POSITION_SEQUENCE))
            .and_then(|m| attrs::float_array::<3>(m, tags::IMAGE_POSITION_PATIENT));
        let iop = group
            .and_then(|g| attrs::functional(g, tags::PLANE_ORIENTATION_SEQUENCE))
            .and_then(|m| attrs::float_array::<6>(m, tags::IMAGE_ORIENTATION_PATIENT));
        let ps = group
            .and_then(|g| attrs::functional(g, tags::PIXEL_MEASURES_SEQUENCE))
            .and_then(|m| attrs::float_array::<2>(m, tags::PIXEL_SPACING));
        (ipp, iop, ps)
    };
    let (shared_ipp, shared_iop, shared_ps) = from_group(shared);
    let ipp = attrs::float_array::<3>(obj, tags::IMAGE_POSITION_PATIENT).or(shared_ipp);
    let iop = attrs::float_array::<6>(obj, tags::IMAGE_ORIENTATION_PATIENT).or(shared_iop);
    let ps = attrs::float_array::<2>(obj, tags::PIXEL_SPACING).or(shared_ps);

    let per_frame = attrs::per_frame_groups(obj);
    let planes: Vec<Option<ImagePlane>> = if per_frame.len() == frames as usize && frames > 0 {
        per_frame
            .iter()
            .map(|item| {
                let (f_ipp, f_iop, f_ps) = from_group(Some(item));
                ImagePlane::from_dicom(f_ipp.or(ipp)?, f_iop.or(iop)?, f_ps.or(ps)?, rows, cols)
            })
            .collect()
    } else if frames == 1 {
        vec![ipp.zip(iop).zip(ps).and_then(|((ipp, iop), ps)| ImagePlane::from_dicom(ipp, iop, ps, rows, cols))]
    } else {
        // Classic multi-frame (cine, NM…) without per-frame positions.
        vec![None; frames as usize]
    };
    let (pixel_spacing, calibration) = Calibration::ladder(
        ps.or_else(|| per_frame.first().and_then(|f| from_group(Some(f)).2)),
        attrs::float_array::<2>(obj, tags::IMAGER_PIXEL_SPACING),
    );
    (pixel_spacing, calibration, planes)
}

#[cfg(test)]
mod tests {
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, Length, PrimitiveValue, VR};

    use super::*;

    fn decimal(tag: Tag, value: &str) -> DataElement<InMemDicomObject> {
        DataElement::new(tag, VR::DS, PrimitiveValue::from(value))
    }

    fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
        DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED))
    }

    /// A functional group with a Pixel Value Transformation.
    fn transformation(slope: &str, intercept: &str) -> InMemDicomObject {
        let values = InMemDicomObject::from_element_iter([
            decimal(tags::RESCALE_SLOPE, slope),
            decimal(tags::RESCALE_INTERCEPT, intercept),
        ]);
        InMemDicomObject::from_element_iter([sequence(tags::PIXEL_VALUE_TRANSFORMATION_SEQUENCE, vec![values])])
    }

    /// The index records where the spacing comes from instead of merging the
    /// attributes into one value (measurement-tools D4).
    #[test]
    fn the_source_of_the_pixel_spacing_is_recorded() {
        let spacing = |tag: Tag, values: [&str; 2]| {
            let values = values.iter().map(|v| (*v).to_owned()).collect();
            DataElement::new(tag, VR::DS, PrimitiveValue::Strs(values))
        };
        let read = |elements: Vec<DataElement<InMemDicomObject>>| {
            let (pixel_spacing, calibration, _) = geometry(&InMemDicomObject::from_element_iter(elements), 4, 4, 1);
            (pixel_spacing, calibration)
        };
        let pixel = spacing(tags::PIXEL_SPACING, ["0.5", "0.25"]);
        let imager = spacing(tags::IMAGER_PIXEL_SPACING, ["0.2", "0.2"]);
        assert_eq!(read(vec![pixel.clone()]), (Some([0.5, 0.25]), Calibration::PixelSpacing));
        assert_eq!(read(vec![pixel, imager.clone()]), (Some([0.5, 0.25]), Calibration::PixelSpacing));
        assert_eq!(read(vec![imager.clone()]), (Some([0.2, 0.2]), Calibration::ImagerPixelSpacing));
        assert_eq!(read(Vec::new()), (None, Calibration::Uncalibrated));
        let unusable = spacing(tags::PIXEL_SPACING, ["0", "0"]);
        assert_eq!(read(vec![unusable, imager]), (None, Calibration::Uncalibrated));
        // In the functional groups of an Enhanced object it counts as Pixel Spacing too.
        let measures = InMemDicomObject::from_element_iter([spacing(tags::PIXEL_SPACING, ["0.7", "0.7"])]);
        let group = InMemDicomObject::from_element_iter([sequence(tags::PIXEL_MEASURES_SEQUENCE, vec![measures])]);
        let shared = sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![group]);
        assert_eq!(read(vec![shared]), (Some([0.7, 0.7]), Calibration::PixelSpacing));
    }

    #[test]
    fn rescale_values_are_found_where_the_pixel_decoder_looks() {
        // Absent: stored values are modality values.
        assert_eq!(rescale(&InMemDicomObject::new_empty()), vec![Rescale::IDENTITY]);
        // Classic attributes; a missing slope is 1.
        let classic = InMemDicomObject::from_element_iter([
            decimal(tags::RESCALE_SLOPE, "2"),
            decimal(tags::RESCALE_INTERCEPT, "-1024"),
        ]);
        assert_eq!(rescale(&classic), vec![Rescale { slope: 2.0, intercept: -1024.0 }]);
        let intercept_only = InMemDicomObject::from_element_iter([decimal(tags::RESCALE_INTERCEPT, "-1024")]);
        assert_eq!(rescale(&intercept_only), vec![Rescale { slope: 1.0, intercept: -1024.0 }]);

        // Enhanced multi-frame: one transformation per frame …
        let per_frame = InMemDicomObject::from_element_iter([sequence(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![transformation("1", "0"), transformation("0.5", "-100")],
        )]);
        assert_eq!(
            rescale(&per_frame),
            vec![Rescale { slope: 1.0, intercept: 0.0 }, Rescale { slope: 0.5, intercept: -100.0 }]
        );
        // … or one shared by all frames …
        let shared = InMemDicomObject::from_element_iter([sequence(
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![transformation("1", "-1000")],
        )]);
        assert_eq!(rescale(&shared), vec![Rescale { slope: 1.0, intercept: -1000.0 }]);
        // … which some files store flat in the shared group.
        let flat = InMemDicomObject::from_element_iter([sequence(
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![InMemDicomObject::from_element_iter([
                decimal(tags::RESCALE_SLOPE, "3"),
                decimal(tags::RESCALE_INTERCEPT, "7"),
            ])],
        )]);
        assert_eq!(rescale(&flat), vec![Rescale { slope: 3.0, intercept: 7.0 }]);

        // The classic attributes win over the functional groups, as in the decoder.
        let both = InMemDicomObject::from_element_iter([
            decimal(tags::RESCALE_SLOPE, "2"),
            decimal(tags::RESCALE_INTERCEPT, "5"),
            sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![transformation("9", "9")]),
        ]);
        assert_eq!(rescale(&both), vec![Rescale { slope: 2.0, intercept: 5.0 }]);
    }
}
