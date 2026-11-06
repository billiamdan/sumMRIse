//! PET display: the colour map of PET and nuclear-medicine series, the
//! uptake presets and how uptake values are written, the overlay of a PET
//! over a CT or MR series (fusion) in the view state, which series can be
//! fused, and the inspector's section on uptake values.
//!
//! Uptake values themselves (SUVbw and why it is sometimes refused) come from
//! `summrise_dicom::pet`; the series store applies the factor, so everything
//! here already sees SUV. Fusion is positional only: a PET is offered over a
//! CT or MR of the same study that records the same Frame of Reference UID,
//! and nothing is registered (`openspec/changes/pet-support/design.md`).

use std::borrow::Cow;

use summrise_dicom::format::{self, number};
use summrise_dicom::{PetHeader, SUV_UNIT, Series, SummaryRow, SummarySection, SuvBasis, SuvRefusal, SuvScale};
use summrise_i18n::tr;
use summrise_imaging::{Colormap, OverlayLook, PresetSource, Window, WindowPreset};

use crate::series::SeriesData;
use crate::session::SeriesKey;
use crate::view::ViewState;

/// Upper limits of the built-in uptake windows (from SUV 0).
pub const SUV_LIMITS: [f64; 4] = [5.0, 7.0, 10.0, 15.0];

/// Whether series of `modality` are offered the PET colour maps: PET and
/// nuclear medicine.
pub fn has_colormaps(modality: &str) -> bool {
    matches!(modality.trim(), "PT" | "NM")
}

/// The colour map a series opens with: the inverted grey scale customary for
/// PET and nuclear medicine (high uptake dark) — the plain one for
/// MONOCHROME1 images, which are inverted already — and grey for the rest.
pub fn default_colormap(series: &SeriesData) -> Colormap {
    if has_colormaps(series.modality()) && !series.is_color() && !series.monochrome1() {
        Colormap::InvertedGrey
    } else {
        Colormap::Grey
    }
}

/// Whether the values of series with this unit are uptake values.
pub fn is_suv(units: Option<&str>) -> bool {
    units == Some(SUV_UNIT)
}

/// Decimals for values of this unit in the W and L fields: two for SUV, none
/// otherwise.
pub fn decimals(units: Option<&str>) -> usize {
    if is_suv(units) { 2 } else { 0 }
}

/// A value with its unit, as the read-out shows it: "SUV 3.1" (the unit
/// first, as is customary), "40 HU", "820".
pub fn value_text(units: Option<&str>, value: f64) -> String {
    match units.map(str::trim).filter(|u| !u.is_empty()) {
        Some(SUV_UNIT) => tr!("SUV {value}", value = number(value, 2)),
        Some(unit) => format!("{} {unit}", number(value, 1)),
        None => number(value, 1),
    }
}

/// The built-in uptake windows "SUV 0–5" … "SUV 0–15" for a series in SUV,
/// none for any other.
pub fn uptake_presets(series: &SeriesData) -> Vec<WindowPreset> {
    if !is_suv(series.units()) {
        return Vec::new();
    }
    SUV_LIMITS
        .iter()
        .map(|&limit| WindowPreset {
            name: Cow::Owned(tr!("SUV 0–{limit}", limit = number(limit, 0))),
            window: Window::from_range(0.0, limit),
            source: PresetSource::Clinical,
        })
        .collect()
}

/// The window a series or a stack opens with and its name, given its stored
/// windows and the window computed for it (`initial`: the first stored
/// window, else an automatic one): the first stored window; else "SUV 0–5"
/// for uptake values, the window PET is customarily read in (the automatic
/// window of one PET image can span little more than the body's own uptake);
/// else the automatic window.
pub(crate) fn opening_window(series: &SeriesData, stored: &[WindowPreset], initial: Window) -> (Window, String) {
    if let Some(first) = stored.first() {
        return (initial, first.name.to_string());
    }
    match uptake_presets(series).into_iter().next() {
        Some(suv) => (suv.window, suv.name.into_owned()),
        None => (initial, "Auto".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Fusion

/// A PET drawn over the images of a view, and how. Part of the view state
/// of the CT or MR series; never saved.
#[derive(Debug, Clone, PartialEq)]
pub struct Fusion {
    /// The series drawn over this one.
    pub series: SeriesKey,
    /// 0 (invisible) to 1 (covers the picture).
    pub opacity: f32,
    pub colormap: Colormap,
    /// The colour scale, in the overlay's units, and the name of the preset
    /// it came from (`None`: set by hand); `None` until the overlay's first
    /// image is decoded (see [`ViewState::settle_fusion`]).
    pub scale: Option<(Window, Option<String>)>,
    /// Overlay values below this are not drawn; `None`: 10 % of the colour
    /// scale above its lower end.
    pub threshold: Option<f64>,
}

impl Fusion {
    pub const DEFAULT_OPACITY: f32 = 0.5;
    pub const DEFAULT_COLORMAP: Colormap = Colormap::HotIron;

    /// `series` drawn with the default opacity and colour map, and the colour
    /// scale of that series once it is known.
    pub fn new(series: SeriesKey) -> Self {
        Self { series, opacity: Self::DEFAULT_OPACITY, colormap: Self::DEFAULT_COLORMAP, scale: None, threshold: None }
    }

    /// `series` drawn with the colour scale its own view shows (when a CT is
    /// put under the PET on screen), and with its colour map if that is a
    /// colour scale (the grey scales would hide the anatomy).
    pub fn from_view(series: SeriesKey, view: &ViewState) -> Self {
        let colormap = match view.colormap() {
            map @ (Colormap::HotIron | Colormap::Rainbow) => map,
            Colormap::Grey | Colormap::InvertedGrey => Self::DEFAULT_COLORMAP,
        };
        Self { scale: Some((view.window, view.preset.clone())), colormap, ..Self::new(series) }
    }

    /// The threshold in use, once the colour scale is known.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold.or_else(|| self.scale.as_ref().map(|(window, _)| window.low() + 0.1 * window.width))
    }

    /// How to draw the overlay, once its colour scale is known.
    pub fn look(&self) -> Option<OverlayLook> {
        let (window, _) = self.scale.as_ref()?;
        Some(OverlayLook {
            window: *window,
            colormap: self.colormap,
            opacity: self.opacity.clamp(0.0, 1.0),
            threshold: self.threshold()?,
        })
    }
}

impl ViewState {
    /// The colour map of the series (grey for every series but PET and
    /// nuclear medicine).
    pub fn colormap(&self) -> Colormap {
        self.colormap
    }

    pub fn set_colormap(&mut self, colormap: Colormap) {
        self.colormap = colormap;
    }

    /// The PET drawn over this series, if any.
    pub fn fusion(&self) -> Option<&Fusion> {
        self.fusion.as_ref()
    }

    pub fn fusion_mut(&mut self) -> Option<&mut Fusion> {
        self.fusion.as_mut()
    }

    pub fn set_fusion(&mut self, fusion: Option<Fusion>) {
        self.fusion = fusion;
    }

    /// Gives the overlay its colour scale as soon as the overlay's first image
    /// is decoded: "SUV 0–5" for uptake values, else the window that series
    /// opens with, named like the window of a series that opens. Call once
    /// per frame.
    pub fn settle_fusion(&mut self, overlay: &SeriesData) {
        let Some(fusion) = self.fusion.as_mut().filter(|f| f.scale.is_none()) else { return };
        if overlay.is_viewable() {
            fusion.scale = Some(match uptake_presets(overlay).into_iter().next() {
                Some(suv) => (suv.window, Some(suv.name.into_owned())),
                None => {
                    let preset = overlay.presets().first().map_or_else(|| "Auto".to_owned(), |p| p.name.to_string());
                    (overlay.initial_window(), Some(preset))
                }
            });
        }
    }
}

/// Whether every image of `series` has a position in the patient.
fn has_positions(series: &Series) -> bool {
    !series.instances.is_empty() && series.instances.iter().all(|i| i.frame_planes.iter().all(Option::is_some))
}

/// Whether `overlay` can be drawn over `base`: a grey PET series over a CT or
/// MR series, both with positions, recording the same non-empty Frame of
/// Reference UID. (Both must belong to the same study; the session checks
/// that.)
pub fn can_fuse(base: &Series, overlay: &Series) -> bool {
    let frame = base.frame_of_reference.as_deref().map(str::trim).filter(|f| !f.is_empty());
    matches!(base.modality.trim(), "CT" | "MR")
        && overlay.modality.trim() == "PT"
        && frame.is_some()
        && frame == overlay.frame_of_reference.as_deref().map(str::trim)
        && overlay.instances.iter().all(|i| i.samples_per_pixel == 1)
        && has_positions(base)
        && has_positions(overlay)
}

/// About how much memory the images of `series` take once decoded: 2 bytes
/// per value of 16-bit integer data, 4 for PET (uptake values are floating
/// point) and for data with fractional rescale values or more bits, 3 per
/// colour pixel.
pub fn estimated_bytes(series: &Series) -> usize {
    let pet = series.modality.trim() == "PT";
    series
        .instances
        .iter()
        .map(|i| {
            let integral = i.rescale.iter().all(|r| r.slope.fract() == 0.0 && r.intercept.fract() == 0.0);
            let bytes = match i.samples_per_pixel {
                3 => 3,
                _ if pet || !integral || i.bits_stored > 16 => 4,
                _ => 2,
            };
            i.rows as usize * i.cols as usize * i.frames as usize * bytes
        })
        .sum()
}

// ---------------------------------------------------------------------------
// The inspector's section

/// The inspector's section on the uptake values of a PET series: what the
/// values are shown as, why SUV is not computed when it is not, and every
/// input with its recorded value. Built from the series' header record and
/// its SUV outcome, so it always describes what is shown. The weight, a
/// patient detail, is hidden with `hide_patient`. `None` for other series.
pub fn uptake_section(series: &Series, data: &SeriesData, hide_patient: bool) -> Option<SummarySection> {
    let header = series.pet.as_ref()?;
    let outcome = data.plan().suv.as_ref()?;
    let not_recorded = || tr!("Not recorded").to_owned();
    let mut rows = Vec::new();
    let mut row = |label: &str, key: Option<&'static str>, value: String| {
        rows.push(SummaryRow { label: label.to_owned(), value, key });
    };

    let shown = match outcome {
        Ok(SuvScale { basis: SuvBasis::Computed { .. }, .. }) => {
            tr!("SUV (standardised uptake value by body weight), computed by SumMRIse; approximate").to_owned()
        }
        Ok(SuvScale { basis: SuvBasis::Stored, .. }) => {
            tr!("SUV, as stored in the images by the scanner or a workstation (g/ml)").to_owned()
        }
        Err(_) => tr!("{unit} as stored; SUV not computed", unit = data.units().unwrap_or("")).trim().to_owned(),
    };
    row(tr!("Values shown"), Some("SUVbw"), shown);
    if let Err(refusal) = outcome {
        row(tr!("Not computed because"), None, refusal.reason());
    }

    let weight = if hide_patient {
        tr!("Hidden (privacy mode)").to_owned()
    } else {
        header.patient_weight_kg.map_or_else(not_recorded, |kg| tr!("{weight} kg", weight = number(kg, 1)))
    };
    row(tr!("Patient weight"), Some("PatientWeight"), weight);
    let dose = header.injected_dose_bq.map_or_else(not_recorded, |bq| tr!("{dose} MBq", dose = number(bq / 1e6, 1)));
    row(tr!("Injected dose"), Some("RadionuclideTotalDose"), dose);
    row(
        tr!("Injection time"),
        Some("RadiopharmaceuticalStartTime"),
        injection_text(header).unwrap_or_else(not_recorded),
    );
    let reference = date_time_text(header.series_date.as_deref(), header.series_time.as_deref());
    row(tr!("Reference time"), Some("SeriesTime"), reference.unwrap_or_else(not_recorded));
    let half_life = header.half_life_s.map_or_else(not_recorded, |s| {
        tr!("{seconds} s ({minutes} min)", seconds = number(s, 1), minutes = number(s / 60.0, 1))
    });
    row(tr!("Half-life"), Some("RadionuclideHalfLife"), half_life);
    row(tr!("Decay correction"), Some("DecayCorrection"), header.decay_correction.clone().unwrap_or_else(not_recorded));
    let corrections = (!header.corrected_image.is_empty()).then(|| header.corrected_image.join(", "));
    row(tr!("Corrections"), Some("CorrectedImage"), corrections.unwrap_or_else(not_recorded));
    row(tr!("Stored unit"), Some("Units"), header.units.clone().unwrap_or_else(not_recorded));
    let tracer = match (&header.radiopharmaceutical, &header.radionuclide) {
        (Some(drug), Some(nuclide)) => Some(tr!("{tracer} ({nuclide})", tracer = drug, nuclide = nuclide)),
        (drug, nuclide) => drug.clone().or_else(|| nuclide.clone()),
    };
    row(tr!("Tracer"), Some("Radiopharmaceutical"), tracer.unwrap_or_else(not_recorded));
    if let Ok(SuvScale { basis: SuvBasis::Computed { decayed_dose_bq, decay_s, .. }, .. }) = outcome {
        let after = duration_text(*decay_s);
        let left =
            tr!("{dose} MBq, {after} after the injection", dose = number(decayed_dose_bq / 1e6, 1), after = after);
        row(tr!("Dose at the reference time"), None, left);
    }
    Some(SummarySection { key: "Uptake", title: tr!("Uptake values"), rows })
}

/// Why SUV is not computed for `data`, if it is a PET series for which it is
/// not.
pub fn refusal(data: &SeriesData) -> Option<&SuvRefusal> {
    data.plan().suv.as_ref()?.as_ref().err()
}

/// "2026-01-15 08:25:13" from a DA and a TM value (either may be missing).
fn date_time_text(date: Option<&str>, time: Option<&str>) -> Option<String> {
    let date = date.and_then(format::date);
    let time = time.and_then(format::time);
    match (date, time) {
        (Some(date), Some(time)) => Some(tr!("{date} {time}", date = date, time = time)),
        (d, t) => d.or(t),
    }
}

/// The injection time as recorded: the DateTime, else the Start Time.
fn injection_text(header: &PetHeader) -> Option<String> {
    let from_datetime = header.injection_datetime.as_deref().and_then(|dt| {
        let (date, time) = (dt.get(..8)?, dt.get(8..)?);
        let time: String = time.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        date_time_text(Some(date), Some(&time))
    });
    from_datetime.or_else(|| header.injection_time.as_deref().and_then(format::time))
}

/// "1 h 49 min", "25 min" or "40 s" for a duration in seconds.
fn duration_text(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    if minutes < 1.0 {
        return tr!("{seconds} s", seconds = number(seconds, 0));
    }
    let (hours, minutes) = ((minutes / 60.0).floor(), minutes % 60.0);
    if hours < 1.0 {
        tr!("{minutes} min", minutes = number(minutes, 0))
    } else {
        tr!("{hours} h {minutes} min", hours = number(hours, 0), minutes = number(minutes, 0))
    }
}

#[cfg(test)]
mod tests {
    use summrise_dicom::{Progress, StudyIndex, scan};
    use summrise_imaging::Volume;
    use summrise_testkit::pet::{self as phantom, PetStudyOptions, write_pet_study};

    use super::*;
    use crate::series::testing::load;
    use crate::view::ViewKind;

    pub(crate) fn pet_study(options: &PetStudyOptions) -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_pet_study(dir.path(), options).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    #[test]
    fn a_pet_series_opens_inverted_in_suv_with_the_uptake_presets() {
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let study = &index.studies[0];
        let pet = load(&study.series[phantom::PET]);
        assert_eq!(pet.units(), Some("SUV"));
        let state = ViewState::new(&pet);
        assert_eq!(state.colormap(), Colormap::InvertedGrey, "high uptake dark");
        assert!(state.fusion().is_none() && state.is_pristine(&pet));
        // No stored window: SUV 0–5, the window PET is read in, not the automatic window.
        assert_eq!((state.window.low(), state.window.high()), (0.0, 5.0));
        assert_eq!(state.preset.as_deref(), Some("SUV 0–5"));
        let names: Vec<String> = ViewState::presets(&pet).iter().map(|p| p.name.to_string()).collect();
        assert_eq!(names, ["SUV 0–5", "SUV 0–7", "SUV 0–10", "SUV 0–15", "Auto", "Full range"]);
        let presets = ViewState::presets(&pet);
        assert_eq!((presets[1].window.low(), presets[1].window.high()), (0.0, 7.0));
        // The hot sphere is SUV 5 in the volume, and the full range reaches it.
        let volume: &Volume = pet.volume().expect("volume");
        let at_marker = summrise_imaging::fusion::sample_at(volume, summrise_testkit::MARKER_POSITION.into());
        assert!(at_marker.is_some_and(|v| (v - 5.0).abs() < 1e-3), "{at_marker:?}");
        let (lo, hi) = pet.value_range();
        assert!(lo == 0.0 && (hi - 5.0).abs() < 1e-3, "{lo} … {hi}");

        // The CT keeps its grey scale and its presets.
        let ct = load(&study.series[phantom::CT]);
        let state = ViewState::new(&ct);
        assert_eq!(state.colormap(), Colormap::Grey);
        assert!(ViewState::presets(&ct).iter().all(|p| !p.name.starts_with("SUV")));

        // The PET without attenuation correction stays in Bq/ml, without uptake presets.
        let nac = load(&study.series[phantom::PET_NAC]);
        assert_eq!(nac.units(), Some("Bq/ml"));
        assert_eq!(refusal(&nac), Some(&SuvRefusal::NotAttenuationCorrected));
        assert!(ViewState::presets(&nac).iter().all(|p| !p.name.starts_with("SUV")));
        let state = ViewState::new(&nac);
        assert_eq!(state.colormap(), Colormap::InvertedGrey);
        // Kept in Bq/ml it opens like any series without a stored window.
        assert_eq!((state.window, state.preset.as_deref()), (nac.initial_window(), Some("Auto")));
    }

    #[test]
    fn colour_map_and_overlay_are_view_state_that_reset_clears() {
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let study = &index.studies[0];
        let pet = load(&study.series[phantom::PET]);
        let ct = load(&study.series[phantom::CT]);
        let mut state = ViewState::new(&pet);
        state.set_colormap(Colormap::HotIron);
        assert!(!state.is_pristine(&pet), "another colour map is the user's doing");
        state.reset(&pet);
        assert_eq!(state.colormap(), Colormap::InvertedGrey);
        assert!(state.is_pristine(&pet));

        let mut state = ViewState::new(&ct);
        let key = SeriesKey { study: 0, series: phantom::PET };
        state.set_fusion(Some(Fusion::new(key)));
        assert!(!state.is_pristine(&ct));
        // The colour scale of a PET in SUV is SUV 0–5; the threshold 10 % into it, SUV 0.5.
        assert_eq!(state.fusion().and_then(Fusion::look), None, "not before the PET's first image");
        state.settle_fusion(&pet);
        let fusion = state.fusion().expect("fusion").clone();
        let (window, preset) = fusion.scale.clone().expect("scale");
        assert_eq!((window.low(), window.high(), preset.as_deref()), (0.0, 5.0, Some("SUV 0–5")));
        let look = fusion.look().expect("look");
        assert_eq!((look.opacity, look.colormap), (0.5, Colormap::HotIron));
        assert!((look.threshold - 0.5).abs() < 1e-12);
        // A PET kept in Bq/ml gets its own opening window.
        let nac = load(&study.series[phantom::PET_NAC]);
        let mut over_nac = ViewState::new(&ct);
        over_nac.set_fusion(Some(Fusion::new(SeriesKey { study: 0, series: phantom::PET_NAC })));
        over_nac.settle_fusion(&nac);
        let scale = over_nac.fusion().and_then(|f| f.scale.clone()).expect("scale");
        assert_eq!(scale, (nac.initial_window(), Some("Auto".to_owned())));
        // A colour scale the user set is kept.
        state.fusion_mut().expect("fusion").scale = Some((Window::from_range(0.0, 7.0), None));
        state.settle_fusion(&pet);
        assert_eq!(state.fusion().and_then(|f| f.scale.clone()).map(|s| s.0.high()), Some(7.0));
        // From the PET's own view: its window.
        let mut pet_view = ViewState::new(&pet);
        pet_view.apply_preset(&ViewState::presets(&pet)[2]);
        let from_view = Fusion::from_view(key, &pet_view);
        assert_eq!(from_view.colormap, Colormap::HotIron, "not the inverted grey scale of the PET's view");
        assert_eq!(from_view.scale.map(|(w, name)| (w.high(), name)), Some((10.0, Some("SUV 0–10".to_owned()))));
        pet_view.set_colormap(Colormap::Rainbow);
        assert_eq!(Fusion::from_view(key, &pet_view).colormap, Colormap::Rainbow);
        // Reset switches the overlay off.
        state.reset(&ct);
        assert!(state.fusion().is_none() && state.is_pristine(&ct));
        assert_eq!(state.slice_index(ViewKind::Stack, &ct), ViewState::new(&ct).slice);
    }

    #[test]
    fn a_pet_is_offered_over_the_ct_of_its_frame_of_reference_only() {
        let (_dir, index) =
            pet_study(&PetStudyOptions { other_frame_of_reference: true, ..PetStudyOptions::default() });
        let series = &index.studies[0].series;
        let (ct, pet, nac, elsewhere) =
            (&series[phantom::CT], &series[phantom::PET], &series[phantom::PET_NAC], &series[phantom::PET_ELSEWHERE]);
        assert!(ct.frame_of_reference.is_some());
        assert_eq!(ct.frame_of_reference, pet.frame_of_reference);
        assert_ne!(elsewhere.frame_of_reference, ct.frame_of_reference);
        assert!(can_fuse(ct, pet) && can_fuse(ct, nac));
        assert!(!can_fuse(ct, elsewhere), "another Frame of Reference is not offered");
        assert!(!can_fuse(pet, ct) && !can_fuse(pet, nac) && !can_fuse(ct, ct), "a PET over a CT or MR only");
        let mut without = ct.clone();
        without.frame_of_reference = None;
        assert!(!can_fuse(&without, pet), "no Frame of Reference, no fusion");
        // Memory estimates: 16-bit CT, floating-point PET.
        assert_eq!(estimated_bytes(ct), 48 * 48 * 24 * 2);
        assert_eq!(estimated_bytes(pet), 60 * 60 * 50 * 4);
    }

    #[test]
    fn the_uptake_section_lists_every_input_and_hides_the_weight_in_privacy_mode() {
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let series = &index.studies[0].series;
        let pet = load(&series[phantom::PET]);
        let section = uptake_section(&series[phantom::PET], &pet, false).expect("a PET series");
        assert_eq!(section.title, "Uptake values");
        let value = |section: &SummarySection, label: &str| {
            section.rows.iter().find(|r| r.label == label).map(|r| r.value.clone())
        };
        assert!(value(&section, "Values shown").is_some_and(|v| v.starts_with("SUV (standardised uptake value")));
        assert_eq!(value(&section, "Not computed because"), None);
        assert_eq!(value(&section, "Patient weight").as_deref(), Some("70 kg"));
        assert_eq!(value(&section, "Injected dose").as_deref(), Some("370 MBq"));
        assert_eq!(value(&section, "Injection time").as_deref(), Some("08:25:13"));
        assert_eq!(value(&section, "Reference time").as_deref(), Some("2026-01-15 10:15:00"));
        assert_eq!(value(&section, "Half-life").as_deref(), Some("6586.2 s (109.8 min)"));
        assert_eq!(value(&section, "Decay correction").as_deref(), Some("START"));
        assert_eq!(value(&section, "Corrections").as_deref(), Some("DECY, ATTN, SCAT, DTIM, RAN, NORM"));
        assert_eq!(value(&section, "Stored unit").as_deref(), Some("BQML"));
        assert_eq!(value(&section, "Tracer").as_deref(), Some("Fluorodeoxyglucose (^18^Fluorine)"));
        assert_eq!(
            value(&section, "Dose at the reference time").as_deref(),
            Some("185 MBq, 1 h 49 min after the injection")
        );
        assert_eq!(section.rows[0].key, Some("SUVbw"));

        // Privacy mode: the weight is hidden, and appears nowhere else in the section.
        let private = uptake_section(&series[phantom::PET], &pet, true).expect("section");
        assert_eq!(value(&private, "Patient weight").as_deref(), Some("Hidden (privacy mode)"));
        assert!(private.rows.iter().all(|r| !r.value.contains("70 kg")));

        // Refused: the reason, in the stored unit.
        let nac = load(&series[phantom::PET_NAC]);
        let section = uptake_section(&series[phantom::PET_NAC], &nac, false).expect("section");
        assert_eq!(value(&section, "Values shown").as_deref(), Some("Bq/ml as stored; SUV not computed"));
        assert_eq!(
            value(&section, "Not computed because").as_deref(),
            Some("The images are not corrected for attenuation (the weakening of the signal inside the body).")
        );
        assert_eq!(value(&section, "Dose at the reference time"), None);
        // Other series have no such section.
        let ct = load(&series[phantom::CT]);
        assert!(uptake_section(&series[phantom::CT], &ct, false).is_none());

        // Values stored as uptake values (Units GML) are shown as they are, and said to be.
        let mut stored = series[phantom::PET].clone();
        if let Some(header) = stored.pet.as_mut() {
            header.units = Some("GML".to_owned());
        }
        let planned = SeriesData::new(summrise_dicom::plan_series(&stored));
        assert_eq!(planned.units(), Some("SUV"));
        let section = uptake_section(&stored, &planned, false).expect("section");
        assert_eq!(
            value(&section, "Values shown").as_deref(),
            Some("SUV, as stored in the images by the scanner or a workstation (g/ml)")
        );
    }

    /// The weight is not recorded: Bq/ml, no uptake windows, and the
    /// inspector says why; the hot sphere reads 13214.3 Bq/ml.
    #[test]
    fn without_the_weight_the_values_stay_in_bq_per_ml() {
        let (_dir, index) = pet_study(&PetStudyOptions { without_weight: true, ..PetStudyOptions::default() });
        let meta = &index.studies[0].series[phantom::PET];
        let pet = load(meta);
        assert_eq!(pet.units(), Some("Bq/ml"));
        assert_eq!(refusal(&pet), Some(&SuvRefusal::WeightMissing));
        assert!(ViewState::presets(&pet).iter().all(|p| !p.name.starts_with("SUV")), "no uptake windows");
        let at_marker = summrise_imaging::fusion::sample_at(
            pet.volume().expect("volume"),
            summrise_testkit::MARKER_POSITION.into(),
        )
        .expect("inside");
        assert_eq!(value_text(pet.units(), f64::from(at_marker)), "13214.3 Bq/ml");
        let section = uptake_section(meta, &pet, false).expect("section");
        let value = |label: &str| section.rows.iter().find(|r| r.label == label).map(|r| r.value.clone());
        assert_eq!(value("Not computed because").as_deref(), Some("The patient's weight is not recorded."));
        assert_eq!(value("Patient weight").as_deref(), Some("Not recorded"));
    }

    /// Task 4.1 — the viewer's progressive loading gives what `load_series`
    /// gives, in SUV, for every series of the PET study.
    #[test]
    fn progressive_loading_equals_load_series_in_suv() {
        use std::time::{Duration, Instant};

        use summrise_dicom::{LoadedSeries, load_series};

        use crate::session::Session;

        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let reference: Vec<LoadedSeries> = index.studies[0]
            .series
            .iter()
            .map(|s| load_series(&index.media, s, &Progress::new()).expect("decode"))
            .collect();
        assert_eq!(
            reference.iter().map(|r| r.units.as_deref()).collect::<Vec<_>>(),
            [Some("HU"), Some("SUV"), Some("Bq/ml")]
        );
        let mut session = Session::new(index, 1 << 30, None);
        for (series, expected) in reference.iter().enumerate() {
            let key = SeriesKey { study: 0, series };
            session.select_series(key);
            let start = Instant::now();
            while session.loaded(key).is_none() {
                session.poll();
                assert!(start.elapsed() < Duration::from_secs(30), "timed out loading series {series}");
                std::thread::sleep(Duration::from_millis(2));
            }
            let data = session.loaded(key).expect("complete");
            assert_eq!(data.frames(), expected.frames.as_slice(), "series {series}");
            for index in 0..expected.len() {
                assert_eq!(data.frame(index), expected.frame(index), "series {series}: image {index}");
            }
            assert_eq!(data.volume().map(|v| &v.slices), expected.volume().map(|v| &v.slices), "series {series}");
            assert_eq!(data.value_range(), expected.value_range, "series {series}");
            assert_eq!(data.units(), expected.units.as_deref(), "series {series}");
            assert_eq!(data.presets(), expected.presets.as_slice(), "series {series}");
            assert_eq!(data.byte_size(), expected.byte_size(), "series {series}");
        }
        // The PET in SUV: up to the hot sphere's 5.0.
        let (lo, hi) = reference[phantom::PET].value_range;
        assert!(lo == 0.0 && (hi - 5.0).abs() < 1e-3, "{lo} … {hi}");
        let (_, hi) = reference[phantom::PET_NAC].value_range;
        assert!((hi - 13_214.3).abs() < 0.1, "Bq/ml as stored: {hi}");
    }

    /// Task 4.6 — positions are true: the PET's maximum lies on the CT marker
    /// (within one PET voxel, 4 mm) on the acquired CT image, on every
    /// reformat through the marker and on a tilted plane.
    #[test]
    fn the_pet_maximum_lies_on_the_ct_marker_in_every_view() {
        use summrise_imaging::fusion::{resample, sample_at};
        use summrise_imaging::{DVec3, MprAxis, PixelGrid, reslice_plane};
        use summrise_testkit::{CT_MARKER_HU, MARKER_POSITION, PhantomOptions};

        let ct_options = PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() };
        let (_dir, index) = pet_study(&PetStudyOptions { ct: ct_options, ..PetStudyOptions::default() });
        let series = &index.studies[0].series;
        let (ct, pet) = (load(&series[phantom::CT]), load(&series[phantom::PET]));
        let volume = pet.volume().expect("the PET forms a volume");
        let marker = DVec3::from_array(MARKER_POSITION);
        assert!(sample_at(volume, marker).is_some_and(|suv| (suv - 5.0).abs() < 1e-3));
        // Where the PET drawn on `grid` peaks: the centre of its pixels within 1 % of the largest value.
        let peak = |grid: &PixelGrid| -> DVec3 {
            let values = resample(volume, grid);
            let max = values.iter().copied().filter(|v| v.is_finite()).fold(f32::MIN, f32::max);
            assert!((max - 5.0).abs() < 1e-3, "SUV 5 at the peak, not {max}");
            let at = |i: usize| grid.to_patient((i % grid.width) as f64, (i / grid.width) as f64);
            let top: Vec<DVec3> = (0..grid.len()).filter(|&i| values[i] >= 0.99 * max).map(at).collect();
            top.iter().copied().sum::<DVec3>() / top.len() as f64
        };

        let mut state = ViewState::new(&ct);
        state.set_cursor(marker, &ct);
        // The acquired CT image through the marker's slice: the marker shows at its own place in it.
        let plane = ct.frames()[state.slot_index(&ct)].plane.expect("plane");
        let normal = plane.normal();
        let on_image = marker - normal * (marker - plane.position).dot(normal);
        let found = peak(&PixelGrid::of_plane(&plane));
        assert!((found - on_image).length() < 4.0, "acquired: {found:?}, marker at {on_image:?}");
        let ct_frame = ct.frame(state.slot_index(&ct)).expect("loaded");
        let (x, y) = {
            let d = on_image - plane.position;
            (
                (d.dot(plane.row_dir) / plane.col_spacing).round() as usize,
                (d.dot(plane.col_dir) / plane.row_spacing).round() as usize,
            )
        };
        let summrise_dicom::FrameRef::Gray { data, width, .. } = ct_frame else { panic!("grey CT") };
        assert_eq!(f64::from(data.get(y * width as usize + x)), CT_MARKER_HU, "the CT marker is there too");

        // Every reformat through the crosshair on the marker, untilted and tilted by 30°.
        let ct_volume = ct.volume().expect("CT volume");
        for tilt in [0.0, 30f64.to_radians()] {
            state.tilt_planes(MprAxis::Axial, tilt);
            for axis in MprAxis::ALL {
                let image = reslice_plane(ct_volume, &state.plane_basis(axis), state.mpr_position(axis), None, 768);
                let found = peak(&PixelGrid::of_reformat(&image));
                assert!((found - marker).length() < 4.0, "{axis:?} tilted {tilt}: {found:?}");
            }
        }
        assert!(state.plane_tilt(MprAxis::Coronal).is_some_and(|t| (t.degrees - 30.0).abs() < 1e-9));
    }

    #[test]
    fn values_are_written_with_their_unit() {
        assert_eq!(value_text(Some("SUV"), 3.137), "SUV 3.14");
        assert_eq!(value_text(Some("SUV"), 5.0), "SUV 5");
        assert_eq!(value_text(Some("HU"), 40.0), "40 HU");
        assert_eq!(value_text(Some("Bq/ml"), 13_214.29), "13214.3 Bq/ml");
        assert_eq!(value_text(None, 820.0), "820");
        assert_eq!((decimals(Some("SUV")), decimals(Some("HU")), decimals(None)), (2, 0, 0));
        assert_eq!(duration_text(6586.2), "1 h 49 min");
        assert_eq!(duration_text(1500.0), "25 min");
        assert_eq!(duration_text(40.0), "40 s");
        assert!(has_colormaps("PT") && has_colormaps("NM") && !has_colormaps("CT"));
    }
}
