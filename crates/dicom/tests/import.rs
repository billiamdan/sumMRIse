//! End-to-end import tests against the synthetic CD written by
//! `summrise-testkit` (no real patient data involved).

use std::path::Path;

use summrise_dicom::{
    Acquisition, DiscoverySource, Media, Progress, SeriesImages, discover, format, load_series, read_metadata, scan,
    thumbnail,
};
use summrise_imaging::{AnatomicalPlane, DVec3, MprAxis, VolumeError, reslice};
use summrise_testkit::{
    CT_BONE_AXIS_MM, CT_BONE_HU, CT_MARKER_HU, CT_SOFT_TISSUE_HU, CT_VESSEL_AXIS_MM, CT_VESSEL_HU, CtPhantom,
    INSTITUTION, MARKER_POSITION, PATIENT_NAME, PhantomOptions, WrittenStudy, write_phantom_study,
};

fn fixture(opts: PhantomOptions) -> (tempfile::TempDir, WrittenStudy) {
    let dir = tempfile::tempdir().expect("tempdir");
    let study = write_phantom_study(dir.path(), &opts).expect("write fixture");
    (dir, study)
}

#[test]
fn dicomdir_is_used_and_clutter_ignored() {
    let (dir, written) = fixture(PhantomOptions::small());
    let discovery = discover(dir.path(), &Progress::new()).expect("discover");
    assert!(matches!(discovery.source, DiscoverySource::Dicomdir(_)));
    let expected: usize = written.series.iter().map(|s| s.files.len()).sum();
    assert_eq!(discovery.files.len(), expected);
    assert!(discovery.files.iter().all(|f| !f.to_string_lossy().contains("VIEWER")));
}

/// A DICOMDIR that lists many missing files is not used; the user is told why
/// and the folder is scanned instead.
#[test]
fn unusable_dicomdir_falls_back_to_a_scan_and_says_why() {
    let (_dir, written) = fixture(PhantomOptions::small());
    let ct = written.series("CT").expect("CT");
    // Rename a third of the CT files: the DICOMDIR no longer finds them, a scan does.
    let hidden = ct.files.len() / 3;
    for file in &ct.files[..hidden] {
        let renamed = file.with_file_name(format!("MOVED{}", file.file_name().expect("name").to_string_lossy()));
        std::fs::rename(file, renamed).expect("rename");
    }
    let found = discover(&written.root, &Progress::new()).expect("discover");
    assert_eq!(found.source, DiscoverySource::FolderScan);
    assert!(
        found.warnings.iter().any(|w| w.contains("the folder was scanned instead")),
        "the reason is shown: {:?}",
        found.warnings
    );
    let index = scan(&written.root, &Progress::new()).expect("scan");
    let ct_count: usize =
        index.studies.iter().flat_map(|s| &s.series).filter(|s| s.modality == "CT").map(|s| s.instances.len()).sum();
    assert_eq!(ct_count, ct.files.len(), "the scan finds every CT image, including the renamed ones");
    assert!(index.warnings.iter().any(|w| w.contains("the folder was scanned instead")));
}

#[test]
fn folder_scan_without_dicomdir_finds_the_same_images() {
    let (dir, written) = fixture(PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() });
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    assert_eq!(index.source, DiscoverySource::FolderScan);
    assert_eq!(index.studies.len(), 1);
    assert_eq!(index.studies[0].series.len(), written.series.len());
    assert_eq!(index.stats.unreadable, 0, "clutter must be filtered before parsing");
}

#[test]
fn index_groups_series_with_metadata() {
    let (dir, written) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let study = &index.studies[0];
    assert_eq!(study.uid, written.study_uid);
    assert_eq!(study.patient.name.as_deref(), Some(PATIENT_NAME));
    assert_eq!(study.institution.as_deref(), Some(INSTITUTION), "UTF-8 (ISO_IR 192) decoded");
    assert_eq!(study.modalities(), vec!["CT", "MR", "OT"]);
    let numbers: Vec<_> = study.series.iter().map(|s| s.number).collect();
    assert_eq!(numbers, vec![Some(1), Some(2), Some(3), Some(99)], "sorted by series number");

    let ct = &study.series[0];
    assert_eq!(ct.plane(), Some(AnatomicalPlane::Axial));
    assert_eq!(ct.frame_count(), 24);
    let mr = &study.series[1];
    assert_eq!(mr.plane(), Some(AnatomicalPlane::Sagittal));
    let localizer = &study.series[2];
    assert!(localizer.has_mixed_planes());
    assert!(localizer.unsupported_transfer_syntaxes().is_empty(), "JPEG baseline is decodable");
}

#[test]
fn ct_decodes_to_hounsfield_units_in_spatial_order() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let ct = &index.studies[0].series[0];
    let loaded = load_series(&index.media, ct, &Progress::new()).expect("decode CT");
    assert_eq!(loaded.units.as_deref(), Some("HU"));
    assert!(loaded.volume_issue.is_none(), "{:?}", loaded.volume_issue);
    let SeriesImages::Volume(volume) = &loaded.images else { panic!("expected a volume") };
    assert_eq!(volume.geometry.dims[2], 24);

    // Files were written shuffled; display order must follow instance numbers
    // (1 = most superior) and be spatially monotonic.
    let numbers: Vec<_> = loaded.frames.iter().map(|f| f.instance_number.unwrap_or(0)).collect();
    assert!(numbers.windows(2).all(|w| w[0] < w[1]), "{numbers:?}");
    let z: Vec<f64> = loaded.frames.iter().map(|f| f.plane.map_or(0.0, |p| p.position.z)).collect();
    assert!(z.windows(2).all(|w| w[0] > w[1]), "superior → inferior: {z:?}");

    // Air, soft tissue and the marker have the expected HU.
    let (lo, hi) = loaded.value_range;
    assert!(lo <= -990.0 && hi >= 690.0, "range {lo}..{hi}");
    let marker = volume.value_at_patient(DVec3::from_array(MARKER_POSITION)).expect("inside volume");
    assert!((f64::from(marker) - CT_MARKER_HU).abs() < 1.0, "marker HU {marker}");
    let tissue = volume.value_at_patient(DVec3::new(-70.0, 30.0, 0.0)).expect("inside volume");
    assert!((f64::from(tissue) - CT_SOFT_TISSUE_HU).abs() <= 7.0, "soft tissue HU {tissue}");

    // Stored DICOM windows are offered as presets, the first is the default.
    let names: Vec<_> = loaded.presets.iter().map(|p| p.name.to_string()).collect();
    assert_eq!(names, vec!["Mediastinum", "Lung"]);
    assert_eq!(loaded.initial_window.center, 40.0);
}

/// The CT phantom in memory holds exactly the voxels of the written series,
/// at the same positions; the vessel and the bone cylinder have their values.
#[test]
fn in_memory_ct_phantom_equals_the_written_series() {
    let options = PhantomOptions::small();
    let (dir, _) = fixture(options.clone());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let loaded = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode CT");
    let volume = loaded.volume().expect("volume");
    let phantom = CtPhantom::new(&options);
    let geometry = &volume.geometry;
    assert_eq!(geometry.dims, phantom.dims());
    let spacing = DVec3::from_array(phantom.spacing());
    assert!((geometry.spacing() - spacing).length() < 1e-6, "{:?}", geometry.spacing());
    let origin = DVec3::from_array(phantom.origin());
    for (k, slice) in volume.slices.iter().enumerate() {
        // The written series may be stored head first: find the phantom slice by position.
        let corner = geometry.to_patient(DVec3::new(0.0, 0.0, k as f64));
        assert!((corner.x - origin.x).abs() < 1e-6 && (corner.y - origin.y).abs() < 1e-6);
        let index = ((corner.z - origin.z) / spacing.z).round();
        assert!((corner.z - origin.z - index * spacing.z).abs() < 1e-6, "slice {k} between phantom slices");
        let expected = phantom.slice(index as usize);
        assert_eq!(slice.len(), expected.len());
        assert!((0..expected.len()).all(|i| slice.get(i) == f32::from(expected[i])), "slice {k}");
    }
    let at = |[x, y]: [f64; 2]| volume.value_at_patient(DVec3::new(x, y, 0.0)).expect("inside the volume");
    assert_eq!(f64::from(at(CT_VESSEL_AXIS_MM)), CT_VESSEL_HU);
    assert_eq!(f64::from(at(CT_BONE_AXIS_MM)), CT_BONE_HU);
}

#[test]
fn mpr_of_ct_places_marker_left_anterior_superior() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let loaded = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode CT");
    let volume = loaded.volume().expect("volume");
    let marker = DVec3::from_array(MARKER_POSITION);
    for axis in MprAxis::ALL {
        let img = reslice(volume, axis, marker.dot(axis.normal()), None, 512);
        let (x, y) = img.patient_to_pixel(marker);
        let v = img.value_at(x.round() as usize, y.round() as usize).expect("marker inside view");
        assert!((f64::from(v) - CT_MARKER_HU).abs() < 30.0, "{axis:?}: {v}");
        let right_half = x > img.width as f64 / 2.0;
        assert_eq!(
            right_half,
            axis != MprAxis::Sagittal,
            "{axis:?}: patient left is screen right; anterior is screen left on sagittal"
        );
        assert!(y < img.height as f64 / 2.0, "{axis:?}: anterior/superior is up");
    }
}

#[test]
fn sagittal_mr_volume_and_presets() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let mr = load_series(&index.media, &index.studies[0].series[1], &Progress::new()).expect("decode MR");
    assert!(mr.units.is_none());
    assert!(mr.volume().is_some(), "{:?}", mr.volume_issue);
    assert_eq!(mr.presets.len(), 2);
    assert_eq!(mr.presets[1].name, "Window 2", "unnamed windows get a generic label");
}

#[test]
fn jpeg_localizer_decodes_as_frames_without_volume() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let loc = load_series(&index.media, &index.studies[0].series[2], &Progress::new()).expect("decode JPEG");
    assert_eq!(loc.len(), 3);
    assert_eq!(loc.volume_issue, Some(VolumeError::InconsistentGrid));
    let frame = loc.frame(0).expect("frame");
    assert_eq!(frame.size(), (64, 64));
}

#[test]
fn rgb_secondary_capture_decodes() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let sc = load_series(&index.media, &index.studies[0].series[3], &Progress::new()).expect("decode RGB");
    assert!(sc.is_color);
    assert_eq!(sc.volume_issue, Some(VolumeError::NotGrayscale));
    let thumb = thumbnail(&index.media, &index.studies[0].series[3], 64).expect("thumbnail");
    assert_eq!((thumb.width, thumb.height), (64, 40));
}

#[test]
fn metadata_tree_and_summary() {
    let (_dir, written) = fixture(PhantomOptions::small());
    let path = &written.series("MR").expect("mr").files[0];
    let md = read_metadata(&Media::folder(), path).expect("metadata");
    let tr = md.dataset.iter().find(|n| n.keyword.as_deref() == Some("RepetitionTime")).expect("TR tag");
    assert_eq!(tr.name, "Repetition Time");
    assert_eq!(tr.tag_label(), "(0018,0080)");
    assert!(md.dataset.iter().any(|n| n.matches("tse2d1")));
    assert!(md.attribute_count() > 40);

    let section = |title: &str| md.summary.iter().find(|s| s.title == title).expect(title);
    let row = |title: &str, label: &str| {
        section(title).rows.iter().find(|row| row.label == label).map(|row| row.value.clone()).expect(label)
    };
    assert_eq!(row("Patient", "Name"), "SYNTHETIC PHANTOM");
    assert_eq!(row("Patient", "Birth date"), "1970-01-01");
    assert_eq!(row("Patient", "Age"), "56 years");
    assert_eq!(row("Series", "Modality"), "Magnetic Resonance (MR)");
    assert_eq!(row("Acquisition", "Repetition time (TR)"), "4000 ms");
    assert_eq!(row("Acquisition", "Scanning sequence"), "Spin echo");
    assert!(row("Image", "Orientation").starts_with("Sagittal"));
    assert_eq!(row("Image", "Transfer syntax"), "Explicit VR Little Endian");
    assert_eq!(row("Equipment", "Field strength"), "1.5 T");
}

#[test]
fn cancellation_stops_scanning() {
    let (dir, _) = fixture(PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() });
    let progress = Progress::new();
    progress.cancel();
    assert!(scan(dir.path(), &progress).is_err());
}

#[test]
fn empty_folder_reports_nothing_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("notes.txt"), "hello").expect("write");
    let err = scan(dir.path(), &Progress::new()).expect_err("nothing to find");
    assert!(err.to_string().contains("no DICOM images"), "{err}");
    let _ = format::date("20260101");
    let _ = Path::new("");
}

#[test]
fn index_keeps_acquisition_parameters_of_each_series() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let study = &index.studies[0];

    let ct = &study.series[0].acquisition;
    assert_eq!(ct.kvp, Some(120.0));
    assert_eq!(ct.convolution_kernel.as_deref(), Some("STANDARD"));
    assert_eq!(ct.contrast_agent.as_deref(), Some("Iodinated contrast (synthetic)"));
    assert!(ct.slice_thickness_mm.is_some_and(|t| (t - 200.0 / 24.0).abs() < 1e-3), "{:?}", ct.slice_thickness_mm);
    assert_eq!(ct.image_type, ["ORIGINAL", "PRIMARY", "AXIAL"]);
    assert_eq!(ct.body_part.as_deref(), Some("CHEST"));
    assert!(ct.scanning_sequence.is_empty() && ct.repetition_time_ms.is_none(), "no MR values on a CT");

    let mr = &study.series[1].acquisition;
    assert_eq!((mr.repetition_time_ms, mr.echo_time_ms, mr.flip_angle_deg), (Some(4000.0), Some(96.0), Some(150.0)));
    assert_eq!((mr.inversion_time_ms, mr.diffusion_b_value), (None, None));
    assert_eq!(mr.scanning_sequence, ["SE"]);
    assert_eq!(mr.sequence_variant, ["SK", "SP"]);
    assert_eq!(mr.sequence_name.as_deref(), Some("*tse2d1_15"));
    assert_eq!(mr.protocol_name.as_deref(), Some("t2_tse_sag"));
    assert_eq!(mr.mr_acquisition_type.as_deref(), Some("2D"));
    assert_eq!(mr.field_strength_t, Some(1.5));
    assert_eq!(mr.slice_thickness_mm, Some(4.0));
    assert_eq!((mr.kvp, mr.convolution_kernel.as_deref(), mr.contrast_agent.as_deref()), (None, None, None));

    let localizer = &study.series[2].acquisition;
    assert_eq!(localizer.image_type, ["ORIGINAL", "PRIMARY", "LOCALIZER"]);
    assert_eq!(localizer.body_part, None);
    // The stored picture records nothing about an acquisition.
    assert_eq!(study.series[3].acquisition, Acquisition::default());
}

#[test]
fn summary_rows_carry_stable_keys() {
    let (_dir, written) = fixture(PhantomOptions::small());
    let key = |modality: &str, title: &str, label: &str| {
        let md =
            read_metadata(&Media::folder(), &written.series(modality).expect(modality).files[0]).expect("metadata");
        let section = md.summary.iter().find(|s| s.title == title).expect(title);
        section.rows.iter().find(|row| row.label == label).expect(label).key
    };
    // Rows that show one attribute carry its keyword, whatever their label says.
    assert_eq!(key("MR", "Acquisition", "Repetition time (TR)"), Some("RepetitionTime"));
    assert_eq!(key("MR", "Acquisition", "Scanning sequence"), Some("ScanningSequence"));
    assert_eq!(key("MR", "Acquisition", "Acquisition type"), Some("MRAcquisitionType"));
    assert_eq!(key("MR", "Equipment", "Field strength"), Some("MagneticFieldStrength"));
    assert_eq!(key("MR", "Series", "Modality"), Some("Modality"));
    assert_eq!(key("MR", "Series", "Protocol"), Some("ProtocolName"));
    assert_eq!(key("CT", "Acquisition", "Tube voltage"), Some("KVP"));
    assert_eq!(key("CT", "Acquisition", "Kernel"), Some("ConvolutionKernel"));
    assert_eq!(key("CT", "Acquisition", "Contrast agent"), Some("ContrastBolusAgent"));
    assert_eq!(key("CT", "Patient", "Name"), Some("PatientName"));
    // Computed rows name their main attribute …
    assert_eq!(key("CT", "Image", "Orientation"), Some("ImageOrientationPatient"));
    assert_eq!(key("CT", "Image", "Pixel spacing"), Some("PixelSpacing"));
    assert_eq!(key("CT", "Image", "Rescale"), Some("RescaleSlope"));
    assert_eq!(key("CT", "Image", "Stored windows"), Some("WindowCenter"));
    assert_eq!(key("CT", "Image", "Transfer syntax"), Some("TransferSyntaxUID"));
    // … the two that combine attributes have keys of their own …
    assert_eq!(key("CT", "Image", "Matrix"), Some("Matrix"));
    assert_eq!(key("CT", "Image", "Field of view"), Some("FieldOfView"));
    // … and rows without a main attribute have none.
    assert_eq!(key("CT", "Study", "Date"), None);
}
