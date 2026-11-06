//! Decoding of images that are standard but unusual (display-fidelity):
//! palette colour, contrast tables, a Modality LUT, masking to Bits Stored,
//! pixel padding, overlay planes and the facts read with the headers.

use summrise_dicom::{
    DecodedInstance, FrameRef, Progress, SeriesImages, StudyIndex, decode_instance, load_series, plan_series,
    read_instance, read_metadata, scan,
};
use summrise_imaging::window::subsample;
use summrise_imaging::{FramePixels, MprAxis, Padding, auto_window, reslice};
use summrise_testkit::fidelity::{self as fx, write_fidelity_study};
use summrise_testkit::{PhantomOptions, write_phantom_study};

fn study() -> (tempfile::TempDir, StudyIndex) {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fidelity_study(dir.path()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    assert_eq!(index.studies.len(), 1);
    assert_eq!(index.studies[0].series.len(), fx::SERIES);
    (dir, index)
}

/// Decodes file `file` of series `series`.
fn decode(index: &StudyIndex, series: usize, file: usize) -> DecodedInstance {
    let header = &index.studies[0].series[series].instances[file];
    decode_instance(read_instance(&index.media, header).expect("read")).expect("decode")
}

fn grey(decoded: &DecodedInstance, frame: usize) -> &summrise_imaging::VoxelBuffer {
    match &decoded.frames[frame].pixels {
        FramePixels::Gray(b) => b,
        FramePixels::Rgb(_) => panic!("a grey-scale frame"),
    }
}

fn rgb_at(decoded: &DecodedInstance, x: u32, y: u32) -> [u8; 3] {
    let frame = &decoded.frames[0];
    let FramePixels::Rgb(rgb) = &frame.pixels else { panic!("a colour frame") };
    let i = ((y * frame.width + x) * 3) as usize;
    [rgb[i], rgb[i + 1], rgb[i + 2]]
}

/// Task 3.3 — palette index 3 is shown as the RGB of entry 3, for 8-bit
/// tables packed two per word and for 16-bit tables; the series is colour.
#[test]
fn palette_images_show_the_colours_of_their_tables() {
    let (_dir, index) = study();
    let (w, h) = fx::PALETTE_SIZE;
    for series in [fx::PALETTE_8, fx::PALETTE_16] {
        let plan = plan_series(&index.studies[0].series[series]);
        assert!(plan.is_color && !plan.monochrome1, "palette colour is colour before decoding");
        assert_eq!(plan.value_range, (0.0, 255.0));
        let decoded = decode(&index, series, 0);
        assert_eq!((decoded.frames[0].width, decoded.frames[0].height), (w, h));
        let mut threes = 0;
        for y in 0..h {
            for x in 0..w {
                let index = (x + y) % 8;
                if series == fx::PALETTE_16 && (x, y) == (0, 0) {
                    // Beyond the 4096 entries: the last entry.
                    assert_eq!(rgb_at(&decoded, x, y), fx::palette_colour(4095));
                    continue;
                }
                assert_eq!(rgb_at(&decoded, x, y), fx::palette_colour(index), "({x}, {y}) index {index}");
                threes += usize::from(index == 3);
            }
        }
        assert!(threes > 50);
        assert_eq!(rgb_at(&decoded, 3, 0), fx::PALETTE_RGB_3);
        assert_eq!(fx::PALETTE_RGB_3, [200, 30, 60]);
    }
}

#[test]
fn segmented_palettes_are_reported_as_not_supported() {
    let (_dir, index) = study();
    let header = &index.studies[0].series[fx::SEGMENTED_PALETTE].instances[0];
    let error = decode_instance(read_instance(&index.media, header).expect("read")).expect_err("not supported");
    assert_eq!(error.to_string(), "Segmented palette colour images are not supported yet");
}

/// Task 3.4 — both tables of the VOI LUT Sequence are read under their
/// explanations; the image stores no window; values stay stored values.
#[test]
fn contrast_tables_are_read_under_their_explanations() {
    let (_dir, index) = study();
    let decoded = decode(&index, fx::VOI_LUT, 0);
    assert!(decoded.presets.is_empty(), "no window stored");
    let names: Vec<&str> = decoded.extras.voi_luts.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Normal", "Harder"]);
    let normal = &decoded.extras.voi_luts[0].table;
    assert_eq!((normal.len(), normal.first(), normal.bits()), (4096, 0, 12));
    // Stored 100 (column 36, row 1) maps to entry 640 of 4095: grey 40 of 255.
    let n = fx::VOI_LUT_SIZE as usize;
    assert_eq!(grey(&decoded, 0).get(n + 36), 100.0, "the value is the stored value");
    assert_eq!(normal.entry(100), fx::normal_entry(100));
    assert_eq!(normal.entry(100), 640);
    assert_eq!((normal.fraction(100.0) * 255.0).round(), 40.0);
    assert_eq!(decoded.extras.voi_luts[1].table.entry(4095), fx::harder_entry(4095));
}

/// The Modality LUT Sequence replaces rescaling: stored 100 is 625 OD.
#[test]
fn modality_table_replaces_rescaling() {
    let (_dir, index) = study();
    let series = &index.studies[0].series[fx::MODALITY_LUT];
    let header = &series.instances[0];
    assert_eq!(header.rescale_type.as_deref(), Some("OD"));
    assert_eq!(header.fidelity.modality_lut_range, Some((0.0, f64::from(fx::modality_entry(1023)))));
    let plan = plan_series(series);
    assert_eq!(plan.units.as_deref(), Some("OD"));
    assert_eq!(plan.value_range, (0.0, f32::from(fx::modality_entry(1023))), "the table's output range");
    let decoded = decode(&index, fx::MODALITY_LUT, 0);
    let n = fx::MODALITY_LUT_SIZE as usize;
    let values = grey(&decoded, 0);
    assert_eq!(values.get(3 * n + 4), 625.0, "stored 100 at column 4, row 3");
    for i in 0..n * n {
        assert_eq!(values.get(i), f32::from(fx::modality_entry(i as u32)), "pixel {i}");
    }
    assert_eq!(decoded.rescale_type.as_deref(), Some("OD"));
}

/// Masking to Bits Stored: an overlay in bit 12 and garbage in bits 13–15
/// never change a value; the overlay is read from bit 12.
#[test]
fn bits_outside_bits_stored_never_change_a_value() {
    let (_dir, index) = study();
    let decoded = decode(&index, fx::EMBEDDED_OVERLAY, 0);
    let n = fx::EMBEDDED_SIZE;
    let values = grey(&decoded, 0);
    for y in 0..n {
        for x in 0..n {
            let expected = f32::from(fx::embedded_stored(x, y)) + fx::EMBEDDED_INTERCEPT as f32;
            assert_eq!(values.get((y * n + x) as usize), expected, "({x}, {y})");
        }
    }
    // The pixel that holds 0xF52C: stored 1324, 300 HU.
    let (sx, sy) = fx::EMBEDDED_SPECIAL_PIXEL;
    assert_eq!(fx::EMBEDDED_SPECIAL_RAW, 0xF52C);
    assert_eq!(values.get((sy * n + sx) as usize), 300.0);
    // The overlay of the retired form: the cross, one plane over the whole image.
    let graphics = &decoded.extras.frame(0).expect("extras").graphics;
    assert_eq!(graphics.len(), 1);
    let plane = &graphics[0];
    assert_eq!((plane.width(), plane.height(), plane.origin()), (n, n, (0, 0)));
    for y in 0..n {
        for x in 0..n {
            assert_eq!(plane.covers(i64::from(x), i64::from(y)), fx::embedded_overlay(x, y), "({x}, {y})");
        }
    }
}

/// Task 3.5 — a (6000,3000) rectangle at Overlay Origin row 11, column 21
/// lies over image column 20, row 10 (value 500); the values stay as stored.
#[test]
fn overlay_data_lies_at_its_origin_and_values_stay() {
    let (_dir, index) = study();
    let decoded = decode(&index, fx::OVERLAY, 0);
    let values = grey(&decoded, 0);
    let (w, h) = fx::OVERLAY_IMAGE_SIZE;
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        assert_eq!(values.get((y * w + x) as usize), f32::from(fx::overlay_image_value(x, y)), "({x}, {y})");
    }
    assert_eq!(values.get((10 * w + 20) as usize), 500.0, "under the first set bit of the plane");
    let graphics = &decoded.extras.frame(0).expect("extras").graphics;
    assert_eq!(graphics.len(), 1);
    let plane = &graphics[0];
    assert_eq!(fx::OVERLAY_ORIGIN, [11, 21]);
    assert_eq!((plane.width(), plane.height(), plane.origin()), (16, 8, (20, 10)));
    assert!(plane.covers(20, 10) && plane.covers(35, 17) && !plane.covers(19, 10) && !plane.covers(25, 13));
    assert_eq!(plane.count(), 2 * 16 + 2 * 6);
    assert_eq!(decoded.extras.padding(0), None);
}

#[test]
fn multi_frame_overlays_follow_their_frame_origin() {
    let (_dir, index) = study();
    let decoded = decode(&index, fx::MULTI_FRAME_OVERLAY, 0);
    assert_eq!(decoded.frames.len(), fx::MULTI_FRAMES as usize);
    let planes = |frame: usize| decoded.extras.frame(frame).map_or(0, |f| f.graphics.len());
    assert_eq!([planes(0), planes(1), planes(2)], [0, 1, 1], "image frames 2 and 3 only");
    let row = &decoded.extras.frame(1).expect("frame 2").graphics[0];
    let column = &decoded.extras.frame(2).expect("frame 3").graphics[0];
    let (r, c) = (i64::from(fx::MULTI_OVERLAY_ROW), i64::from(fx::MULTI_OVERLAY_COLUMN));
    assert!(row.covers(0, r) && row.covers(31, r) && !row.covers(c, 0));
    assert!(column.covers(c, 0) && column.covers(c, 31) && !column.covers(0, r));
    // The values of every frame are untouched.
    for (f, value) in [50.0, 100.0, 150.0].into_iter().enumerate() {
        let values = grey(&decoded, f);
        assert!((0..values.len()).all(|i| values.get(i) == value), "frame {}", f + 1);
    }
}

/// Task 3.8 — padding (−2000 stored, −3024 HU) is recorded per slice, left
/// out of the value range, the automatic window and the reformats.
#[test]
fn padding_is_left_out_of_statistics_and_reformats() {
    let (_dir, index) = study();
    let n = fx::PADDED_CT_SIZE;
    for (k, &z) in fx::PADDED_CT_HEIGHTS.iter().enumerate() {
        let decoded = decode(&index, fx::PADDED_CT, k);
        let padding = decoded.extras.padding(0);
        assert_eq!(padding, Some(Padding::new(-3024.0, -3024.0)), "slice at S {z} mm");
        let values = grey(&decoded, 0);
        assert!(matches!(values, summrise_imaging::VoxelBuffer::I16(_)), "no second copy, no wider type");
        let inside: Vec<f32> =
            (0..n * n).filter(|&i| !fx::padded_ct_is_padding(i % n, i / n)).map(|i| values.get(i as usize)).collect();
        // The automatic window equals that of the pixels inside the circle.
        let expected = auto_window(inside.iter().copied()).expect("window");
        assert_eq!(decoded.initial_window(0, (-1024.0, 3071.0)), expected, "slice at S {z} mm");
        let with_padding = auto_window(subsample(values.len(), 65_536, |i| values.get(i))).expect("window");
        assert_ne!(with_padding, expected, "the padding would change the window");
        // The values themselves: HU inside, the padding value outside.
        for i in [0, n * n - 1] {
            assert!(fx::padded_ct_is_padding(i % n, i / n));
            assert_eq!(values.get(i as usize), -3024.0);
        }
        let centre = (n / 2) * n + n / 2;
        assert_eq!(values.get(centre as usize) as f64, fx::padded_ct_hu(n / 2, n / 2, z));
    }
    // The whole series: value range −1000 … 700 HU, and reformats show no padding.
    let ct = &index.studies[0].series[fx::PADDED_CT];
    let loaded = load_series(&index.media, ct, &Progress::new()).expect("load");
    assert_eq!(loaded.value_range, (-1000.0, 700.0));
    let SeriesImages::Volume(volume) = &loaded.images else { panic!("a volume of 4 slices") };
    assert_eq!(volume.padding(0), Some(Padding::new(-3024.0, -3024.0)));
    let coronal = reslice(volume, MprAxis::Coronal, 0.0, None, 256);
    assert!(coronal.values.iter().any(|v| v.is_finite()));
    assert!(coronal.values.iter().all(|v| !v.is_finite() || (-1000.0..=700.0).contains(v)), "no −3024 HU");
    let corner = summrise_imaging::DVec3::new(-118.0, -118.0, -2.0);
    assert_eq!(volume.value_at_patient(corner), None, "padding reads as outside");
}

/// Task 3.6 — the facts read with the headers.
#[test]
fn lossy_burned_in_and_inverse_facts_are_read_with_the_headers() {
    let (_dir, index) = study();
    let header = |series: usize| &index.studies[0].series[series].instances[0];
    let lossy = header(fx::LOSSY).fidelity.lossy.as_ref().expect("lossy");
    assert_eq!(lossy.label(), "Lossy 10:1 (JPEG)");
    assert!(header(fx::BURNED_IN).fidelity.burned_in_text);
    assert!(header(fx::INVERSE).fidelity.inverse);
    for plain in [fx::PALETTE_8, fx::VOI_LUT, fx::PADDED_CT, fx::OVERLAY] {
        let f = &header(plain).fidelity;
        assert!(f.lossy.is_none() && !f.burned_in_text && !f.inverse, "series {plain}");
    }
    // INVERSE opens inverted, like MONOCHROME1, and the decoder agrees.
    let inverse = plan_series(&index.studies[0].series[fx::INVERSE]);
    assert!(inverse.monochrome1 && !inverse.is_color);
    assert!(decode(&index, fx::INVERSE, 0).monochrome1);
    assert!(!plan_series(&index.studies[0].series[fx::LOSSY]).monochrome1);

    // The inspector says the same, also when only the transfer syntax tells.
    let row = |metadata: &summrise_dicom::InstanceMetadata, label: &str| {
        metadata.summary.iter().flat_map(|s| &s.rows).find(|r| r.label == label).map(|r| r.value.clone())
    };
    let metadata = read_metadata(&index.media, &header(fx::LOSSY).path).expect("metadata");
    assert_eq!(row(&metadata, "Lossy compression").as_deref(), Some("Lossy 10:1 (JPEG)"));
    let metadata = read_metadata(&index.media, &header(fx::BURNED_IN).path).expect("metadata");
    assert_eq!(row(&metadata, "Burned-in text").as_deref(), Some("Yes"));
    assert_eq!(row(&metadata, "Lossy compression"), None);

    let phantom = tempfile::tempdir().expect("tempdir");
    write_phantom_study(phantom.path(), &PhantomOptions::small()).expect("fixture");
    let cd = scan(phantom.path(), &Progress::new()).expect("scan");
    // The JPEG Baseline localizer: the encoder that wrote it recorded its ratio as well.
    let localizer = &cd.studies[0].series[2].instances[0];
    let label = localizer.fidelity.lossy.as_ref().map(|l| l.label()).expect("lossy");
    assert!(label.starts_with("Lossy ") && label.ends_with(":1 (JPEG)"), "{label}");
    let metadata = read_metadata(&cd.media, &localizer.path).expect("metadata");
    assert_eq!(row(&metadata, "Lossy compression"), Some(label));
    assert!(cd.studies[0].series[0].instances[0].fidelity.lossy.is_none(), "the uncompressed CT");
}

/// The plan from the headers agrees with decoding for every series of the
/// study (as for the synthetic CD): order, colour, inversion, units.
#[test]
fn plan_and_decoding_agree_for_every_case() {
    let (_dir, index) = study();
    for (number, series) in index.studies[0].series.iter().enumerate() {
        if number == fx::SEGMENTED_PALETTE {
            continue;
        }
        let plan = plan_series(series);
        let loaded = load_series(&index.media, series, &Progress::new()).expect("load");
        let title = series.title();
        assert_eq!(plan.frames, loaded.frames, "{title}");
        assert_eq!(plan.is_color, loaded.is_color, "{title}");
        assert_eq!(plan.monochrome1, loaded.monochrome1, "{title}");
        assert_eq!(plan.units, loaded.units, "{title}");
        let (lo, hi) = loaded.value_range;
        assert!(plan.value_range.0 <= lo && hi <= plan.value_range.1, "{title}: {:?}", plan.value_range);
        if let Some(FrameRef::Gray { data, .. }) = loaded.frame(0) {
            assert!(!data.is_empty());
        }
    }
}
