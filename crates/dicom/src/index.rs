//! Header-only indexing of DICOM files into patients' studies and series.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::{InMemDicomObject, OpenFileOptions};
use log::{debug, warn};
use rayon::prelude::*;
use summrise_imaging::{AnatomicalPlane, ImagePlane};

use crate::attrs;
use crate::discovery::{DiscoverySource, discover};
use crate::error::{DicomError, Result};
use crate::progress::Progress;

/// Everything found on a disc or in a folder.
#[derive(Debug, Clone)]
pub struct StudyIndex {
    pub root: PathBuf,
    pub source: DiscoverySource,
    pub studies: Vec<Study>,
    pub stats: IndexStats,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexStats {
    /// Files that were examined.
    pub files: usize,
    /// Image instances indexed.
    pub images: usize,
    /// DICOM objects without pixel data (reports, presentation states…).
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
        self.description.clone().unwrap_or_else(|| format!("{} study", self.modalities().join("/")))
    }
}

#[derive(Debug, Clone)]
pub struct Series {
    pub uid: String,
    pub number: Option<i32>,
    pub description: Option<String>,
    pub modality: String,
    pub body_part: Option<String>,
    pub instances: Vec<InstanceHeader>,
}

impl Series {
    pub fn frame_count(&self) -> usize {
        self.instances.iter().map(|i| i.frames as usize).sum()
    }

    pub fn title(&self) -> String {
        match (&self.description, self.number) {
            (Some(d), _) => d.clone(),
            (None, Some(n)) => format!("Series {n}"),
            (None, None) => "Unnamed series".to_owned(),
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
    /// Geometry per frame (length == `frames`); entries are `None` for
    /// projection images (X-ray, secondary capture…).
    pub frame_planes: Vec<Option<ImagePlane>>,
    pub image_type: Vec<String>,
}

/// Discovers and indexes all images below `root`.
pub fn scan(root: &Path, progress: &Progress) -> Result<StudyIndex> {
    let discovery = discover(root, progress)?;
    let mut index = index_files(&discovery.files, progress)?;
    index.root = root.to_path_buf();
    index.source = discovery.source;
    index.warnings.splice(0..0, discovery.warnings);
    if index.studies.is_empty() {
        return Err(DicomError::NothingFound(root.to_path_buf()));
    }
    Ok(index)
}

struct HeaderRecord {
    study_uid: String,
    study: Study,
    series_uid: String,
    series: Series,
    instance: Option<InstanceHeader>,
}

/// Reads headers in parallel and groups them by Study/Series Instance UID.
pub fn index_files(files: &[PathBuf], progress: &Progress) -> Result<StudyIndex> {
    progress.begin("Reading image headers", files.len());
    let records: Vec<std::result::Result<HeaderRecord, String>> = files
        .par_iter()
        .map(|path| {
            if progress.is_cancelled() {
                return Err("cancelled".to_owned());
            }
            let record = catch_unwind(AssertUnwindSafe(|| read_header(path)))
                .unwrap_or_else(|_| Err("parser panicked".to_owned()));
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
            stats.non_images += 1;
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
        let ri = *series_pos.entry((si, rec.series_uid.clone())).or_insert_with(|| {
            study.series.push(rec.series);
            study.series.len() - 1
        });
        study.series[ri].instances.push(instance);
    }

    for study in &mut studies {
        study.series.retain(|s| !s.instances.is_empty());
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
        });
    }
    studies.sort_by(|a, b| (&a.date, &a.time).cmp(&(&b.date, &b.time)));

    let mut warnings = Vec::new();
    if stats.unreadable > 0 {
        warnings.push(format!("{} file(s) could not be read and were skipped", stats.unreadable));
    }
    if stats.non_images > 0 {
        warnings.push(format!("{} non-image object(s) (reports, annotations) are not shown", stats.non_images));
    }
    if stats.unreadable > 0 {
        warn!("{} unreadable files during indexing", stats.unreadable);
    }
    Ok(StudyIndex { root: PathBuf::new(), source: DiscoverySource::FolderScan, studies, stats, warnings })
}

fn read_header(path: &Path) -> std::result::Result<HeaderRecord, String> {
    let file = OpenFileOptions::new().read_until(tags::PIXEL_DATA).open_file(path).map_err(|e| e.to_string())?;
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
    };
    let series = Series {
        uid: series_uid.clone(),
        number: attrs::int(obj, tags::SERIES_NUMBER),
        description: attrs::text(obj, tags::SERIES_DESCRIPTION),
        modality: attrs::text(obj, tags::MODALITY).unwrap_or_else(|| "OT".to_owned()),
        body_part: attrs::text(obj, tags::BODY_PART_EXAMINED),
        instances: Vec::new(),
    };

    let rows = attrs::uint(obj, tags::ROWS).unwrap_or(0);
    let cols = attrs::uint(obj, tags::COLUMNS).unwrap_or(0);
    let instance = (rows > 0 && cols > 0).then(|| {
        let frames = attrs::uint(obj, tags::NUMBER_OF_FRAMES).unwrap_or(1).max(1);
        let (pixel_spacing, frame_planes) = geometry(obj, rows, cols, frames);
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
            frame_planes,
            image_type: attrs::texts(obj, tags::IMAGE_TYPE),
        }
    });
    Ok(HeaderRecord { study_uid, study, series_uid, series, instance })
}

/// Pixel spacing and per-frame geometry, from classic attributes or the
/// Enhanced multi-frame functional groups (PS3.3 C.7.6.16).
fn geometry(obj: &InMemDicomObject, rows: u32, cols: u32, frames: u32) -> (Option<[f64; 2]>, Vec<Option<ImagePlane>>) {
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
    let pixel_spacing = ps
        .or_else(|| per_frame.first().and_then(|f| from_group(Some(f)).2))
        .or_else(|| attrs::float_array::<2>(obj, tags::IMAGER_PIXEL_SPACING))
        .filter(|s| s[0] > 0.0 && s[1] > 0.0);
    (pixel_spacing, planes)
}
