//! End-to-end import tests against the synthetic CD written by
//! `summrise-testkit` (no real patient data involved).

use std::path::Path;

use summrise_dicom::{
    DiscoverySource, Progress, SeriesImages, discover, format, load_series, read_metadata, scan, thumbnail,
};
use summrise_imaging::{AnatomicalPlane, DVec3, MprAxis, VolumeError, reslice};
use summrise_testkit::{
    CT_MARKER_HU, CT_SOFT_TISSUE_HU, INSTITUTION, MARKER_POSITION, PATIENT_NAME, PhantomOptions, WrittenStudy,
    write_phantom_study,
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
    let loaded = load_series(ct, &Progress::new()).expect("decode CT");
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

#[test]
fn mpr_of_ct_places_marker_left_anterior_superior() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let loaded = load_series(&index.studies[0].series[0], &Progress::new()).expect("decode CT");
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
    let mr = load_series(&index.studies[0].series[1], &Progress::new()).expect("decode MR");
    assert!(mr.units.is_none());
    assert!(mr.volume().is_some(), "{:?}", mr.volume_issue);
    assert_eq!(mr.presets.len(), 2);
    assert_eq!(mr.presets[1].name, "Window 2", "unnamed windows get a generic label");
}

#[test]
fn jpeg_localizer_decodes_as_frames_without_volume() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let loc = load_series(&index.studies[0].series[2], &Progress::new()).expect("decode JPEG");
    assert_eq!(loc.len(), 3);
    assert_eq!(loc.volume_issue, Some(VolumeError::InconsistentGrid));
    let frame = loc.frame(0).expect("frame");
    assert_eq!(frame.size(), (64, 64));
}

#[test]
fn rgb_secondary_capture_decodes() {
    let (dir, _) = fixture(PhantomOptions::small());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let sc = load_series(&index.studies[0].series[3], &Progress::new()).expect("decode RGB");
    assert!(sc.is_color);
    assert_eq!(sc.volume_issue, Some(VolumeError::NotGrayscale));
    let thumb = thumbnail(&index.studies[0].series[3], 64).expect("thumbnail");
    assert_eq!((thumb.width, thumb.height), (64, 40));
}

#[test]
fn metadata_tree_and_summary() {
    let (_dir, written) = fixture(PhantomOptions::small());
    let path = &written.series("MR").expect("mr").files[0];
    let md = read_metadata(path).expect("metadata");
    let tr = md.dataset.iter().find(|n| n.keyword.as_deref() == Some("RepetitionTime")).expect("TR tag");
    assert_eq!(tr.name, "Repetition Time");
    assert_eq!(tr.tag_label(), "(0018,0080)");
    assert!(md.dataset.iter().any(|n| n.matches("tse2d1")));
    assert!(md.attribute_count() > 40);

    let section = |title: &str| md.summary.iter().find(|s| s.title == title).expect(title);
    let row = |title: &str, label: &str| {
        section(title).rows.iter().find(|(l, _)| l == label).map(|(_, v)| v.clone()).expect(label)
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
