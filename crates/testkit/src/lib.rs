//! Synthetic, PHI-free DICOM fixtures.
//!
//! [`write_phantom_study`] writes a small study laid out like a patient CD:
//!
//! ```text
//! <root>/DICOMDIR
//! <root>/DICOM/ST000001/SE00000N/IM00000M   (no file extensions)
//! <root>/AUTORUN.INF, README.TXT, VIEWER/…  (non-DICOM clutter)
//! ```
//!
//! Series:
//! 1. CT chest phantom, axial, 16-bit with rescale intercept −1024 (HU),
//!    instance numbers running superior → inferior, files shuffled; a
//!    contrast agent is recorded.
//! 2. MR T2 sagittal phantom (turbo spin echo, TR 4000 / TE 96 ms, with a
//!    protocol name), 16-bit unsigned, multiple window presets.
//! 3. Three-plane localizer, 8-bit, **JPEG Baseline** compressed.
//! 4. RGB secondary capture ("dose report").
//!
//! A bright marker sphere sits at [`MARKER_POSITION`] (patient left,
//! anterior, superior) in the CT and MR so tests can verify orientation.
//!
//! The CT holds air around the body (−1000 HU), soft tissue
//! ([`CT_SOFT_TISSUE_HU`]), two air-filled lungs (about −820 HU), a spine-like
//! bone cylinder ([`CT_BONE_HU`]) and a contrast-filled vessel
//! ([`CT_VESSEL_HU`]) running from head to feet in front of it.
//! [`CtPhantom`] gives the same voxels in memory, without writing files.
//!
//! [`archive`] packs such a folder into a ZIP archive or an ISO 9660 disc
//! image, the forms in which a disc is handed out as a download.
//!
//! [`stacks`] writes series with several sets of images at the same slice
//! positions (time points, echoes, b-values) and a cine of one slice.
//!
//! [`fidelity`] writes images that are standard but unusual: palette colour,
//! contrast tables, a Modality LUT, padding, overlay planes, lossy and
//! burned-in facts.
//!
//! [`comparison`] writes two studies of one patient, a year apart, with series
//! whose slice positions are known exactly.
//!
//! [`pet`] writes a PET/CT study: this CT and PET series in its Frame of
//! Reference with a hot sphere of known uptake (SUV 5) at the marker.
//!
//! [`measure`] writes images of known size and known values for measuring.
//!
//! [`reports`] adds structured reports, a dose report, a PDF stored in DICOM
//! and loose report files to this CD, and writes PDF files.
//!
//! [`export`] writes images whose grey values and proportions can be checked
//! in the files that come out of saving, exporting and printing.

pub mod archive;
pub mod comparison;
pub mod export;
pub mod fidelity;
pub mod measure;
pub mod pet;
pub mod reports;
pub mod stacks;

use std::fs;
use std::path::{Path, PathBuf};

use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, Length, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{InMemDicomObject, meta::FileMetaTableBuilder};
use dicom_pixeldata::Transcode;

/// Fake patient name used by every fixture.
pub const PATIENT_NAME: &str = "PHANTOM^SYNTHETIC";
pub const PATIENT_ID: &str = "SYN-0001";
/// Institution name with non-ASCII characters (written as UTF-8, ISO_IR 192).
pub const INSTITUTION: &str = "Klinikum Musterstadt — Röntgen";
/// Centre of the marker sphere in patient coordinates (mm, LPS).
pub const MARKER_POSITION: [f64; 3] = [40.0, -35.0, 50.0];
pub const MARKER_RADIUS_MM: f64 = 10.0;
/// HU of the CT marker.
pub const CT_MARKER_HU: f64 = 300.0;
pub const CT_SOFT_TISSUE_HU: f64 = 40.0;
/// HU of the spine-like bone cylinder of the CT.
pub const CT_BONE_HU: f64 = 700.0;
/// Axis of the bone cylinder (patient x and y in mm; it runs along z) and its radius.
pub const CT_BONE_AXIS_MM: [f64; 2] = [0.0, 55.0];
pub const CT_BONE_RADIUS_MM: f64 = 14.0;
/// HU of the contrast-filled vessel of the CT.
pub const CT_VESSEL_HU: f64 = 300.0;
/// Axis of the vessel (patient x and y in mm: left of the midline, in front of
/// the bone cylinder; it runs along z) and its radius.
pub const CT_VESSEL_AXIS_MM: [f64; 2] = [10.0, 25.0];
pub const CT_VESSEL_RADIUS_MM: f64 = 8.0;

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("DICOM error: {0}")]
    Dicom(String),
}

fn dicom_err(e: impl std::fmt::Display) -> FixtureError {
    FixtureError::Dicom(e.to_string())
}

#[derive(Debug, Clone)]
pub struct PhantomOptions {
    pub ct_size: u32,
    pub ct_slices: u32,
    pub mr_size: u32,
    pub mr_slices: u32,
    pub write_dicomdir: bool,
    pub include_localizer: bool,
    pub include_secondary_capture: bool,
    /// Non-DICOM files that a real CD carries (autorun, viewer software…).
    pub include_clutter: bool,
}

impl Default for PhantomOptions {
    /// Realistic sizes for manual QA.
    fn default() -> Self {
        Self {
            ct_size: 256,
            ct_slices: 120,
            mr_size: 192,
            mr_slices: 24,
            write_dicomdir: true,
            include_localizer: true,
            include_secondary_capture: true,
            include_clutter: true,
        }
    }
}

impl PhantomOptions {
    /// Small sizes for fast automated tests.
    pub fn small() -> Self {
        Self { ct_size: 48, ct_slices: 24, mr_size: 40, mr_slices: 12, ..Self::default() }
    }
}

#[derive(Debug, Clone)]
pub struct WrittenSeries {
    pub uid: String,
    pub number: u32,
    pub modality: &'static str,
    pub description: &'static str,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WrittenStudy {
    pub root: PathBuf,
    pub study_uid: String,
    pub series: Vec<WrittenSeries>,
    pub dicomdir: Option<PathBuf>,
}

impl WrittenStudy {
    pub fn series(&self, modality: &str) -> Option<&WrittenSeries> {
        self.series.iter().find(|s| s.modality == modality)
    }
}

const FOV_MM: f64 = 240.0;
const UID_ROOT: &str = "2.25.31415926535897932384626433832";

/// Writes the phantom study below `root` (created if needed).
pub fn write_phantom_study(root: &Path, opts: &PhantomOptions) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}0");
    let study_dir = root.join("DICOM").join("ST000001");
    let mut series = Vec::new();

    series.push(write_ct_series(&study_dir, &study_uid, opts)?);
    series.push(write_mr_series(&study_dir, &study_uid, opts)?);
    if opts.include_localizer {
        series.push(write_localizer_series(&study_dir, &study_uid)?);
    }
    if opts.include_secondary_capture {
        series.push(write_secondary_capture(&study_dir, &study_uid)?);
    }
    if opts.include_clutter {
        write_clutter(root)?;
    }
    let dicomdir = if opts.write_dicomdir { Some(write_dicomdir(root, &study_uid, &series)?) } else { None };
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir })
}

// ---------------------------------------------------------------------------
// Phantoms (patient coordinates in mm)

fn noise(p: [f64; 3]) -> f64 {
    // Deterministic hash noise in [-1, 1].
    let h = (p[0] * 12.9898 + p[1] * 78.233 + p[2] * 37.719).sin() * 43_758.545_3;
    (h - h.floor()) * 2.0 - 1.0
}

fn in_ellipsoid(p: [f64; 3], c: [f64; 3], r: [f64; 3]) -> bool {
    (0..3).map(|i| ((p[i] - c[i]) / r[i]).powi(2)).sum::<f64>() <= 1.0
}

/// Inside the cylinder of radius `r` along z through (`axis[0]`, `axis[1]`).
fn in_cylinder(p: [f64; 3], axis: [f64; 2], r: f64) -> bool {
    ((p[0] - axis[0]) / r).powi(2) + ((p[1] - axis[1]) / r).powi(2) <= 1.0
}

fn in_marker(p: [f64; 3]) -> bool {
    (0..3).map(|i| (p[i] - MARKER_POSITION[i]).powi(2)).sum::<f64>().sqrt() <= MARKER_RADIUS_MM
}

/// CT chest phantom in Hounsfield units.
fn ct_hu(p: [f64; 3]) -> f64 {
    let [x, y, _] = p;
    if (x / 100.0).powi(2) + (y / 80.0).powi(2) > 1.0 {
        return -1000.0;
    }
    let mut hu = CT_SOFT_TISSUE_HU + 6.0 * noise(p);
    for side in [-1.0, 1.0] {
        if in_ellipsoid(p, [side * 45.0, -5.0, 0.0], [30.0, 45.0, 70.0]) {
            hu = -820.0 + 15.0 * noise(p);
        }
    }
    if in_cylinder(p, CT_BONE_AXIS_MM, CT_BONE_RADIUS_MM) {
        hu = CT_BONE_HU;
    }
    if in_cylinder(p, CT_VESSEL_AXIS_MM, CT_VESSEL_RADIUS_MM) {
        hu = CT_VESSEL_HU;
    }
    if in_marker(p) {
        hu = CT_MARKER_HU;
    }
    hu
}

/// MR (T2-like) phantom signal intensity.
fn mr_signal(p: [f64; 3]) -> f64 {
    if !in_ellipsoid(p, [0.0, 0.0, 0.0], [100.0, 80.0, 110.0]) {
        return 0.0;
    }
    let mut s = 320.0 + 20.0 * noise(p);
    if in_ellipsoid(p, [0.0, 0.0, 30.0], [60.0, 55.0, 50.0]) {
        s = 820.0 + 30.0 * noise(p);
    }
    if in_marker(p) {
        s = 1500.0;
    }
    s
}

// ---------------------------------------------------------------------------
// Series writers

struct Geometry {
    position: [f64; 3],
    orientation: [f64; 6],
    spacing: f64,
}

impl Geometry {
    fn pixel(&self, col: u32, row: u32) -> [f64; 3] {
        let o = &self.orientation;
        let (c, r) = (f64::from(col) * self.spacing, f64::from(row) * self.spacing);
        [
            self.position[0] + o[0] * c + o[3] * r,
            self.position[1] + o[1] * c + o[4] * r,
            self.position[2] + o[2] * c + o[5] * r,
        ]
    }
}

/// Slice `k` (counted from the feet) of the CT series: 240 mm field of view,
/// 200 mm covered by `n` slices.
fn ct_slice_geometry(size: u32, n: u32, k: u32) -> Geometry {
    let spacing = FOV_MM / f64::from(size);
    let thickness = 200.0 / f64::from(n);
    let half = (f64::from(size) - 1.0) * spacing / 2.0;
    let z = -100.0 + thickness / 2.0 + f64::from(k) * thickness;
    Geometry { position: [-half, -half, z], orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], spacing }
}

/// The CT series of [`write_phantom_study`] in memory: the same voxels, in
/// Hounsfield units, without writing or decoding files.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CtPhantom {
    size: u32,
    slices: u32,
}

impl CtPhantom {
    pub fn new(opts: &PhantomOptions) -> Self {
        Self { size: opts.ct_size, slices: opts.ct_slices }
    }

    /// Columns, rows and slices.
    pub fn dims(&self) -> [usize; 3] {
        [self.size as usize, self.size as usize, self.slices as usize]
    }

    /// Patient position (mm, LPS) of the first voxel of the lowest slice.
    pub fn origin(&self) -> [f64; 3] {
        ct_slice_geometry(self.size, self.slices, 0).position
    }

    /// Voxel size in mm: columns run to the patient's left (+x), rows to the
    /// back (+y) and slices from the feet to the head (+z).
    pub fn spacing(&self) -> [f64; 3] {
        let geometry = ct_slice_geometry(self.size, self.slices, 0);
        [geometry.spacing, geometry.spacing, 200.0 / f64::from(self.slices)]
    }

    /// Hounsfield units of slice `k` (counted from the feet), row by row.
    pub fn slice(&self, k: usize) -> Vec<i16> {
        let geometry = ct_slice_geometry(self.size, self.slices, k as u32);
        let size = self.size;
        (0..size * size).map(|i| ct_hu(geometry.pixel(i % size, i / size)).round() as i16).collect()
    }
}

fn write_ct_series(study_dir: &Path, study_uid: &str, opts: &PhantomOptions) -> Result<WrittenSeries, FixtureError> {
    let number = 1;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let n = opts.ct_slices;
    let size = opts.ct_size;
    let thickness = 200.0 / f64::from(n);
    let mut files = Vec::new();
    // Write slices in a scrambled order to make sure readers sort spatially.
    let mut order: Vec<u32> = (0..n).collect();
    order.sort_by_key(|k| (k * 7919) % n);
    for (file_index, k) in order.into_iter().enumerate() {
        let geom = ct_slice_geometry(size, n, k);
        let z = geom.position[2];
        let pixels: Vec<u16> =
            (0..size * size).map(|i| (ct_hu(geom.pixel(i % size, i / size)).round() + 1024.0) as u16).collect();
        // Instance 1 is the most superior slice.
        let instance_number = n - k;
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance_number}"),
            uids::CT_IMAGE_STORAGE,
            "CT",
            number,
            "CT CHEST 2.5mm (synthetic)",
            instance_number,
        );
        put_geometry(&mut obj, &geom, size, size);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "AXIAL"]);
        put_ds(&mut obj, tags::SLICE_THICKNESS, &[thickness]);
        put_ds(&mut obj, tags::SLICE_LOCATION, &[z]);
        put_ds(&mut obj, tags::KVP, &[120.0]);
        put_str(&mut obj, tags::X_RAY_TUBE_CURRENT, VR::IS, &["210"]);
        put_str(&mut obj, tags::EXPOSURE, VR::IS, &["150"]);
        put_str(&mut obj, tags::CONVOLUTION_KERNEL, VR::SH, &["STANDARD"]);
        put_str(&mut obj, tags::CONTRAST_BOLUS_AGENT, VR::LO, &["Iodinated contrast (synthetic)"]);
        put_str(&mut obj, tags::BODY_PART_EXAMINED, VR::CS, &["CHEST"]);
        put_ds(&mut obj, tags::RESCALE_INTERCEPT, &[-1024.0]);
        put_ds(&mut obj, tags::RESCALE_SLOPE, &[1.0]);
        put_str(&mut obj, tags::RESCALE_TYPE, VR::LO, &["HU"]);
        put_ds(&mut obj, tags::WINDOW_CENTER, &[40.0, -600.0]);
        put_ds(&mut obj, tags::WINDOW_WIDTH, &[400.0, 1500.0]);
        put_str(&mut obj, tags::WINDOW_CENTER_WIDTH_EXPLANATION, VR::LO, &["MEDIASTINUM", "LUNG"]);
        put_mono16(&mut obj, size, size, 12, pixels);
        let path = dir.join(format!("IM{:06}", file_index + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number, modality: "CT", description: "CT CHEST 2.5mm (synthetic)", files })
}

fn write_mr_series(study_dir: &Path, study_uid: &str, opts: &PhantomOptions) -> Result<WrittenSeries, FixtureError> {
    let number = 2;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let n = opts.mr_slices;
    let size = opts.mr_size;
    let spacing = 220.0 / f64::from(size);
    let half = (f64::from(size) - 1.0) * spacing / 2.0;
    let gap = 120.0 / f64::from(n - 1);
    let mut files = Vec::new();
    for k in 0..n {
        let x = 60.0 - f64::from(k) * gap;
        // Sagittal: rows run posterior (+Y), columns run inferior (−Z).
        let geom = Geometry { position: [x, -half, half], orientation: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0], spacing };
        let pixels: Vec<u16> =
            (0..size * size).map(|i| mr_signal(geom.pixel(i % size, i / size)).round() as u16).collect();
        let instance_number = k + 1;
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance_number}"),
            uids::MR_IMAGE_STORAGE,
            "MR",
            number,
            "T2 TSE SAG (synthetic)",
            instance_number,
        );
        put_geometry(&mut obj, &geom, size, size);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "ND"]);
        put_ds(&mut obj, tags::SLICE_THICKNESS, &[4.0]);
        put_ds(&mut obj, tags::SPACING_BETWEEN_SLICES, &[gap]);
        put_ds(&mut obj, tags::SLICE_LOCATION, &[x]);
        put_ds(&mut obj, tags::REPETITION_TIME, &[4000.0]);
        put_ds(&mut obj, tags::ECHO_TIME, &[96.0]);
        put_ds(&mut obj, tags::FLIP_ANGLE, &[150.0]);
        put_ds(&mut obj, tags::MAGNETIC_FIELD_STRENGTH, &[1.5]);
        put_ds(&mut obj, tags::IMAGING_FREQUENCY, &[63.6]);
        put_str(&mut obj, tags::ECHO_TRAIN_LENGTH, VR::IS, &["15"]);
        put_str(&mut obj, tags::SCANNING_SEQUENCE, VR::CS, &["SE"]);
        put_str(&mut obj, tags::SEQUENCE_VARIANT, VR::CS, &["SK", "SP"]);
        put_str(&mut obj, tags::MR_ACQUISITION_TYPE, VR::CS, &["2D"]);
        put_str(&mut obj, tags::SEQUENCE_NAME, VR::SH, &["*tse2d1_15"]);
        put_str(&mut obj, tags::PROTOCOL_NAME, VR::LO, &["t2_tse_sag"]);
        put_str(&mut obj, tags::BODY_PART_EXAMINED, VR::CS, &["CHEST"]);
        put_ds(&mut obj, tags::WINDOW_CENTER, &[700.0, 900.0]);
        put_ds(&mut obj, tags::WINDOW_WIDTH, &[1400.0, 1000.0]);
        put_mono16(&mut obj, size, size, 12, pixels);
        let path = dir.join(format!("IM{:06}", k + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number, modality: "MR", description: "T2 TSE SAG (synthetic)", files })
}

fn write_localizer_series(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 3;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let size = 64u32;
    let spacing = 260.0 / f64::from(size);
    let h = (f64::from(size) - 1.0) * spacing / 2.0;
    let planes = [
        // Axial, coronal, sagittal through the isocentre.
        ([-h, -h, 0.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
        ([-h, 0.0, h], [1.0, 0.0, 0.0, 0.0, 0.0, -1.0]),
        ([0.0, -h, h], [0.0, 1.0, 0.0, 0.0, 0.0, -1.0]),
    ];
    let mut files = Vec::new();
    for (k, (position, orientation)) in planes.into_iter().enumerate() {
        let geom = Geometry { position, orientation, spacing };
        let pixels: Vec<u8> = (0..size * size)
            .map(|i| (mr_signal(geom.pixel(i % size, i / size)) / 1500.0 * 255.0).min(255.0) as u8)
            .collect();
        let instance_number = k as u32 + 1;
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance_number}"),
            uids::MR_IMAGE_STORAGE,
            "MR",
            number,
            "Localizer 3-plane (synthetic)",
            instance_number,
        );
        put_geometry(&mut obj, &geom, size, size);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "LOCALIZER"]);
        put_u16(&mut obj, tags::SAMPLES_PER_PIXEL, 1);
        put_str(&mut obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]);
        put_u16(&mut obj, tags::ROWS, size as u16);
        put_u16(&mut obj, tags::COLUMNS, size as u16);
        put_u16(&mut obj, tags::BITS_ALLOCATED, 8);
        put_u16(&mut obj, tags::BITS_STORED, 8);
        put_u16(&mut obj, tags::HIGH_BIT, 7);
        put_u16(&mut obj, tags::PIXEL_REPRESENTATION, 0);
        obj.put(DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::U8(pixels.into())));
        let path = dir.join(format!("IM{:06}", k + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, Some(Compression::JpegBaseline))?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number, modality: "MR", description: "Localizer 3-plane (synthetic)", files })
}

fn write_secondary_capture(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 99;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let (w, h) = (160u32, 100u32);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let band = x * 4 / w;
            let v = (255 - y * 2) as u8;
            let pixel: [u8; 3] = match band {
                0 => [v, 40, 40],
                1 => [40, v, 40],
                2 => [40, 40, v],
                _ => [v, v, v],
            };
            rgb.extend_from_slice(&pixel);
        }
    }
    let mut obj = base_object(
        study_uid,
        &uid,
        &format!("{uid}.1"),
        uids::SECONDARY_CAPTURE_IMAGE_STORAGE,
        "OT",
        number,
        "Dose report (synthetic)",
        1,
    );
    put_u16(&mut obj, tags::SAMPLES_PER_PIXEL, 3);
    put_str(&mut obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["RGB"]);
    put_u16(&mut obj, tags::PLANAR_CONFIGURATION, 0);
    put_u16(&mut obj, tags::ROWS, h as u16);
    put_u16(&mut obj, tags::COLUMNS, w as u16);
    put_u16(&mut obj, tags::BITS_ALLOCATED, 8);
    put_u16(&mut obj, tags::BITS_STORED, 8);
    put_u16(&mut obj, tags::HIGH_BIT, 7);
    put_u16(&mut obj, tags::PIXEL_REPRESENTATION, 0);
    obj.put(DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::U8(rgb.into())));
    let path = dir.join("IM000001");
    write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
    Ok(WrittenSeries { uid, number, modality: "OT", description: "Dose report (synthetic)", files: vec![path] })
}

fn write_clutter(root: &Path) -> Result<(), FixtureError> {
    fs::create_dir_all(root.join("VIEWER"))?;
    fs::write(root.join("AUTORUN.INF"), "[autorun]\r\nopen=VIEWER\\viewer.exe\r\n")?;
    fs::write(root.join("README.TXT"), "Synthetic test CD - contains no patient data.\r\n")?;
    fs::write(root.join("VIEWER").join("viewer.exe"), [0x4d, 0x5a, 0x90, 0x00, 0x03])?;
    // 200 bytes of noise without the DICM magic.
    fs::write(root.join("VIEWER").join("DATA0001"), vec![0x42u8; 200])?;
    Ok(())
}

fn write_dicomdir(root: &Path, study_uid: &str, series: &[WrittenSeries]) -> Result<PathBuf, FixtureError> {
    let mut records = Vec::new();
    let record = |kind: &str| {
        let mut item = InMemDicomObject::new_empty();
        item.put(DataElement::new(tags::OFFSET_OF_THE_NEXT_DIRECTORY_RECORD, VR::UL, PrimitiveValue::from(0u32)));
        item.put(DataElement::new(tags::RECORD_IN_USE_FLAG, VR::US, PrimitiveValue::from(0xFFFFu16)));
        item.put(DataElement::new(
            tags::OFFSET_OF_REFERENCED_LOWER_LEVEL_DIRECTORY_ENTITY,
            VR::UL,
            PrimitiveValue::from(0u32),
        ));
        item.put(DataElement::new(tags::DIRECTORY_RECORD_TYPE, VR::CS, PrimitiveValue::from(kind)));
        item
    };
    let mut patient = record("PATIENT");
    put_str(&mut patient, tags::PATIENT_NAME, VR::PN, &[PATIENT_NAME]);
    put_str(&mut patient, tags::PATIENT_ID, VR::LO, &[PATIENT_ID]);
    records.push(patient);
    let mut study = record("STUDY");
    put_str(&mut study, tags::STUDY_INSTANCE_UID, VR::UI, &[study_uid]);
    put_str(&mut study, tags::STUDY_DATE, VR::DA, &["20260115"]);
    records.push(study);
    for s in series {
        let mut rec = record("SERIES");
        put_str(&mut rec, tags::SERIES_INSTANCE_UID, VR::UI, &[s.uid.as_str()]);
        put_str(&mut rec, tags::MODALITY, VR::CS, &[s.modality]);
        records.push(rec);
        for file in &s.files {
            let rel = file.strip_prefix(root).map_err(dicom_err)?;
            let components: Vec<String> =
                rel.components().map(|c| c.as_os_str().to_string_lossy().to_uppercase()).collect();
            let mut img = record("IMAGE");
            img.put(DataElement::new(
                tags::REFERENCED_FILE_ID,
                VR::CS,
                PrimitiveValue::Strs(components.into_iter().collect()),
            ));
            records.push(img);
        }
    }
    let mut dir = InMemDicomObject::new_empty();
    put_str(&mut dir, tags::FILE_SET_ID, VR::CS, &["SUMMRISE_TEST"]);
    dir.put(DataElement::new(
        tags::OFFSET_OF_THE_FIRST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
        VR::UL,
        PrimitiveValue::from(0u32),
    ));
    dir.put(DataElement::new(
        tags::OFFSET_OF_THE_LAST_DIRECTORY_RECORD_OF_THE_ROOT_DIRECTORY_ENTITY,
        VR::UL,
        PrimitiveValue::from(0u32),
    ));
    dir.put(DataElement::new(tags::FILE_SET_CONSISTENCY_FLAG, VR::US, PrimitiveValue::from(0u16)));
    dir.put(DataElement::new(
        tags::DIRECTORY_RECORD_SEQUENCE,
        VR::SQ,
        DataSetSequence::new(records, Length::UNDEFINED),
    ));
    let path = root.join("DICOMDIR");
    let meta = FileMetaTableBuilder::new()
        .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
        .media_storage_sop_class_uid(uids::MEDIA_STORAGE_DIRECTORY_STORAGE)
        .media_storage_sop_instance_uid(format!("{UID_ROOT}9999"));
    dir.with_meta(meta).map_err(dicom_err)?.write_to_file(&path).map_err(dicom_err)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Element helpers

#[allow(clippy::too_many_arguments)]
fn base_object(
    study_uid: &str,
    series_uid: &str,
    sop_uid: &str,
    sop_class: &str,
    modality: &str,
    series_number: u32,
    series_description: &str,
    instance_number: u32,
) -> InMemDicomObject {
    let mut obj = InMemDicomObject::new_empty();
    put_str(&mut obj, tags::SPECIFIC_CHARACTER_SET, VR::CS, &["ISO_IR 192"]);
    put_str(&mut obj, tags::SOP_CLASS_UID, VR::UI, &[sop_class]);
    put_str(&mut obj, tags::SOP_INSTANCE_UID, VR::UI, &[sop_uid]);
    put_str(&mut obj, tags::STUDY_DATE, VR::DA, &["20260115"]);
    put_str(&mut obj, tags::STUDY_TIME, VR::TM, &["093000"]);
    put_str(&mut obj, tags::ACCESSION_NUMBER, VR::SH, &["SYN123"]);
    put_str(&mut obj, tags::MODALITY, VR::CS, &[modality]);
    put_str(&mut obj, tags::MANUFACTURER, VR::LO, &["SumMRIse Synthetic"]);
    put_str(&mut obj, tags::MANUFACTURER_MODEL_NAME, VR::LO, &["Phantom 1.0"]);
    put_str(&mut obj, tags::INSTITUTION_NAME, VR::LO, &[INSTITUTION]);
    put_str(&mut obj, tags::REFERRING_PHYSICIAN_NAME, VR::PN, &["DOE^JANE"]);
    put_str(&mut obj, tags::STUDY_DESCRIPTION, VR::LO, &["Synthetic phantom study"]);
    put_str(&mut obj, tags::SERIES_DESCRIPTION, VR::LO, &[series_description]);
    put_str(&mut obj, tags::PATIENT_NAME, VR::PN, &[PATIENT_NAME]);
    put_str(&mut obj, tags::PATIENT_ID, VR::LO, &[PATIENT_ID]);
    put_str(&mut obj, tags::PATIENT_BIRTH_DATE, VR::DA, &["19700101"]);
    put_str(&mut obj, tags::PATIENT_SEX, VR::CS, &["O"]);
    put_str(&mut obj, tags::PATIENT_AGE, VR::AS, &["056Y"]);
    put_str(&mut obj, tags::PATIENT_POSITION, VR::CS, &["HFS"]);
    put_str(&mut obj, tags::STUDY_INSTANCE_UID, VR::UI, &[study_uid]);
    put_str(&mut obj, tags::SERIES_INSTANCE_UID, VR::UI, &[series_uid]);
    put_str(&mut obj, tags::STUDY_ID, VR::SH, &["1"]);
    put_str(&mut obj, tags::SERIES_NUMBER, VR::IS, &[&series_number.to_string()]);
    put_str(&mut obj, tags::INSTANCE_NUMBER, VR::IS, &[&instance_number.to_string()]);
    put_str(&mut obj, tags::FRAME_OF_REFERENCE_UID, VR::UI, &[&format!("{UID_ROOT}7777")]);
    obj
}

fn put_geometry(obj: &mut InMemDicomObject, geom: &Geometry, rows: u32, cols: u32) {
    put_ds(obj, tags::IMAGE_POSITION_PATIENT, &geom.position);
    put_ds(obj, tags::IMAGE_ORIENTATION_PATIENT, &geom.orientation);
    put_ds(obj, tags::PIXEL_SPACING, &[geom.spacing, geom.spacing]);
    put_u16(obj, tags::ROWS, rows as u16);
    put_u16(obj, tags::COLUMNS, cols as u16);
}

fn put_mono16(obj: &mut InMemDicomObject, rows: u32, cols: u32, bits_stored: u16, pixels: Vec<u16>) {
    put_u16(obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]);
    put_u16(obj, tags::ROWS, rows as u16);
    put_u16(obj, tags::COLUMNS, cols as u16);
    put_u16(obj, tags::BITS_ALLOCATED, 16);
    put_u16(obj, tags::BITS_STORED, bits_stored);
    put_u16(obj, tags::HIGH_BIT, bits_stored - 1);
    put_u16(obj, tags::PIXEL_REPRESENTATION, 0);
    obj.put(DataElement::new(tags::PIXEL_DATA, VR::OW, PrimitiveValue::U16(pixels.into())));
}

fn put_str(obj: &mut InMemDicomObject, tag: Tag, vr: VR, values: &[&str]) {
    let value = match values {
        [single] => PrimitiveValue::from(*single),
        many => PrimitiveValue::Strs(many.iter().map(|s| (*s).to_owned()).collect()),
    };
    obj.put(DataElement::new(tag, vr, value));
}

fn put_u16(obj: &mut InMemDicomObject, tag: Tag, value: u16) {
    obj.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
}

/// Decimal strings (DS) limited to 16 characters.
fn put_ds(obj: &mut InMemDicomObject, tag: Tag, values: &[f64]) {
    let strs: Vec<String> = values
        .iter()
        .map(|v| {
            let mut s = format!("{v:.6}");
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
            }
            s.truncate(16);
            s
        })
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    put_str(obj, tag, VR::DS, &refs);
}

enum Compression {
    JpegBaseline,
}

fn write_file(
    obj: InMemDicomObject,
    path: &Path,
    transfer_syntax: &str,
    compression: Option<Compression>,
) -> Result<(), FixtureError> {
    let mut file = obj.with_meta(FileMetaTableBuilder::new().transfer_syntax(transfer_syntax)).map_err(dicom_err)?;
    if let Some(Compression::JpegBaseline) = compression {
        file.transcode(&dicom_transfer_syntax_registry::entries::JPEG_BASELINE.erased()).map_err(dicom_err)?;
    }
    file.write_to_file(path).map_err(dicom_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_cd_like_layout() {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let study = write_phantom_study(&dir, &PhantomOptions::small()).expect("fixture");
        assert_eq!(study.series.len(), 4);
        assert!(study.dicomdir.as_ref().is_some_and(|p| p.exists()));
        assert_eq!(study.series("CT").map(|s| s.files.len()), Some(24));
        assert!(dir.join("AUTORUN.INF").exists());
        // Files carry the DICM magic after the preamble.
        let bytes = fs::read(&study.series[0].files[0]).expect("read");
        assert_eq!(&bytes[128..132], b"DICM");
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
