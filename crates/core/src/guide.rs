//! Plain-language guide: what a series is, what acquisition parameters and
//! contrast presets mean, and from where the body is seen on screen.
//!
//! Everything here describes how images were made, never what they show:
//! no statement about what is normal or abnormal, about diagnosis or about
//! treatment. Classification prefers "cannot tell" to a label that may be
//! wrong; what is inferred is worded as likely, and every statement carries
//! its basis. The rules and their thresholds are documented in
//! `openspec/changes/plain-language-guide/design.md`.
//!
//! The functions take a [`Series`], which holds no patient attributes (those
//! live in `Study::patient`), so the guide cannot depend on who the patient
//! is and reads the same with privacy mode on or off.

use summrise_dicom::format::number;
use summrise_dicom::{Acquisition, Series, StackKind};
use summrise_i18n::{tr, tr_n};
use summrise_imaging::{AnatomicalPlane, DVec3, PresetSource, WindowPreset};

// ---------------------------------------------------------------------------
// Series description

/// What kind of images a series holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeriesKind {
    Ct,
    Mr,
    /// Overview images used to plan the other series.
    Localizer,
    /// A stored picture of the dose report table.
    DoseReport,
    /// Any other stored picture (screenshot, rendering, scanned document).
    SecondaryCapture,
    Pet,
    NuclearMedicine,
    Ultrasound,
    /// X-ray projection (radiograph).
    Xray,
    Mammogram,
    /// X-ray fluoroscopy or angiography.
    Fluoroscopy,
    /// None of the above: the guide says that it does not recognise it.
    Unknown,
}

/// Contrast of an MR series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MrContrast {
    T1,
    T2,
    ProtonDensity,
    T2Star,
    /// T2-weighted with the signal of fluid suppressed.
    T2Flair,
    /// T1-weighted with the signal of fluid suppressed.
    T1Flair,
    /// Short-TI inversion recovery: the signal of fat suppressed.
    Stir,
    Diffusion,
    /// Apparent diffusion coefficient map, calculated from diffusion images.
    AdcMap,
}

/// How the guide knows the contrast of an MR series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Evidence {
    /// Named in the series description or the protocol name.
    Named,
    /// Recorded by the scanner as a value (b-value, image type).
    Recorded,
    /// Inferred from timing parameters: worded as "likely".
    Inferred,
    /// The rules do not apply: the guide cannot tell.
    Unknown,
}

/// The contrast of an MR series as far as it can be told, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct MrWeighting {
    /// `None` when the guide cannot tell.
    pub contrast: Option<MrContrast>,
    pub evidence: Evidence,
    /// What the verdict rests on, or why there is none; a full sentence.
    pub basis: String,
}

/// One thing the guide says about a series, with what it rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    /// Where the statement comes from: an attribute recorded in the images,
    /// a name typed at the scanner, or an inference with its values.
    pub basis: String,
}

/// The plain-language description of a series.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesGuide {
    pub kind: SeriesKind,
    /// For MR series (that are not localizers): the contrast and its basis.
    pub weighting: Option<MrWeighting>,
    /// Short description, e.g. "MR scan, likely T2-weighted"; also the line
    /// shown in the series browser.
    pub headline: String,
    pub statements: Vec<Statement>,
    /// Whether statements rest on acquisition settings, which are read from
    /// one image of the series (see [`sampling_note`]).
    pub reads_settings: bool,
}

impl SeriesGuide {
    /// Every text of the description (for searching and tests).
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.headline.as_str())
            .chain(self.statements.iter().flat_map(|s| [s.text.as_str(), s.basis.as_str()]))
    }
}

/// The fixed line shown with every description.
pub fn disclaimer() -> &'static str {
    tr!("General information about how these images were made, not an interpretation of the images.")
}

/// Where the settings come from, shown with descriptions that rest on them
/// (see [`SeriesGuide::reads_settings`]).
pub fn sampling_note() -> &'static str {
    tr!("The settings are read from one image of the series; other images of the series may differ.")
}

/// Storage classes of secondary captures (single- and multi-frame).
const SECONDARY_CAPTURE_SOP_CLASSES: [&str; 5] = [
    "1.2.840.10008.5.1.4.1.1.7",
    "1.2.840.10008.5.1.4.1.1.7.1",
    "1.2.840.10008.5.1.4.1.1.7.2",
    "1.2.840.10008.5.1.4.1.1.7.3",
    "1.2.840.10008.5.1.4.1.1.7.4",
];

/// Words that name a localizer in a description or protocol name.
const LOCALIZER_WORDS: [&str; 8] =
    ["localizer", "localiser", "scout", "survey", "topogram", "scanogram", "surview", "loc"];

/// Lower-case words of a name: split at every character that is not a
/// letter, a digit or `*` (as in "T2*"). Whole words are compared, never
/// substrings, so "LOW DOSE CHEST" does not contain "loc".
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '*'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn has_word(words: &[String], word: &str) -> bool {
    words.iter().any(|w| w == word)
}

fn has_image_type(series: &Series, value: &str) -> bool {
    series.acquisition.image_type.iter().any(|t| t.eq_ignore_ascii_case(value))
}

/// The names typed at the scanner, in the order they are consulted.
fn names(series: &Series) -> impl Iterator<Item = (NameSource, &str)> {
    [
        (NameSource::Description, series.description.as_deref()),
        (NameSource::Protocol, series.acquisition.protocol_name.as_deref()),
    ]
    .into_iter()
    .filter_map(|(source, name)| Some((source, name?)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameSource {
    Description,
    Protocol,
}

fn named_basis(source: NameSource, name: &str) -> String {
    match source {
        NameSource::Description => {
            tr!("Named by the scanner operator: the series description reads “{name}”.", name = name)
        }
        NameSource::Protocol => {
            tr!("Named by the scanner operator: the protocol name reads “{name}”.", name = name)
        }
    }
}

fn is_dose_report(series: &Series, secondary_capture: bool) -> bool {
    let Some(description) = series.description.as_deref() else { return false };
    let w = words(description);
    let dose = has_word(&w, "dose") || has_word(&w, "dosis");
    let patient_protocol = has_word(&w, "patient") && has_word(&w, "protocol");
    if secondary_capture {
        return dose || patient_protocol;
    }
    // Some scanners store the dose page as an image of their own modality.
    let page = ["report", "info", "summary", "record", "protocol"].iter().any(|word| has_word(&w, word));
    has_image_type(series, "SECONDARY") && ((dose && page) || patient_protocol)
}

/// Why a series is a localizer, if it is one.
fn localizer_basis(series: &Series) -> Option<String> {
    if has_image_type(series, "LOCALIZER") {
        return Some(tr!("Recorded in the images: image type LOCALIZER.").to_owned());
    }
    names(series)
        .find(|(_, name)| {
            let w = words(name);
            LOCALIZER_WORDS.iter().any(|word| has_word(&w, word))
        })
        .map(|(source, name)| named_basis(source, name))
}

/// What kind of images `series` holds (see the design document, D3).
pub fn series_kind(series: &Series) -> SeriesKind {
    let secondary_capture = series
        .instances
        .first()
        .is_some_and(|instance| SECONDARY_CAPTURE_SOP_CLASSES.contains(&instance.sop_class_uid.trim()));
    if is_dose_report(series, secondary_capture) {
        return SeriesKind::DoseReport;
    }
    if secondary_capture {
        return SeriesKind::SecondaryCapture;
    }
    if localizer_basis(series).is_some() {
        return SeriesKind::Localizer;
    }
    match series.modality.trim().to_ascii_uppercase().as_str() {
        "CT" => SeriesKind::Ct,
        "MR" => SeriesKind::Mr,
        "PT" => SeriesKind::Pet,
        "NM" => SeriesKind::NuclearMedicine,
        "US" | "IVUS" => SeriesKind::Ultrasound,
        "MG" => SeriesKind::Mammogram,
        "XA" | "RF" => SeriesKind::Fluoroscopy,
        "CR" | "DX" | "RG" | "PX" | "IO" => SeriesKind::Xray,
        _ => SeriesKind::Unknown,
    }
}

// ---------------------------------------------------------------------------
// MR weighting

/// Thresholds of the timing rules, in ms and degrees (design document, D4).
mod rule {
    pub(super) const SHORT_TR: f64 = 1000.0;
    pub(super) const LONG_TR: f64 = 2000.0;
    pub(super) const SHORT_TE: f64 = 30.0;
    pub(super) const LONG_TE: f64 = 80.0;
    /// Inversion times that null fat (0.35 T to 3 T).
    pub(super) const STIR_TI: std::ops::RangeInclusive<f64> = 70.0..=260.0;
    pub(super) const STIR_MIN_TR: f64 = 1500.0;
    /// Inversion times that null fluid.
    pub(super) const FLAIR_TI: std::ops::RangeInclusive<f64> = 1500.0..=3000.0;
    pub(super) const FLAIR_MIN_TR: f64 = 4000.0;
    pub(super) const FLAIR_MIN_TE: f64 = 70.0;
    pub(super) const T2_STAR_MIN_TE: f64 = 15.0;
    pub(super) const T2_STAR_MAX_FLIP: f64 = 30.0;
}

/// A weighting word such as "t1", "t1w", "t1wi", "t1ce" or Philips' "sT1W".
fn is_weighting_word(word: &str, base: &str) -> bool {
    const SUFFIXES: [&str; 12] = ["", "w", "wi", "c", "ce", "gd", "km", "fs", "se", "tse", "fse", "weighted"];
    if let Some(rest) = word.strip_prefix(base) {
        return SUFFIXES.contains(&rest);
    }
    word.strip_suffix('w')
        .and_then(|w| w.strip_suffix(base))
        .is_some_and(|prefix| (1..=2).contains(&prefix.len()) && prefix.chars().all(|c| c.is_ascii_alphabetic()))
}

/// The contrast a name states, if it states exactly one.
fn named_contrast(name: &str) -> Option<MrContrast> {
    let w = words(name);
    let any = |test: &dyn Fn(&str) -> bool| w.iter().any(|word| test(word));
    let pair = |first: &str, second: &str| w.windows(2).any(|p| p[0] == first && p[1] == second);

    if any(&|word| word.starts_with("adc")) {
        return Some(MrContrast::AdcMap);
    }
    if any(&|word| {
        word == "dw"
            || word.starts_with("dwi")
            || word == "diff"
            || word.starts_with("diffusion")
            || word == "dti"
            || word == "trace"
            || word == "tracew"
    }) {
        return Some(MrContrast::Diffusion);
    }
    if any(&|word| word.ends_with("stir")) {
        return Some(MrContrast::Stir);
    }
    let t2_star = any(&|word| word.starts_with("t2*") || word.starts_with("t2star")) || pair("t2", "star");
    let t1 = any(&|word| is_weighting_word(word, "t1") || (word.starts_with("t1") && word.contains("flair")));
    let t2 = any(&|word| is_weighting_word(word, "t2") || (word.starts_with("t2") && word.contains("flair")));
    let pd = any(&|word| matches!(word, "pd" | "pdw" | "pdwi" | "pdfs" | "pdweighted")) || pair("proton", "density");
    if any(&|word| word.contains("flair") || word == "darkfluid") || pair("dark", "fluid") {
        return Some(if t1 && !t2 { MrContrast::T1Flair } else { MrContrast::T2Flair });
    }
    if t2_star {
        return Some(MrContrast::T2Star);
    }
    match (t1, t2, pd) {
        (true, false, false) => Some(MrContrast::T1),
        (false, true, false) => Some(MrContrast::T2),
        (false, false, true) => Some(MrContrast::ProtonDensity),
        // No weighting word, or two different ones (a dual-echo "PD T2").
        _ => None,
    }
}

/// The weighting the timing parameters imply, with the values it rests on —
/// or why the rules do not apply.
fn inferred_contrast(a: &Acquisition) -> Result<(MrContrast, String), String> {
    let has = |code: &str| a.scanning_sequence.iter().any(|c| c.eq_ignore_ascii_case(code));
    let positive = |value: Option<f64>| value.filter(|v| *v > 0.0);
    // Repetition, echo and inversion time in ms; flip angle in degrees.
    let repetition = positive(a.repetition_time_ms);
    let echo = positive(a.echo_time_ms);
    let inversion = positive(a.inversion_time_ms);
    let flip = positive(a.flip_angle_deg);
    let ms = |value: f64| number(value, 2);
    let unsettled =
        || Err(tr!("For this kind of sequence the recorded timing does not settle the weighting.").to_owned());
    let no_timing = || Err(tr!("No repetition time or echo time is recorded.").to_owned());

    if a.scanning_sequence.is_empty() {
        return Err(tr!("The images do not record which type of sequence was used.").to_owned());
    }
    let (spin_echo, inverted, gradient_echo, echo_planar) = (has("SE"), has("IR"), has("GR"), has("EP"));
    if gradient_echo {
        if spin_echo || inverted || echo_planar {
            return unsettled();
        }
        return match (echo, flip) {
            (Some(echo), Some(flip)) if echo >= rule::T2_STAR_MIN_TE && flip <= rule::T2_STAR_MAX_FLIP => Ok((
                MrContrast::T2Star,
                tr!(
                    "Inferred from the recorded settings: a gradient-echo sequence with a long echo time ({te} ms) and a small flip angle ({flip} degrees).",
                    te = ms(echo),
                    flip = number(flip, 1)
                ),
            )),
            _ => {
                Err(tr!("For this gradient-echo sequence the recorded timing does not settle the weighting.")
                    .to_owned())
            }
        };
    }
    if !(spin_echo || inverted) || (echo_planar && !spin_echo) {
        return unsettled();
    }
    if inverted || inversion.is_some() {
        // With an inversion pulse the plain spin-echo rules never apply: it changes the contrast.
        let Some(inversion) = inversion else {
            return Err(tr!("An inversion pulse is recorded, but not its timing.").to_owned());
        };
        let Some(repetition) = repetition else { return no_timing() };
        if rule::STIR_TI.contains(&inversion) && repetition >= rule::STIR_MIN_TR {
            return Ok((
                MrContrast::Stir,
                tr!(
                    "Inferred from the recorded timing: repetition time {tr} ms and a short inversion time ({ti} ms), which suppresses the signal of fat.",
                    tr = ms(repetition),
                    ti = ms(inversion)
                ),
            ));
        }
        if let Some(echo) = echo
            && rule::FLAIR_TI.contains(&inversion)
            && repetition >= rule::FLAIR_MIN_TR
            && echo >= rule::FLAIR_MIN_TE
        {
            return Ok((
                MrContrast::T2Flair,
                tr!(
                    "Inferred from the recorded timing: long repetition time ({tr} ms), long echo time ({te} ms) and a long inversion time ({ti} ms), which suppresses the signal of fluid.",
                    tr = ms(repetition),
                    te = ms(echo),
                    ti = ms(inversion)
                ),
            ));
        }
        return Err(tr!("An inversion time of {ti} ms fits none of the rules this guide uses.", ti = ms(inversion)));
    }
    let (Some(repetition), Some(echo)) = (repetition, echo) else { return no_timing() };
    if repetition <= rule::SHORT_TR && echo <= rule::SHORT_TE {
        Ok((
            MrContrast::T1,
            tr!(
                "Inferred from the recorded timing: short repetition time ({tr} ms) and short echo time ({te} ms).",
                tr = ms(repetition),
                te = ms(echo)
            ),
        ))
    } else if repetition >= rule::LONG_TR && echo >= rule::LONG_TE {
        Ok((
            MrContrast::T2,
            tr!(
                "Inferred from the recorded timing: long repetition time ({tr} ms) and long echo time ({te} ms).",
                tr = ms(repetition),
                te = ms(echo)
            ),
        ))
    } else if repetition >= rule::LONG_TR && echo <= rule::SHORT_TE {
        Ok((
            MrContrast::ProtonDensity,
            tr!(
                "Inferred from the recorded timing: long repetition time ({tr} ms) and short echo time ({te} ms).",
                tr = ms(repetition),
                te = ms(echo)
            ),
        ))
    } else {
        Err(tr!(
            "Repetition time {tr} ms and echo time {te} ms fit none of the rules this guide uses.",
            tr = ms(repetition),
            te = ms(echo)
        ))
    }
}

/// The contrast of an MR series: recorded, named, inferred from the timing
/// or unknown, in that order (see the design document, D4).
pub fn classify_mr(series: &Series) -> MrWeighting {
    let a = &series.acquisition;
    let found = |contrast, evidence, basis: String| MrWeighting { contrast: Some(contrast), evidence, basis };
    let named = |wanted: &dyn Fn(MrContrast) -> bool| {
        names(series).find_map(|(source, name)| {
            named_contrast(name).filter(|c| wanted(*c)).map(|c| (c, named_basis(source, name)))
        })
    };

    // Calculated maps and diffusion first: they are facts about the images,
    // whatever weighting the name also mentions.
    if has_image_type(series, "ADC") {
        return found(
            MrContrast::AdcMap,
            Evidence::Recorded,
            tr!("Recorded in the images: image type ADC.").to_owned(),
        );
    }
    if let Some((contrast, basis)) = named(&|c| c == MrContrast::AdcMap) {
        return found(contrast, Evidence::Named, basis);
    }
    if let Some(b) = a.diffusion_b_value.filter(|b| *b > 0.0) {
        return found(
            MrContrast::Diffusion,
            Evidence::Recorded,
            tr!("Recorded in the images: diffusion b-value {b} s/mm².", b = number(b, 0)),
        );
    }
    if has_image_type(series, "DIFFUSION") && !has_image_type(series, "FA") && !has_image_type(series, "TENSOR") {
        return found(
            MrContrast::Diffusion,
            Evidence::Recorded,
            tr!("Recorded in the images: image type DIFFUSION.").to_owned(),
        );
    }
    if let Some((contrast, basis)) = named(&|_| true) {
        return found(contrast, Evidence::Named, basis);
    }
    match inferred_contrast(a) {
        Ok((contrast, basis)) => found(contrast, Evidence::Inferred, basis),
        Err(basis) => MrWeighting { contrast: None, evidence: Evidence::Unknown, basis },
    }
}

fn mr_headline(weighting: &MrWeighting) -> &'static str {
    let likely = weighting.evidence == Evidence::Inferred;
    match (weighting.contrast, likely) {
        (None, _) => tr!("MR scan"),
        (Some(MrContrast::T1), false) => tr!("MR scan, T1-weighted"),
        (Some(MrContrast::T1), true) => tr!("MR scan, likely T1-weighted"),
        (Some(MrContrast::T2), false) => tr!("MR scan, T2-weighted"),
        (Some(MrContrast::T2), true) => tr!("MR scan, likely T2-weighted"),
        (Some(MrContrast::ProtonDensity), false) => tr!("MR scan, proton-density-weighted"),
        (Some(MrContrast::ProtonDensity), true) => tr!("MR scan, likely proton-density-weighted"),
        (Some(MrContrast::T2Star), false) => tr!("MR scan, T2*-weighted"),
        (Some(MrContrast::T2Star), true) => tr!("MR scan, likely T2*-weighted"),
        (Some(MrContrast::T2Flair), false) => tr!("MR scan, T2 FLAIR (fluid suppressed)"),
        (Some(MrContrast::T2Flair), true) => tr!("MR scan, likely T2 FLAIR (fluid suppressed)"),
        (Some(MrContrast::T1Flair), _) => tr!("MR scan, T1 FLAIR (fluid suppressed)"),
        (Some(MrContrast::Stir), false) => tr!("MR scan, STIR (fat suppressed)"),
        (Some(MrContrast::Stir), true) => tr!("MR scan, likely STIR (fat suppressed)"),
        (Some(MrContrast::Diffusion), _) => tr!("MR scan, diffusion-weighted"),
        (Some(MrContrast::AdcMap), _) => tr!("MR scan, ADC map (calculated)"),
    }
}

/// The weighting and what tends to appear bright or dark with it.
fn mr_statement(weighting: &MrWeighting) -> &'static str {
    let likely = weighting.evidence == Evidence::Inferred;
    match (weighting.contrast, likely) {
        (None, _) => tr!("The weighting of these images cannot be told from what is recorded."),
        (Some(MrContrast::T1), false) => {
            tr!("T1-weighted: on such images fat tends to appear bright and fluid dark.")
        }
        (Some(MrContrast::T1), true) => {
            tr!("Likely T1-weighted: on such images fat tends to appear bright and fluid dark.")
        }
        (Some(MrContrast::T2), false) => tr!("T2-weighted: on such images fluid tends to appear bright."),
        (Some(MrContrast::T2), true) => tr!("Likely T2-weighted: on such images fluid tends to appear bright."),
        (Some(MrContrast::ProtonDensity), false) => tr!(
            "Proton-density-weighted: on such images brightness mainly follows how much water and fat a tissue contains, so fluid and fat both appear fairly bright."
        ),
        (Some(MrContrast::ProtonDensity), true) => tr!(
            "Likely proton-density-weighted: on such images brightness mainly follows how much water and fat a tissue contains, so fluid and fat both appear fairly bright."
        ),
        (Some(MrContrast::T2Star), false) => tr!(
            "T2*-weighted: such images are very sensitive to small disturbances of the magnetic field; places where the field is disturbed, for example next to air, tend to appear dark."
        ),
        (Some(MrContrast::T2Star), true) => tr!(
            "Likely T2*-weighted: such images are very sensitive to small disturbances of the magnetic field; places where the field is disturbed, for example next to air, tend to appear dark."
        ),
        (Some(MrContrast::T2Flair), false) => tr!(
            "T2 FLAIR: like a T2-weighted image, but the signal of free fluid is suppressed, so fluid such as that around the brain appears dark."
        ),
        (Some(MrContrast::T2Flair), true) => tr!(
            "Likely T2 FLAIR: like a T2-weighted image, but the signal of free fluid is suppressed, so fluid such as that around the brain appears dark."
        ),
        (Some(MrContrast::T1Flair), _) => tr!(
            "T1 FLAIR: a T1-weighted image made with an extra preparation pulse that suppresses the signal of free fluid, which appears dark."
        ),
        (Some(MrContrast::Stir), false) => {
            tr!("STIR: the signal of fat is suppressed and appears dark, while fluid tends to appear bright.")
        }
        (Some(MrContrast::Stir), true) => {
            tr!("Likely STIR: the signal of fat is suppressed and appears dark, while fluid tends to appear bright.")
        }
        (Some(MrContrast::Diffusion), _) => tr!(
            "Diffusion-weighted: such images are sensitive to how freely water molecules move in the tissue; where they move less freely, the image tends to be brighter."
        ),
        (Some(MrContrast::AdcMap), _) => tr!(
            "ADC map: an image calculated from diffusion-weighted images, not measured directly; brighter means that water moves more freely."
        ),
    }
}

fn kind_headline(kind: SeriesKind) -> &'static str {
    match kind {
        SeriesKind::Ct => tr!("CT scan"),
        SeriesKind::Mr => tr!("MR scan"),
        SeriesKind::Localizer => tr!("Localizer (planning overview)"),
        SeriesKind::DoseReport => tr!("Dose report (a stored picture)"),
        SeriesKind::SecondaryCapture => tr!("Stored picture (secondary capture)"),
        SeriesKind::Pet => tr!("PET scan"),
        SeriesKind::NuclearMedicine => tr!("Nuclear medicine image"),
        SeriesKind::Ultrasound => tr!("Ultrasound image"),
        SeriesKind::Xray => tr!("X-ray image"),
        SeriesKind::Mammogram => tr!("Mammogram (X-ray image)"),
        SeriesKind::Fluoroscopy => tr!("X-ray fluoroscopy or angiography"),
        SeriesKind::Unknown => tr!("Unrecognised kind of images"),
    }
}

fn kind_statement(kind: SeriesKind) -> &'static str {
    match kind {
        SeriesKind::Ct => tr!(
            "A CT scan (computed tomography) measures with X-rays from many directions and computes cross-sections of the body from them. On the usual display, denser means brighter: bone is bright, air is dark."
        ),
        SeriesKind::Mr => tr!(
            "An MR scan (magnetic resonance imaging) is made with a strong magnet and radio waves, without X-rays. Which tissues appear bright depends on the settings of each series."
        ),
        SeriesKind::Localizer => tr!(
            "A localizer (also called scout or survey): quick overview images that the operator uses to plan the other series. They are coarse on purpose."
        ),
        SeriesKind::DoseReport => tr!(
            "A picture of the dose report: a table of the radiation dose values that the scanner recorded for this examination. It is a stored picture, not a scan of the body."
        ),
        SeriesKind::SecondaryCapture => tr!(
            "A stored picture (secondary capture), such as a screenshot, a computed 3D view or a scanned document. It was produced by a workstation or by the scanner software, not measured directly."
        ),
        SeriesKind::Pet => tr!(
            "A PET scan (positron emission tomography) shows where a weakly radioactive tracer has collected in the body."
        ),
        SeriesKind::NuclearMedicine => tr!(
            "A nuclear medicine image shows where a weakly radioactive tracer has collected in the body, recorded by a gamma camera."
        ),
        SeriesKind::Ultrasound => {
            tr!("An ultrasound image is made with sound waves that the tissues reflect, without X-rays.")
        }
        SeriesKind::Xray => tr!(
            "An X-ray image (projection): X-rays pass through the body onto a detector, so structures lying behind each other overlap in one picture."
        ),
        SeriesKind::Mammogram => tr!("A mammogram: an X-ray projection image of the breast."),
        SeriesKind::Fluoroscopy => tr!(
            "X-ray fluoroscopy or angiography: X-ray projection images, often recorded as a moving sequence, for example while a contrast agent flows through vessels."
        ),
        SeriesKind::Unknown => tr!("This guide does not recognise what kind of images these are."),
    }
}

fn kind_basis(series: &Series, kind: SeriesKind) -> String {
    match kind {
        SeriesKind::Localizer => localizer_basis(series),
        SeriesKind::DoseReport => series
            .description
            .as_deref()
            .map(|name| tr!("The files are stored pictures and the series description reads “{name}”.", name = name)),
        SeriesKind::SecondaryCapture => {
            Some(tr!("Recorded in the images: the files are secondary captures.").to_owned())
        }
        _ => None,
    }
    .unwrap_or_else(|| tr!("Recorded in the images: modality {modality}.", modality = series.modality.trim()))
}

/// The plane of the slices in everyday words.
fn plane_statement(series: &Series) -> Option<&'static str> {
    Some(match series.plane() {
        Some(AnatomicalPlane::Axial) => {
            tr!("The slices are axial: horizontal cross-sections of the body, like the slices of a loaf of bread.")
        }
        Some(AnatomicalPlane::Coronal) => {
            tr!("The slices are coronal: they divide the body into front and back, like a view from the front.")
        }
        Some(AnatomicalPlane::Sagittal) => {
            tr!("The slices are sagittal: they divide the body into left and right, like a view from the side.")
        }
        Some(AnatomicalPlane::Oblique) => {
            tr!("The slices are tilted (oblique): they lie between the standard directions.")
        }
        None if series.has_mixed_planes() => tr!("The images of this series lie in different directions."),
        None => return None,
    })
}

/// What the stacks of a series that holds several sets of images at the same
/// slice positions are (`summrise_dicom::Stacks`); `None` when it holds one.
fn stacks_statement(series: &Series) -> Option<Statement> {
    let stacks = summrise_dicom::series_stacks(series)?;
    let n = stacks.count();
    let (text, basis) = match stacks.kind {
        StackKind::TimePoint => (
            tr_n!(
                "This series holds {n} time point of the same slices.",
                "This series holds {n} time points of the same slices.",
                n
            ),
            tr!("Recorded in the images: temporal position."),
        ),
        StackKind::HeartPhase => (
            tr_n!(
                "This series holds {n} phase of the heartbeat at the same slices.",
                "This series holds {n} phases of the heartbeat at the same slices.",
                n
            ),
            tr!("Recorded in the images: trigger time."),
        ),
        StackKind::Echo => (
            tr_n!(
                "This series holds {n} echo of the same slices: images taken at different echo times.",
                "This series holds {n} echoes of the same slices: images taken at different echo times.",
                n
            ),
            tr!("Recorded in the images: echo number and echo time."),
        ),
        StackKind::Diffusion => (
            tr_n!(
                "This series holds {n} diffusion weighting (b-value) of the same slices.",
                "This series holds {n} diffusion weightings (b-values) of the same slices.",
                n
            ),
            tr!("Recorded in the images: diffusion b-value."),
        ),
        StackKind::ImageType => (
            tr_n!(
                "This series holds {n} kind of image of the same slices.",
                "This series holds {n} kinds of image of the same slices.",
                n
            ),
            tr!("Recorded in the images: image type."),
        ),
        StackKind::Acquisition => (
            tr_n!(
                "This series holds {n} acquisition of the same slices.",
                "This series holds {n} acquisitions of the same slices.",
                n
            ),
            tr!("Recorded in the images: acquisition number."),
        ),
        StackKind::Sequence => (
            tr_n!(
                "This series holds {n} set of the same slices, made with a different sequence.",
                "This series holds {n} sets of the same slices, made with different sequences.",
                n
            ),
            tr!("Recorded in the images: sequence name."),
        ),
        StackKind::Part => (
            tr_n!(
                "This series holds {n} set of images of the same slices.",
                "This series holds {n} sets of images of the same slices.",
                n
            ),
            tr!("The images repeat the same slice positions, and nothing recorded in them tells the sets apart."),
        ),
    };
    Some(Statement { text, basis: basis.to_owned() })
}

/// How an MR series was measured: slice by slice or as a block.
fn acquisition_type_statement(a: &Acquisition) -> Option<Statement> {
    let text = match a.mr_acquisition_type.as_deref().map(str::trim) {
        Some(t) if t.eq_ignore_ascii_case("2D") => tr!("The images were measured slice by slice (2D)."),
        Some(t) if t.eq_ignore_ascii_case("3D") => {
            tr!("A whole block of the body was measured at once (3D) and then divided into the slices shown.")
        }
        _ => return None,
    };
    Some(Statement { text: text.to_owned(), basis: tr!("Recorded in the images: MR acquisition type.").to_owned() })
}

/// Values that say "no contrast agent" rather than name one.
fn is_none_marker(agent: &str) -> bool {
    ["none", "no", "n/a", "na", "0", "-"].iter().any(|marker| agent.trim().eq_ignore_ascii_case(marker))
}

fn contrast_agent_statement(a: &Acquisition) -> Statement {
    let not_recorded = || {
        tr!("No contrast agent is recorded for this series, which does not rule one out: not every scanner records it.")
            .to_owned()
    };
    match a.contrast_agent.as_deref().map(str::trim).filter(|agent| !agent.is_empty()) {
        Some(agent) if is_none_marker(agent) => Statement {
            text: not_recorded(),
            basis: tr!("The contrast agent entry of the images reads “{value}”.", value = agent),
        },
        Some(agent) => Statement {
            text: tr!("A contrast agent is recorded for this series: {agent}.", agent = agent),
            basis: tr!("Recorded in the images: contrast agent.").to_owned(),
        },
        None => Statement {
            text: not_recorded(),
            basis: tr!("The contrast agent entry of the images is absent or empty.").to_owned(),
        },
    }
}

fn kind_and_weighting(series: &Series) -> (SeriesKind, Option<MrWeighting>) {
    let kind = series_kind(series);
    (kind, (kind == SeriesKind::Mr).then(|| classify_mr(series)))
}

/// The short line shown under a series' title in the browser: the headline
/// of its description, or `None` when the guide does not recognise the kind.
pub fn series_line(series: &Series) -> Option<&'static str> {
    match kind_and_weighting(series) {
        (SeriesKind::Unknown, _) => None,
        (_, Some(weighting)) => Some(mr_headline(&weighting)),
        (kind, None) => Some(kind_headline(kind)),
    }
}

/// Describes `series` in plain language, every statement with its basis.
pub fn describe_series(series: &Series) -> SeriesGuide {
    let a = &series.acquisition;
    let (kind, weighting) = kind_and_weighting(series);
    let headline = weighting.as_ref().map_or_else(|| kind_headline(kind), mr_headline).to_owned();

    let mut statements = vec![Statement { text: kind_statement(kind).to_owned(), basis: kind_basis(series, kind) }];
    let scan = matches!(kind, SeriesKind::Ct | SeriesKind::Mr | SeriesKind::Pet | SeriesKind::NuclearMedicine);
    // The localizer of a CT is a projection through the body, not a set of slices.
    let slices = scan || (kind == SeriesKind::Localizer && !series.modality.trim().eq_ignore_ascii_case("CT"));
    if slices && let Some(text) = plane_statement(series) {
        statements.push(Statement {
            text: text.to_owned(),
            basis: tr!("Recorded in the images: image orientation.").to_owned(),
        });
    }
    statements.extend(stacks_statement(series));
    if let Some(weighting) = &weighting {
        statements.push(Statement { text: mr_statement(weighting).to_owned(), basis: weighting.basis.clone() });
        statements.extend(acquisition_type_statement(a));
    }
    if scan && let Some(thickness) = a.slice_thickness_mm.filter(|t| *t > 0.0) {
        statements.push(Statement {
            text: tr!("Each image shows a slice of the body {thickness} mm thick.", thickness = number(thickness, 2)),
            basis: tr!("Recorded in the images: slice thickness.").to_owned(),
        });
    }
    if kind == SeriesKind::Ct
        && let Some(kernel) = a.convolution_kernel.as_deref().map(str::trim).filter(|k| !k.is_empty())
    {
        statements.push(Statement {
            text: tr!(
                "Computed with the reconstruction kernel “{kernel}”, as recorded by the scanner. The kernel sets the balance between sharp detail and low grain.",
                kernel = kernel
            ),
            basis: tr!("Recorded in the images: convolution kernel.").to_owned(),
        });
    }
    if matches!(kind, SeriesKind::Ct | SeriesKind::Mr) {
        statements.push(contrast_agent_statement(a));
    }
    SeriesGuide { kind, weighting, headline, statements, reads_settings: scan || kind == SeriesKind::Localizer }
}

// ---------------------------------------------------------------------------
// Parameter explanations

/// Keys of the inspector rows that have an explanation: DICOM keywords, and
/// `Matrix` / `FieldOfView` for the two rows that combine attributes.
pub const EXPLAINED_PARAMETERS: &[&str] = &[
    "RepetitionTime",
    "EchoTime",
    "InversionTime",
    "FlipAngle",
    "EchoTrainLength",
    "NumberOfAverages",
    "ImagingFrequency",
    "PixelBandwidth",
    "ReceiveCoilName",
    "SequenceName",
    "ScanningSequence",
    "SequenceVariant",
    "ScanOptions",
    "MRAcquisitionType",
    "MagneticFieldStrength",
    "KVP",
    "XRayTubeCurrent",
    "Exposure",
    "ExposureTime",
    "CTDIvol",
    "ConvolutionKernel",
    "FilterType",
    "SpiralPitchFactor",
    "RevolutionTime",
    "TotalCollimationWidth",
    "ReconstructionDiameter",
    "GantryDetectorTilt",
    "ContrastBolusAgent",
    "Modality",
    "BodyPartExamined",
    "ProtocolName",
    "SeriesDescription",
    "PatientPosition",
    "Laterality",
    "AccessionNumber",
    "InstanceNumber",
    "ImageType",
    "Matrix",
    "NumberOfFrames",
    "PixelSpacing",
    "FieldOfView",
    "SliceThickness",
    "SpacingBetweenSlices",
    "SliceLocation",
    "ImagePositionPatient",
    "ImageOrientationPatient",
    "PhotometricInterpretation",
    "BitsStored",
    "RescaleSlope",
    "WindowCenter",
    "LossyImageCompression",
    "BurnedInAnnotation",
    "TransferSyntaxUID",
    "SOPClassUID",
    "StudyInstanceUID",
    "SeriesInstanceUID",
    "SOPInstanceUID",
    "FrameOfReferenceUID",
    // PET uptake values (the inspector's section of `crate::pet`)
    "SUVbw",
    "Units",
    "CorrectedImage",
    "DecayCorrection",
    "RadionuclideTotalDose",
    "RadionuclideHalfLife",
    "RadiopharmaceuticalStartTime",
    "SeriesTime",
    "Radiopharmaceutical",
];

/// One or two plain sentences about the inspector row with this key (see
/// `summrise_dicom::SummaryRow::key`), including what its unit means.
/// `None` when the row has no explanation; rows about the patient have none.
pub fn explain_parameter(key: &str) -> Option<&'static str> {
    Some(match key {
        // MR acquisition
        "RepetitionTime" => tr!(
            "The time between two successive excitations of the same slice, in milliseconds (ms, thousandths of a second). Together with the echo time it largely decides which tissues appear bright."
        ),
        "EchoTime" => tr!(
            "The time between exciting the tissue and measuring its signal, in milliseconds (ms, thousandths of a second). With a long echo time, fluid stands out as bright."
        ),
        "InversionTime" => tr!(
            "The waiting time after an extra preparation pulse, in milliseconds (ms, thousandths of a second). It is chosen so that the signal of one tissue, such as fat or fluid, is suppressed."
        ),
        "FlipAngle" => tr!(
            "How far the radio pulse tips the magnetisation of the tissue, in degrees. It influences brightness and contrast."
        ),
        "EchoTrainLength" => {
            tr!("The number of echoes measured after each excitation. More echoes shorten the scan.")
        }
        "NumberOfAverages" => tr!(
            "How many times each measurement was repeated and averaged. More averages give a less grainy image but take longer."
        ),
        "ImagingFrequency" => tr!(
            "The radio frequency the scanner worked at, in megahertz (MHz, millions of cycles per second). It follows from the strength of the magnet."
        ),
        "PixelBandwidth" => tr!(
            "The range of signal frequencies that falls into one pixel, in hertz per pixel (Hz/px). It affects image grain and some distortions."
        ),
        "ReceiveCoilName" => tr!(
            "The receiver coil, a kind of antenna placed near the body, that picked up the signal, as named by the scanner."
        ),
        "SequenceName" => tr!(
            "The scanner's internal name of the measuring programme (pulse sequence). The names differ between manufacturers."
        ),
        "ScanningSequence" => tr!(
            "The basic measuring technique: spin echo, gradient echo, inversion recovery or echo planar. It shapes how tissues differ in brightness."
        ),
        "SequenceVariant" => tr!(
            "Options of the measuring technique, recorded by the scanner as short codes, for example SP for spoiled or SK for segmented."
        ),
        "ScanOptions" => tr!(
            "Further options used for the scan, recorded by the scanner as short codes, for example FS for fat suppression."
        ),
        "MRAcquisitionType" => tr!(
            "2D means the body was measured slice by slice. 3D means a whole block was measured at once and then divided into slices."
        ),
        "MagneticFieldStrength" => {
            tr!("The strength of the scanner's main magnet, in tesla (T). Most scanners have 1.5 T or 3 T.")
        }
        // CT acquisition
        "KVP" => tr!(
            "The voltage of the X-ray tube, in kilovolts (kV, thousands of volts). It sets how penetrating the X-rays are."
        ),
        "XRayTubeCurrent" => tr!(
            "The electric current through the X-ray tube, in milliamperes (mA, thousandths of an ampere). A higher current produces more X-rays: a less grainy image at a higher dose."
        ),
        "Exposure" => tr!(
            "Tube current multiplied by exposure time, in milliampere-seconds (mAs). It is a measure of the amount of X-rays used for this image."
        ),
        "ExposureTime" => {
            tr!("How long the X-rays were on for this image, in milliseconds (ms, thousandths of a second).")
        }
        "CTDIvol" => tr!(
            "A standardised measure of the radiation output of the scanner for this scan, in milligray (mGy). It describes the scanner setting, not the dose an individual person received."
        ),
        "ConvolutionKernel" => tr!(
            "The mathematical filter used to compute the images, as named by the scanner. Sharp kernels show fine detail and edges; smooth kernels reduce grain."
        ),
        "FilterType" => tr!("The filter placed in the X-ray beam inside the scanner, as named by the manufacturer."),
        "SpiralPitchFactor" => tr!(
            "How far the table moves during one turn of the X-ray tube, relative to the width of the X-ray beam. At 1 the turns of the spiral lie side by side without gap or overlap."
        ),
        "RevolutionTime" => tr!("The time the X-ray tube needs for one turn around the body, in seconds (s)."),
        "TotalCollimationWidth" => tr!("The width of the X-ray beam along the body, in millimetres (mm)."),
        "ReconstructionDiameter" => {
            tr!("The diameter of the circular area that was computed into the image, in millimetres (mm).")
        }
        "GantryDetectorTilt" => {
            tr!("The angle by which the scanner ring was tilted from the upright position, in degrees.")
        }
        "ContrastBolusAgent" => tr!(
            "The contrast agent recorded for this series. Contrast agents make vessels or certain tissues stand out in the images."
        ),
        // Series and study
        "Modality" => tr!(
            "The kind of equipment that made the images, for example CT (computed tomography) or MR (magnetic resonance)."
        ),
        "BodyPartExamined" => tr!(
            "The body region entered at the scanner for this series. It is picked from a list by the operator and can be imprecise."
        ),
        "ProtocolName" => {
            tr!("The name of the scan programme selected at the scanner. Hospitals choose these names themselves.")
        }
        "SeriesDescription" => tr!(
            "The name given to this series at the scanner, usually an abbreviation of the technique and the direction of the slices."
        ),
        "PatientPosition" => tr!("How the person lay in the scanner, for example head first and on the back."),
        "Laterality" => tr!("The side of the body that was examined: R for right, L for left."),
        "AccessionNumber" => {
            tr!("The number under which the hospital's information system filed this examination.")
        }
        // Image
        "InstanceNumber" => tr!("The number of this image within its series, as assigned by the scanner."),
        "ImageType" => tr!(
            "Codes describing how the image came about, for example ORIGINAL for a direct measurement or DERIVED for an image computed from others."
        ),
        "Matrix" => tr!("The number of picture points (pixels) across and down the image."),
        "NumberOfFrames" => tr!("The number of images stored together in this file."),
        "PixelSpacing" => {
            tr!("The size of the piece of the body that one pixel stands for, in millimetres (mm): across by down.")
        }
        "FieldOfView" => {
            tr!("The width and height of the area of the body that the image covers, in millimetres (mm).")
        }
        "SliceThickness" => tr!(
            "The thickness of the layer of the body shown in one image, in millimetres (mm). Thinner slices show finer detail but look grainier."
        ),
        "SpacingBetweenSlices" => tr!(
            "The distance from the middle of one slice to the middle of the next, in millimetres (mm). If it is larger than the slice thickness, there are gaps between the slices."
        ),
        "SliceLocation" => tr!(
            "The position of this slice along the scan direction, in millimetres (mm), counted from a reference point set by the scanner."
        ),
        "ImagePositionPatient" => tr!(
            "Where the first pixel of this image lies in the body, in millimetres (mm): towards the patient's left, towards the back and towards the head, counted from a reference point set by the scanner."
        ),
        "ImageOrientationPatient" => tr!(
            "The direction of the image through the body and the directions of its rows and columns. The letters stand for the patient's right (R), left (L), front (A), back (P), head (S) and feet (I)."
        ),
        "PhotometricInterpretation" => tr!(
            "How the stored numbers translate into grey levels or colours, for example MONOCHROME2: larger numbers are brighter."
        ),
        "BitsStored" => tr!(
            "How many bits hold each pixel value, which limits the number of different grey levels: 12 bits allow 4096 levels."
        ),
        "RescaleSlope" => tr!(
            "The rule that turns stored numbers into meaningful values: stored number times slope plus intercept. For CT the result is in Hounsfield units (HU), a density scale on which water is 0 and air is -1000."
        ),
        "WindowCenter" => tr!(
            "Brightness and contrast settings stored with the image by the scanner or its operator. C is the centre and W the width of the range of values shown from black to white."
        ),
        "LossyImageCompression" => {
            tr!("Whether the image was compressed in a way that discards some detail to save space.")
        }
        "BurnedInAnnotation" => tr!(
            "Whether the equipment wrote text, such as names or dates, into the picture itself. Such text is part of the image and cannot be hidden by this viewer."
        ),
        "TransferSyntaxUID" => {
            tr!("The technical format in which the image data is stored in the file, including any compression.")
        }
        "SOPClassUID" => tr!(
            "The kind of DICOM object this file is, for example a CT image or a secondary capture, which is a stored picture."
        ),
        // Identifiers
        "StudyInstanceUID" => {
            tr!("A worldwide unique technical number of this examination. It has no medical meaning.")
        }
        "SeriesInstanceUID" => tr!("A worldwide unique technical number of this series. It has no medical meaning."),
        "SOPInstanceUID" => tr!("A worldwide unique technical number of this image. It has no medical meaning."),
        "FrameOfReferenceUID" => tr!(
            "Images that share this number use the same coordinate system, so positions can be compared between them."
        ),
        // PET uptake values
        "SUVbw" => tr!(
            "The standardised uptake value (SUV) compares the amount of tracer in a spot with the amount there would be if the injected tracer were spread evenly through the whole body, so 1 is that average. It is computed from the weight, the injected dose and the times that staff entered at the scanner, so it is approximate and only as exact as those entries."
        ),
        "Units" => tr!(
            "The unit in which the scanner stored the values, for example BQML: becquerel per millilitre, the measured concentration of radioactivity."
        ),
        "CorrectedImage" => tr!(
            "Corrections the scanner applied to the values, as short codes: for example ATTN for attenuation, the weakening of the signal inside the body, and DECY for the decay of the radioactivity."
        ),
        "DecayCorrection" => tr!(
            "The moment to which the scanner corrected the values for the decay of the radioactivity: START means the start of the scan."
        ),
        "RadionuclideTotalDose" => tr!(
            "The amount of radioactivity injected, in megabecquerel (MBq, millions of decays per second), as entered by staff."
        ),
        "RadionuclideHalfLife" => tr!(
            "The time in which the radioactivity of the tracer falls to half, in seconds (s): about 110 minutes for fluorine-18."
        ),
        "RadiopharmaceuticalStartTime" => tr!(
            "The time the tracer was injected, as entered by staff. The radioactivity decays between this time and the scan, which the uptake value takes into account."
        ),
        "SeriesTime" => {
            tr!("The time the scanner recorded for this series; for a PET series, the moment its values refer to.")
        }
        "Radiopharmaceutical" => tr!(
            "The tracer that was injected, as entered at the scanner, for example fluorodeoxyglucose (FDG), a form of sugar."
        ),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Contrast presets

/// What a contrast window is for, and which values it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetExplanation {
    /// One sentence: what the window is for.
    pub purpose: String,
    /// One sentence: the values the grey scale is spread over.
    pub range: String,
}

/// Explains a contrast preset. Built-in CT windows are recognised by their
/// source and their width and level, never by their name, so that names can
/// be reworded or translated. `units` is the unit of the grey values, e.g.
/// "HU".
pub fn explain_preset(preset: &WindowPreset, units: Option<&str>) -> PresetExplanation {
    let window = &preset.window;
    let purpose = match preset.source {
        PresetSource::Dicom => {
            tr!(
                "A window stored with the images by the scanner or its operator, usually the one intended for this series."
            )
        }
        PresetSource::Auto => tr!(
            "A window computed from the brightness values of the images loaded so far: it shows almost all of them and ignores the extremes."
        ),
        PresetSource::FullRange => tr!(
            "A window from the smallest to the largest value in the series: nothing is clipped, but contrast is low."
        ),
        // The built-in uptake windows: only series in SUV have them (`crate::pet`).
        PresetSource::Clinical if crate::pet::is_suv(units) => tr!(
            "A built-in window for uptake values (SUV) from SUV 0 to its upper limit: a higher limit keeps differences between high values visible, a lower one shows low values more clearly."
        ),
        PresetSource::Clinical => clinical_purpose(window.width, window.center),
    };
    let decimals = crate::pet::decimals(units);
    let (low, high) = (number(window.low(), decimals), number(window.high(), decimals));
    let range = match units.map(str::trim).filter(|u| !u.is_empty()) {
        Some(unit) if unit.eq_ignore_ascii_case("HU") => tr!(
            "It spreads the grey scale over densities from {low} to {high} HU (Hounsfield units: air is -1000, water 0); everything outside that range is shown as black or white.",
            low = low,
            high = high
        ),
        Some(unit) => tr!(
            "It spreads the grey scale over values from {low} to {high} {unit}; everything outside that range is shown as black or white.",
            low = low,
            high = high,
            unit = unit
        ),
        None => tr!(
            "It spreads the grey scale over values from {low} to {high}; everything outside that range is shown as black or white.",
            low = low,
            high = high
        ),
    };
    PresetExplanation { purpose: purpose.to_owned(), range }
}

/// What a contrast table stored with the images (VOI LUT Sequence) is.
pub fn explain_contrast_table() -> &'static str {
    tr!(
        "A contrast table stored with the images by the equipment: it turns values into grey levels along a curve chosen for these images instead of a straight line. Adjusting the contrast by hand switches to an ordinary window."
    )
}

/// What a built-in CT window (width / level in HU) is for.
fn clinical_purpose(width: f64, level: f64) -> &'static str {
    match (width.round() as i64, level.round() as i64) {
        (80, 40) => tr!(
            "A narrow window around the density of brain tissue, so that small density differences inside the brain become visible."
        ),
        (200, 75) => tr!(
            "A wider window for the head, in which the thin layer between the brain and the skull stays distinguishable from the bone."
        ),
        (40, 40) => tr!(
            "A very narrow window that exaggerates small density differences in brain tissue; the image looks grainy with it."
        ),
        (380, 40) => tr!("A window for the soft tissues of the head and neck, such as muscles and glands."),
        (2800, 600) => tr!(
            "A very wide window centred on bone, so that fine bone structures, such as those of the ear, stay distinguishable."
        ),
        (1500, -600) => tr!(
            "A wide window centred on low densities, so that the air-filled lungs with their vessels and airways become visible."
        ),
        (350, 50) => tr!(
            "A window for the soft tissues in the middle of the chest, such as the heart and the large vessels; the lungs look black with it."
        ),
        (400, 50) => {
            tr!("A window for the soft tissues of the abdomen, such as the liver, the kidneys and the bowel.")
        }
        (150, 30) => tr!(
            "A narrow window around the density of the liver, so that small density differences inside it become visible."
        ),
        (1800, 400) => tr!(
            "A wide window centred on high densities, so that the outer shell and the inner structure of bones become visible."
        ),
        (600, 300) => tr!(
            "A window centred above soft tissue, so that vessels filled with contrast agent stand out and stay distinguishable from bone."
        ),
        _ => tr!("A standard CT window."),
    }
}

// ---------------------------------------------------------------------------
// Viewing direction

/// From where the body is seen in the image on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewingDirection {
    /// The standard plane the image lies in, or oblique.
    pub plane: AnatomicalPlane,
    /// "Axial image", "Tilted (oblique) image"…
    pub title: &'static str,
    /// "You are looking from the feet towards the head."
    pub looking: &'static str,
    /// "The patient's right side is on the left of the screen."
    pub screen_left: &'static str,
    /// The image is more than 10° off the patient axes: a sentence says
    /// "roughly".
    pub approximate: bool,
}

/// Cosine of the largest angle (10°) between a direction and a patient axis
/// for which a sentence is stated as exact; beyond it the sentence says
/// "roughly".
const EXACT_COSINE: f64 = 0.984_807_753_012_208;

/// The patient axis a direction is closest to: `axis` 0, 1, 2 for left
/// (+X), back (+Y) and head (+Z); `positive` false for right, front and
/// feet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AxisDirection {
    axis: usize,
    positive: bool,
    /// Within 10° of the axis.
    aligned: bool,
}

fn dominant(v: DVec3) -> Option<AxisDirection> {
    let v = v.try_normalize()?;
    let a = v.abs().to_array();
    let axis = if a[2] >= a[0] && a[2] >= a[1] {
        2
    } else if a[1] >= a[0] {
        1
    } else {
        0
    };
    Some(AxisDirection { axis, positive: v.to_array()[axis] > 0.0, aligned: a[axis] >= EXACT_COSINE })
}

/// Describes from where the body is seen, given the patient-space (LPS)
/// directions of screen-right and screen-down of the image on screen. The
/// viewer looks along `right × down`. `None` for degenerate directions.
pub fn viewing_direction(right: DVec3, down: DVec3) -> Option<ViewingDirection> {
    let look = dominant(right.cross(down))?;
    let left = dominant(-right)?;
    let looking = match (look.axis, look.positive, look.aligned) {
        (2, true, true) => tr!("You are looking from the feet towards the head."),
        (2, false, true) => tr!("You are looking from the head towards the feet."),
        (1, true, true) => tr!("You are looking from the front of the body towards the back."),
        (1, false, true) => tr!("You are looking from the back of the body towards the front."),
        (_, true, true) => tr!("You are looking from the patient's right side towards the left."),
        (_, false, true) => tr!("You are looking from the patient's left side towards the right."),
        (2, true, false) => tr!("The image is tilted: you are looking roughly from the feet towards the head."),
        (2, false, false) => tr!("The image is tilted: you are looking roughly from the head towards the feet."),
        (1, true, false) => {
            tr!("The image is tilted: you are looking roughly from the front of the body towards the back.")
        }
        (1, false, false) => {
            tr!("The image is tilted: you are looking roughly from the back of the body towards the front.")
        }
        (_, true, false) => {
            tr!("The image is tilted: you are looking roughly from the patient's right side towards the left.")
        }
        (_, false, false) => {
            tr!("The image is tilted: you are looking roughly from the patient's left side towards the right.")
        }
    };
    let screen_left = match (left.axis, left.positive, left.aligned) {
        (0, false, true) => tr!("The patient's right side is on the left of the screen."),
        (0, true, true) => tr!("The patient's left side is on the left of the screen."),
        (1, false, true) => tr!("The front of the body is on the left of the screen."),
        (1, true, true) => tr!("The back of the body is on the left of the screen."),
        (_, true, true) => tr!("The head end is on the left of the screen."),
        (_, false, true) => tr!("The feet end is on the left of the screen."),
        (0, false, false) => tr!("Roughly, the patient's right side is on the left of the screen."),
        (0, true, false) => tr!("Roughly, the patient's left side is on the left of the screen."),
        (1, false, false) => tr!("Roughly, the front of the body is on the left of the screen."),
        (1, true, false) => tr!("Roughly, the back of the body is on the left of the screen."),
        (_, true, false) => tr!("Roughly, the head end is on the left of the screen."),
        (_, false, false) => tr!("Roughly, the feet end is on the left of the screen."),
    };
    let plane = AnatomicalPlane::from_normal(right.cross(down).normalize_or_zero());
    let title = match plane {
        AnatomicalPlane::Axial => tr!("Axial image"),
        AnatomicalPlane::Coronal => tr!("Coronal image"),
        AnatomicalPlane::Sagittal => tr!("Sagittal image"),
        AnatomicalPlane::Oblique => tr!("Tilted (oblique) image"),
    };
    Some(ViewingDirection { plane, title, looking, screen_left, approximate: !(look.aligned && left.aligned) })
}

// ---------------------------------------------------------------------------
// Body region

/// A region of the body that the position figure can highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyRegion {
    Head,
    Neck,
    HeadNeck,
    Chest,
    Abdomen,
    Pelvis,
    NeckChest,
    ChestAbdomen,
    AbdomenPelvis,
    ChestAbdomenPelvis,
    NeckChestAbdomen,
    NeckChestAbdomenPelvis,
    CervicalSpine,
    ThoracicSpine,
    LumbarSpine,
    SacralSpine,
    CervicothoracicSpine,
    ThoracolumbarSpine,
    WholeSpine,
    Shoulder,
    UpperArm,
    Elbow,
    Forearm,
    Wrist,
    Hand,
    WholeArm,
    Hip,
    Thigh,
    Knee,
    LowerLeg,
    Ankle,
    Foot,
    WholeLeg,
    WholeBody,
}

/// The body region a Body Part Examined value (or an anatomic region name)
/// stands for. Case, spaces and punctuation are ignored. `None` for values
/// the guide does not recognise: it never guesses.
pub fn body_region(body_part: &str) -> Option<BodyRegion> {
    use BodyRegion::*;
    let key: String = body_part.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_uppercase();
    Some(match key.as_str() {
        "HEAD" | "SKULL" | "BRAIN" | "CRANIUM" | "ORBIT" | "ORBITS" | "EYE" | "FACE" | "FACIALBONES" | "JAW"
        | "MANDIBLE" | "MAXILLA" | "TMJ" | "SINUS" | "SINUSES" | "PARANASALSINUS" | "PARANASALSINUSES" | "NOSE"
        | "EAR" | "IAC" | "IAM" | "PITUITARY" | "SELLA" | "TEMPORALBONE" | "MASTOID" => Head,
        "NECK" | "THYROID" | "LARYNX" | "PHARYNX" | "CAROTID" | "CAROTIDS" => Neck,
        "HEADNECK" | "HEADANDNECK" => HeadNeck,
        "CHEST" | "THORAX" | "LUNG" | "LUNGS" | "HEART" | "MEDIASTINUM" | "RIB" | "RIBS" | "STERNUM" | "BREAST"
        | "BREASTS" => Chest,
        "ABDOMEN" | "UPPERABDOMEN" | "LIVER" | "KIDNEY" | "KIDNEYS" | "PANCREAS" | "SPLEEN" | "STOMACH"
        | "GALLBLADDER" => Abdomen,
        "PELVIS" | "BLADDER" | "PROSTATE" | "UTERUS" | "OVARY" | "OVARIES" | "RECTUM" => Pelvis,
        "NECKCHEST" => NeckChest,
        "CHESTABDOMEN" | "THORAXABDOMEN" => ChestAbdomen,
        "ABDOMENPELVIS" | "ABDOPELVIS" | "ABDPELVIS" | "ABDPEL" => AbdomenPelvis,
        "CHESTABDPELVIS" | "CHESTABDOMENPELVIS" | "THORAXABDOMENPELVIS" | "TRUNK" | "TORSO" => ChestAbdomenPelvis,
        "NECKCHESTABDOMEN" => NeckChestAbdomen,
        "NECKCHESTABDPELV" | "NECKCHESTABDPELVIS" | "NECKCHESTABDOMENPELVIS" => NeckChestAbdomenPelvis,
        "CSPINE" | "CERVICALSPINE" => CervicalSpine,
        "TSPINE" | "THORACICSPINE" | "DSPINE" | "DORSALSPINE" => ThoracicSpine,
        "LSPINE" | "LUMBARSPINE" | "LSSPINE" | "LUMBOSACRALSPINE" => LumbarSpine,
        "SSPINE" | "SACRALSPINE" | "SACRUM" | "COCCYX" | "SIJOINT" | "SIJOINTS" | "SACROILIACJOINT"
        | "SACROILIACJOINTS" => SacralSpine,
        "CTSPINE" | "CERVICOTHORACICSPINE" => CervicothoracicSpine,
        "TLSPINE" | "THORACOLUMBARSPINE" => ThoracolumbarSpine,
        "SPINE" | "WHOLESPINE" | "TOTALSPINE" | "ENTIRESPINE" => WholeSpine,
        "SHOULDER" | "SHOULDERS" | "CLAVICLE" | "SCAPULA" => Shoulder,
        "UPPERARM" | "HUMERUS" => UpperArm,
        "ELBOW" => Elbow,
        "FOREARM" => Forearm,
        "WRIST" => Wrist,
        "HAND" | "HANDS" | "FINGER" | "FINGERS" | "THUMB" => Hand,
        "ARM" | "UPPERLIMB" | "UPPEREXTREMITY" => WholeArm,
        "HIP" | "HIPS" => Hip,
        "THIGH" | "FEMUR" | "UPPERLEG" => Thigh,
        "KNEE" | "KNEES" | "PATELLA" => Knee,
        "LOWERLEG" | "CALF" | "TIBIA" | "FIBULA" | "TIBIAFIBULA" | "TIBFIB" => LowerLeg,
        "ANKLE" | "ANKLES" => Ankle,
        "FOOT" | "FEET" | "TOE" | "TOES" | "HEEL" | "CALCANEUS" => Foot,
        "LEG" | "LEGS" | "LOWERLIMB" | "LOWEREXTREMITY" => WholeLeg,
        "WHOLEBODY" | "TOTALBODY" => WholeBody,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Where the image on screen lies in the scanned block

/// The block of the body a series covers: the bounding box, along the
/// patient axes (LPS, mm), of the corners of every image of the series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub min: DVec3,
    pub max: DVec3,
}

impl Block {
    /// Extent along patient left (x), back (y) and head (z), in mm.
    pub fn size(&self) -> DVec3 {
        self.max - self.min
    }
}

/// The block covered by `series`, from the indexed headers. `None` when no
/// image of the series has a position (projections, stored pictures).
pub fn scanned_block(series: &Series) -> Option<Block> {
    series
        .instances
        .iter()
        .flat_map(|instance| instance.frame_planes.iter().flatten())
        .flat_map(|plane| {
            let across = plane.row_dir * (f64::from(plane.cols.saturating_sub(1)) * plane.col_spacing);
            let down = plane.col_dir * (f64::from(plane.rows.saturating_sub(1)) * plane.row_spacing);
            [plane.position, plane.position + across, plane.position + down, plane.position + across + down]
        })
        .filter(|corner| corner.is_finite())
        .fold(None, |block: Option<Block>, corner| {
            Some(block.map_or(Block { min: corner, max: corner }, |b| Block {
                min: b.min.min(corner),
                max: b.max.max(corner),
            }))
        })
}

/// The image a viewport shows, in patient space (LPS, mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPlane {
    /// Direction of screen-right.
    pub right: DVec3,
    /// Direction of screen-down.
    pub down: DVec3,
    /// Corners of the image: top left, top right, bottom right, bottom left.
    pub corners: [DVec3; 4],
}

/// Where an image lies inside a [`Block`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The corners of the image as fractions of the block: `[x, y, z]`, each
    /// from 0 to 1 — from the patient's right to left, from front to back
    /// and from the feet end to the head end. 0.5 along a direction in which
    /// the block has no extent.
    pub corners: [[f64; 3]; 4],
    /// Extent of the block along x, y and z, in mm.
    pub size_mm: [f64; 3],
}

/// Expresses the corners of `plane` as fractions of `block`.
pub fn place(block: &Block, plane: &ScreenPlane) -> Placement {
    let size = block.size();
    let fraction = |corner: DVec3| {
        let mut out = [0.5; 3];
        for (axis, value) in out.iter_mut().enumerate() {
            let extent = size.to_array()[axis];
            if extent > 1e-6 {
                *value = ((corner.to_array()[axis] - block.min.to_array()[axis]) / extent).clamp(0.0, 1.0);
            }
        }
        out
    };
    Placement { corners: plane.corners.map(fraction), size_mm: size.to_array() }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use summrise_dicom::{InstanceHeader, Rescale};
    use summrise_imaging::{CT_PRESETS, ImagePlane, Window};
    use summrise_testkit::{PATIENT_ID, PhantomOptions};

    use super::*;
    use crate::series::testing::phantom;

    const SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";
    const CT_IMAGE: &str = "1.2.840.10008.5.1.4.1.1.2";

    /// Words that would make a text an interpretation of the images.
    const INTERPRETIVE: [&str; 34] = [
        "normal",
        "abnormal",
        "tumour",
        "tumor",
        "lesion",
        "disease",
        "diagnos",
        "patholog",
        "cancer",
        "healthy",
        "benign",
        "malignan",
        "inflam",
        "infect",
        "fracture",
        "bleed",
        "haemorrhage",
        "hemorrhage",
        "treatment",
        "therapy",
        "prognos",
        "finding",
        "suspicious",
        "injur",
        "infarct",
        "stroke",
        "cyst",
        "metasta",
        "oedema",
        "edema",
        "worr",
        "concern",
        "harmless",
        "dangerous",
    ];

    /// Sentence ends: '.', '!' or '?' followed by white space or the end.
    fn sentences(text: &str) -> usize {
        let chars: Vec<char> = text.chars().collect();
        (0..chars.len())
            .filter(|&i| matches!(chars[i], '.' | '!' | '?') && chars.get(i + 1).is_none_or(|c| c.is_whitespace()))
            .count()
    }

    /// No word that interprets images, and no placeholder left unfilled.
    #[track_caller]
    fn assert_no_interpretation(text: &str) {
        assert!(!text.trim().is_empty(), "empty text");
        let lower = text.to_lowercase();
        for word in INTERPRETIVE {
            assert!(!lower.contains(word), "interpretive word {word:?} in: {text}");
        }
        assert!(!text.contains('{') && !text.contains('}'), "unfilled placeholder in: {text}");
    }

    /// An explanation: one or two sentences without interpretation.
    #[track_caller]
    fn assert_plain(text: &str) {
        assert_no_interpretation(text);
        assert!((1..=2).contains(&sentences(text)), "one or two sentences, not {}: {text}", sentences(text));
    }

    fn series(modality: &str, description: Option<&str>, acquisition: Acquisition) -> Series {
        Series {
            uid: "1.2.3".to_owned(),
            number: Some(1),
            description: description.map(str::to_owned),
            modality: modality.to_owned(),
            body_part: None,
            acquisition,
            frame_of_reference: None,
            pet: None,
            instances: Vec::new(),
        }
    }

    fn instance(sop_class: &str, plane: Option<ImagePlane>) -> InstanceHeader {
        InstanceHeader {
            path: PathBuf::from("IM000001"),
            sop_instance_uid: "1.2.3.1".to_owned(),
            sop_class_uid: sop_class.to_owned(),
            transfer_syntax_uid: "1.2.840.10008.1.2.1".to_owned(),
            instance_number: Some(1),
            frames: 1,
            rows: 4,
            cols: 4,
            samples_per_pixel: 1,
            photometric: "MONOCHROME2".to_owned(),
            pixel_spacing: Some([1.0, 1.0]),
            frame_planes: vec![plane],
            image_type: Vec::new(),
            bits_stored: 12,
            signed: false,
            rescale: vec![Rescale::IDENTITY],
            rescale_type: None,
            frame_values: Vec::new(),
            fidelity: Default::default(),
        }
    }

    fn codes(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    /// MR settings: scanning sequence, TR and TE in ms.
    fn timing(sequence: &[&str], tr: f64, te: f64) -> Acquisition {
        Acquisition {
            scanning_sequence: codes(sequence),
            repetition_time_ms: Some(tr),
            echo_time_ms: Some(te),
            ..Acquisition::default()
        }
    }

    fn mr(description: Option<&str>, acquisition: Acquisition) -> MrWeighting {
        classify_mr(&series("MR", description, acquisition))
    }

    #[track_caller]
    fn assert_inferred(weighting: &MrWeighting, contrast: MrContrast, why: &str) {
        assert_eq!((weighting.contrast, weighting.evidence), (Some(contrast), Evidence::Inferred), "{weighting:?}");
        assert!(weighting.basis.contains(why), "{:?} does not show {why:?}", weighting.basis);
        assert!(mr_headline(weighting).contains("likely"), "{}", mr_headline(weighting));
        assert!(mr_statement(weighting).starts_with("Likely "), "{}", mr_statement(weighting));
    }

    #[track_caller]
    fn assert_unknown(weighting: &MrWeighting, why: &str) {
        assert_eq!((weighting.contrast, weighting.evidence), (None, Evidence::Unknown), "{weighting:?}");
        assert!(weighting.basis.contains(why), "{:?} does not say {why:?}", weighting.basis);
        assert_eq!(mr_headline(weighting), "MR scan");
        assert_eq!(mr_statement(weighting), "The weighting of these images cannot be told from what is recorded.");
    }

    #[test]
    fn spin_echo_timing_gives_a_likely_weighting_with_its_values() {
        // Spin echo, TR 500 / TE 12 ms.
        assert_inferred(
            &mr(None, timing(&["SE"], 500.0, 12.0)),
            MrContrast::T1,
            "short repetition time (500 ms) and short echo time (12 ms)",
        );
        // Turbo spin echo, TR 4000 / TE 100 ms.
        assert_inferred(
            &mr(None, timing(&["SE"], 4000.0, 100.0)),
            MrContrast::T2,
            "long repetition time (4000 ms) and long echo time (100 ms)",
        );
        // TR 2500 / TE 20 ms.
        assert_inferred(
            &mr(None, timing(&["SE"], 2500.0, 20.0)),
            MrContrast::ProtonDensity,
            "long repetition time (2500 ms) and short echo time (20 ms)",
        );
        // The limits belong to the rules.
        assert_inferred(&mr(None, timing(&["SE"], 1000.0, 30.0)), MrContrast::T1, "(1000 ms)");
        assert_inferred(&mr(None, timing(&["SE"], 2000.0, 80.0)), MrContrast::T2, "(80 ms)");
        // A spin-echo echo-planar image without diffusion weighting.
        assert_inferred(&mr(None, timing(&["EP", "SE"], 5000.0, 90.0)), MrContrast::T2, "(5000 ms)");
        // Decimals are kept.
        assert_inferred(
            &mr(None, timing(&["SE"], 612.5, 9.46)),
            MrContrast::T1,
            "(612.5 ms) and short echo time (9.46 ms)",
        );
    }

    #[test]
    fn inversion_timing_gives_flair_or_stir() {
        // TR 9000 / TE 120 / TI 2500 ms.
        let flair = Acquisition { inversion_time_ms: Some(2500.0), ..timing(&["SE", "IR"], 9000.0, 120.0) };
        let weighting = mr(None, flair);
        assert_inferred(
            &weighting,
            MrContrast::T2Flair,
            "long inversion time (2500 ms), which suppresses the signal of fluid",
        );
        assert!(weighting.basis.contains("long repetition time (9000 ms), long echo time (120 ms)"));
        assert_eq!(mr_headline(&weighting), "MR scan, likely T2 FLAIR (fluid suppressed)");

        // TR 4000 / TE 40 / TI 150 ms.
        let stir = Acquisition { inversion_time_ms: Some(150.0), ..timing(&["IR"], 4000.0, 40.0) };
        let weighting = mr(None, stir);
        assert_inferred(
            &weighting,
            MrContrast::Stir,
            "repetition time 4000 ms and a short inversion time (150 ms), which suppresses the signal of fat",
        );
        assert_eq!(mr_headline(&weighting), "MR scan, likely STIR (fat suppressed)");

        // An inversion time is enough to leave the plain spin-echo rules, with or without the IR code.
        let t1_flair = Acquisition { inversion_time_ms: Some(900.0), ..timing(&["SE"], 2500.0, 20.0) };
        assert_unknown(&mr(None, t1_flair), "An inversion time of 900 ms fits none of the rules");
        assert_unknown(
            &mr(None, timing(&["SE", "IR"], 2500.0, 20.0)),
            "An inversion pulse is recorded, but not its timing",
        );
        // A recorded inversion time of 0 means none.
        let none = Acquisition { inversion_time_ms: Some(0.0), ..timing(&["SE"], 2500.0, 20.0) };
        assert_inferred(&mr(None, none), MrContrast::ProtonDensity, "(2500 ms)");
        // Fluid suppression needs a long echo time to be T2 FLAIR.
        let short_te = Acquisition { inversion_time_ms: Some(2500.0), ..timing(&["IR"], 9000.0, 20.0) };
        assert_unknown(&mr(None, short_te), "An inversion time of 2500 ms");
        // A short inversion time with a short repetition time is not STIR (inversion-prepared T1 imaging).
        let prepared = Acquisition { inversion_time_ms: Some(250.0), ..timing(&["IR"], 800.0, 4.0) };
        assert_unknown(&mr(None, prepared), "An inversion time of 250 ms");
    }

    #[test]
    fn gradient_echo_is_only_classified_with_a_long_echo_and_a_small_flip_angle() {
        // Gradient echo, TE 20 ms, flip angle 20 degrees.
        let t2_star = Acquisition { flip_angle_deg: Some(20.0), ..timing(&["GR"], 600.0, 20.0) };
        let weighting = mr(None, t2_star);
        assert_inferred(&weighting, MrContrast::T2Star, "long echo time (20 ms) and a small flip angle (20 degrees)");
        assert_eq!(mr_headline(&weighting), "MR scan, likely T2*-weighted");

        let unknown = "For this gradient-echo sequence the recorded timing does not settle the weighting";
        // Time-of-flight angiography, spoiled 3D T1 imaging, balanced steady state: all short TE.
        assert_unknown(&mr(None, Acquisition { flip_angle_deg: Some(20.0), ..timing(&["GR"], 25.0, 3.6) }), unknown);
        assert_unknown(&mr(None, Acquisition { flip_angle_deg: Some(9.0), ..timing(&["GR"], 4.1, 1.9) }), unknown);
        // A long echo with a large flip angle, or without a recorded flip angle.
        assert_unknown(&mr(None, Acquisition { flip_angle_deg: Some(70.0), ..timing(&["GR"], 600.0, 20.0) }), unknown);
        assert_unknown(&mr(None, timing(&["GR"], 600.0, 20.0)), unknown);
        // The spin-echo rules never apply to gradient echo: TR 500 / TE 12 is not "T1".
        assert_unknown(&mr(None, timing(&["GR"], 500.0, 12.0)), unknown);

        // Inversion-prepared gradient echo, echo-planar gradient echo, mixed sequences.
        let other = "For this kind of sequence the recorded timing does not settle the weighting";
        let mprage = Acquisition { inversion_time_ms: Some(900.0), ..timing(&["GR", "IR"], 2300.0, 2.9) };
        assert_unknown(&mr(None, mprage), other);
        assert_unknown(&mr(None, timing(&["EP", "GR"], 2000.0, 30.0)), other);
        assert_unknown(&mr(None, timing(&["EP"], 2000.0, 30.0)), other);
        assert_unknown(&mr(None, timing(&["SE", "GR"], 4000.0, 100.0)), other);
        assert_unknown(&mr(None, timing(&["RM"], 4000.0, 100.0)), other);
    }

    #[test]
    fn contradictory_or_missing_values_cannot_be_told() {
        // Short TR with long TE: contradictory.
        assert_unknown(
            &mr(None, timing(&["SE"], 500.0, 100.0)),
            "Repetition time 500 ms and echo time 100 ms fit none",
        );
        // In between: mixed contrast, intermediate weighting.
        assert_unknown(&mr(None, timing(&["SE"], 1500.0, 12.0)), "Repetition time 1500 ms and echo time 12 ms");
        assert_unknown(&mr(None, timing(&["SE"], 3000.0, 50.0)), "Repetition time 3000 ms and echo time 50 ms");
        // Missing values.
        let no_te = Acquisition { echo_time_ms: None, ..timing(&["SE"], 500.0, 12.0) };
        assert_unknown(&mr(None, no_te), "No repetition time or echo time is recorded");
        let zero_tr = Acquisition { repetition_time_ms: Some(0.0), ..timing(&["SE"], 500.0, 12.0) };
        assert_unknown(&mr(None, zero_tr), "No repetition time or echo time is recorded");
        let no_tr =
            Acquisition { repetition_time_ms: None, inversion_time_ms: Some(150.0), ..timing(&["IR"], 0.0, 40.0) };
        assert_unknown(&mr(None, no_tr), "No repetition time or echo time is recorded");
        // The type of sequence is needed: the same numbers mean different things for gradient echo.
        assert_unknown(&mr(None, timing(&[], 500.0, 12.0)), "do not record which type of sequence");
        assert_unknown(&mr(None, Acquisition::default()), "do not record which type of sequence");
    }

    #[test]
    fn diffusion_and_adc_are_facts_not_likelihoods() {
        // b-value 1000 s/mm².
        let dwi = Acquisition { diffusion_b_value: Some(1000.0), ..timing(&["EP", "SE"], 5000.0, 90.0) };
        let weighting = mr(None, dwi.clone());
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::Diffusion), Evidence::Recorded));
        assert_eq!(weighting.basis, "Recorded in the images: diffusion b-value 1000 s/mm².");
        assert_eq!(mr_headline(&weighting), "MR scan, diffusion-weighted");
        assert!(mr_statement(&weighting).starts_with("Diffusion-weighted: "));
        // b = 0 is not diffusion weighting: the timing rules apply.
        let b0 = Acquisition { diffusion_b_value: Some(0.0), ..dwi.clone() };
        assert_inferred(&mr(None, b0), MrContrast::T2, "(5000 ms)");
        // The recorded b-value outranks a weighting in the name.
        assert_eq!(mr(Some("T2 TRA"), dwi.clone()).contrast, Some(MrContrast::Diffusion));

        // Image type.
        let typed = |values: &[&str]| Acquisition { image_type: codes(values), ..Acquisition::default() };
        let weighting = mr(None, typed(&["DERIVED", "PRIMARY", "DIFFUSION", "ADC"]));
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::AdcMap), Evidence::Recorded));
        assert_eq!(mr_headline(&weighting), "MR scan, ADC map (calculated)");
        let weighting = mr(None, typed(&["ORIGINAL", "PRIMARY", "DIFFUSION", "TRACEW"]));
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::Diffusion), Evidence::Recorded));
        // Other maps derived from diffusion images are not called diffusion-weighted.
        assert_eq!(mr(None, typed(&["DERIVED", "PRIMARY", "DIFFUSION", "FA"])).contrast, None);

        // Names. A map named ADC stays a map even where a b-value is recorded.
        let weighting = mr(Some("ep2d_diff_3scan_trace_ADC"), dwi);
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::AdcMap), Evidence::Named));
        for name in ["ep2d_diff_3scan_trace_p2", "DWI b1000", "AX DIFFUSION", "DTI 32 dir", "dwi_tra"] {
            let weighting = mr(Some(name), Acquisition::default());
            assert_eq!(
                (weighting.contrast, weighting.evidence),
                (Some(MrContrast::Diffusion), Evidence::Named),
                "{name}"
            );
        }
    }

    #[test]
    fn a_name_wins_over_inference_and_is_reported_as_named() {
        // "t1_mprage_sag", whose timing fits no rule.
        let mprage = Acquisition { inversion_time_ms: Some(900.0), ..timing(&["GR", "IR"], 2300.0, 2.9) };
        let weighting = mr(Some("t1_mprage_sag"), mprage);
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::T1), Evidence::Named));
        assert_eq!(weighting.basis, "Named by the scanner operator: the series description reads “t1_mprage_sag”.");
        assert_eq!(mr_headline(&weighting), "MR scan, T1-weighted");
        assert!(mr_statement(&weighting).starts_with("T1-weighted: "), "no \"likely\" for a named weighting");

        // The name wins even where the timing says otherwise.
        let weighting = mr(Some("T1 SE COR"), timing(&["SE"], 4000.0, 100.0));
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::T1), Evidence::Named));

        // The protocol name is used when the description names no weighting.
        let protocol = Acquisition { protocol_name: Some("t2_tse_tra_512".to_owned()), ..Acquisition::default() };
        let weighting = mr(Some("SAG 3mm"), protocol.clone());
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::T2), Evidence::Named));
        assert_eq!(weighting.basis, "Named by the scanner operator: the protocol name reads “t2_tse_tra_512”.");
        // The description comes first.
        assert_eq!(mr(Some("PD FS SAG"), protocol).contrast, Some(MrContrast::ProtonDensity));

        // Two weightings in one name (dual echo) name nothing: the timing decides, or nothing does.
        assert_inferred(&mr(Some("PD T2 AX"), timing(&["SE"], 2500.0, 20.0)), MrContrast::ProtonDensity, "(20 ms)");
        assert_unknown(&mr(Some("T1 T2 mapping"), Acquisition::default()), "type of sequence");
    }

    #[test]
    fn weighting_words_are_recognised_as_whole_words() {
        let named = |name: &str| named_contrast(name);
        for name in
            ["t1_mprage_sag", "T1W_TSE", "AX T1 +C", "T1WI SAG", "sT1W_3D_TFE", "T1FS COR", "cor t1ce", "T1-weighted"]
        {
            assert_eq!(named(name), Some(MrContrast::T1), "{name}");
        }
        for name in ["T2 TSE SAG (synthetic)", "t2_tse_sag_384", "T2W_TSE", "eT2W_SPIR", "AX T2 FS"] {
            assert_eq!(named(name), Some(MrContrast::T2), "{name}");
        }
        for name in ["PD FS SAG", "pd_tse_fs_cor", "PDW_SPAIR", "Proton Density AX", "PDFS"] {
            assert_eq!(named(name), Some(MrContrast::ProtonDensity), "{name}");
        }
        for name in ["T2*_GRE", "t2_star_ax", "T2STAR", "AX T2* GRE"] {
            assert_eq!(named(name), Some(MrContrast::T2Star), "{name}");
        }
        for name in
            ["AX T2 FLAIR", "t2_tirm_tra_dark-fluid", "FLAIR", "T2W_FLAIR", "T2FLAIR 3D", "t2_spc_darkfluid_sag"]
        {
            assert_eq!(named(name), Some(MrContrast::T2Flair), "{name}");
        }
        for name in ["AX T1 FLAIR", "T1FLAIR SAG"] {
            assert_eq!(named(name), Some(MrContrast::T1Flair), "{name}");
        }
        for name in ["STIR COR", "t2_stir_sag", "T2STIR"] {
            assert_eq!(named(name), Some(MrContrast::Stir), "{name}");
        }
        assert_eq!(named("ADC"), Some(MrContrast::AdcMap));
        // No weighting word: other measurements, substrings of longer words, plain numbers.
        for name in
            ["t1map", "T1rho", "T2 T1 dual", "SAG 3D", "localizer", "t10 series", "st1", "upd", "ct12w", "MRA TOF"]
        {
            assert_eq!(named(name), None, "{name}");
        }
        assert_eq!(words("T2*_GRE  ax/3mm (post)"), ["t2*", "gre", "ax", "3mm", "post"]);
    }

    #[test]
    fn ct_series_states_thickness_kernel_and_contrast_agent() {
        let acquisition = Acquisition {
            slice_thickness_mm: Some(1.25),
            convolution_kernel: Some("B30f".to_owned()),
            contrast_agent: Some("Iopromide".to_owned()),
            kvp: Some(120.0),
            ..Acquisition::default()
        };
        let ct = series("CT", Some("CHEST 1.25 B30f"), acquisition.clone());
        let guide = describe_series(&ct);
        assert_eq!((guide.kind, guide.headline.as_str()), (SeriesKind::Ct, "CT scan"));
        assert_eq!(series_line(&ct), Some("CT scan"));
        assert!(guide.weighting.is_none());
        let texts: Vec<&str> = guide.statements.iter().map(|s| s.text.as_str()).collect();
        assert!(texts[0].starts_with("A CT scan (computed tomography)"), "{texts:?}");
        assert!(texts.contains(&"Each image shows a slice of the body 1.25 mm thick."), "{texts:?}");
        assert!(texts.iter().any(|t| t.starts_with("Computed with the reconstruction kernel “B30f”, as recorded")));
        assert!(texts.contains(&"A contrast agent is recorded for this series: Iopromide."), "{texts:?}");
        assert_eq!(guide.statements[0].basis, "Recorded in the images: modality CT.");
        // Every statement carries its basis.
        assert!(guide.statements.iter().all(|s| !s.basis.trim().is_empty()));

        // Without an agent the guide says so, and that this rules nothing out.
        let plain = describe_series(&series("CT", None, Acquisition { contrast_agent: None, ..acquisition.clone() }));
        let last = plain.statements.last().expect("statements");
        assert!(last.text.starts_with("No contrast agent is recorded for this series, which does not rule one out"));
        assert_eq!(last.basis, "The contrast agent entry of the images is absent or empty.");
        let none = Acquisition { contrast_agent: Some("NONE".to_owned()), ..acquisition };
        let last = describe_series(&series("CT", None, none)).statements.pop().expect("statements");
        assert!(last.text.starts_with("No contrast agent is recorded"));
        assert_eq!(last.basis, "The contrast agent entry of the images reads “NONE”.");
        // Nothing recorded: only what is known is said.
        let bare = describe_series(&series("CT", None, Acquisition::default()));
        assert_eq!(bare.statements.len(), 2, "kind and contrast agent: {:?}", bare.statements);
    }

    #[test]
    fn mr_acquisition_type_says_slice_by_slice_or_block() {
        let statement = |value: Option<&str>| {
            let a = Acquisition { mr_acquisition_type: value.map(str::to_owned), ..Acquisition::default() };
            acquisition_type_statement(&a).map(|s| s.text)
        };
        assert_eq!(statement(Some("2D")).as_deref(), Some("The images were measured slice by slice (2D)."));
        assert_eq!(
            statement(Some("3d ")).as_deref(),
            Some("A whole block of the body was measured at once (3D) and then divided into the slices shown.")
        );
        for other in [None, Some(""), Some("4D"), Some("2D/3D")] {
            assert_eq!(statement(other), None, "{other:?}");
        }
        for value in ["2D", "3D"] {
            assert_plain(&statement(Some(value)).unwrap_or_default());
        }
        // Only for MR series, after the weighting.
        let mr = series(
            "MR",
            Some("t1_mprage_sag"),
            Acquisition { mr_acquisition_type: Some("3D".to_owned()), ..Acquisition::default() },
        );
        let texts: Vec<String> = describe_series(&mr).statements.into_iter().map(|s| s.text).collect();
        assert!(texts[1].starts_with("T1-weighted: ") && texts[2].contains("(3D)"), "{texts:?}");
        let ct =
            series("CT", None, Acquisition { mr_acquisition_type: Some("3D".to_owned()), ..Acquisition::default() });
        assert!(describe_series(&ct).statements.iter().all(|s| !s.text.contains("(3D)")));
    }

    #[test]
    fn localizers_and_stored_pictures_are_recognised() {
        // A localizer by its image type …
        let typed = Acquisition { image_type: codes(&["ORIGINAL", "PRIMARY", "LOCALIZER"]), ..Acquisition::default() };
        let localizer = series("CT", Some("Topo"), typed);
        assert_eq!(series_kind(&localizer), SeriesKind::Localizer);
        assert_eq!(series_line(&localizer), Some("Localizer (planning overview)"));
        let guide = describe_series(&localizer);
        assert_eq!(guide.statements[0].basis, "Recorded in the images: image type LOCALIZER.");
        assert!(guide.weighting.is_none(), "a localizer gets no weighting");
        // The localizer of a CT is a projection: nothing is said about slices, even with an orientation.
        let front = ImagePlane::from_dicom([0.0; 3], [1.0, 0.0, 0.0, 0.0, 0.0, -1.0], [1.0, 1.0], 4, 4);
        let projection = Series { instances: vec![instance(CT_IMAGE, front)], ..localizer.clone() };
        assert_eq!(describe_series(&projection).statements.len(), 1);
        let slices = Series { modality: "MR".to_owned(), ..projection };
        assert_eq!(
            describe_series(&slices).statements[1].text,
            "The slices are coronal: they divide the body into front and back, like a view from the front."
        );
        // … or by its name.
        for name in ["3-Plane Loc SSFSE", "localizer", "Scout", "SURVEY", "Topogram 0.6 T20f", "AX LOC"] {
            let named = series("MR", Some(name), timing(&["GR"], 8.0, 4.0));
            assert_eq!(series_kind(&named), SeriesKind::Localizer, "{name}");
        }
        let named = describe_series(&series("MR", Some("AX LOC"), Acquisition::default()));
        assert_eq!(named.statements[0].basis, "Named by the scanner operator: the series description reads “AX LOC”.");
        // Whole words only.
        assert_eq!(series_kind(&series("MR", Some("T2 LOCATION 3"), Acquisition::default())), SeriesKind::Mr);

        // Secondary captures: a dose report, or any other stored picture.
        let stored = |modality: &str, description: &str| Series {
            instances: vec![instance(SECONDARY_CAPTURE, None)],
            ..series(modality, Some(description), Acquisition::default())
        };
        let dose = stored("OT", "Dose Report");
        assert_eq!(series_kind(&dose), SeriesKind::DoseReport);
        assert_eq!(series_line(&dose), Some("Dose report (a stored picture)"));
        let guide = describe_series(&dose);
        assert!(guide.statements[0].text.contains("It is a stored picture, not a scan of the body."));
        assert_eq!(
            guide.statements[0].basis,
            "The files are stored pictures and the series description reads “Dose Report”."
        );
        assert_eq!(guide.statements.len(), 1, "no plane, thickness or contrast agent for a picture");
        assert_eq!(series_kind(&stored("CT", "Patient Protocol")), SeriesKind::DoseReport);
        let screenshot = stored("CT", "Screen Save");
        assert_eq!(series_kind(&screenshot), SeriesKind::SecondaryCapture);
        assert_eq!(series_line(&screenshot), Some("Stored picture (secondary capture)"));
        assert_eq!(
            describe_series(&screenshot).statements[0].basis,
            "Recorded in the images: the files are secondary captures."
        );

        // A scan whose name mentions the dose is a scan.
        let low_dose = Series {
            instances: vec![instance(CT_IMAGE, None)],
            ..series("CT", Some("LOW DOSE CHEST"), Acquisition::default())
        };
        assert_eq!(series_kind(&low_dose), SeriesKind::Ct);
        // A dose page stored as an image of the scanner's own modality.
        let page = Acquisition { image_type: codes(&["DERIVED", "SECONDARY", "OTHER"]), ..Acquisition::default() };
        let protocol =
            Series { instances: vec![instance(CT_IMAGE, None)], ..series("CT", Some("Patient Protocol"), page) };
        assert_eq!(series_kind(&protocol), SeriesKind::DoseReport);
    }

    #[test]
    fn other_modalities_are_named_and_unknown_ones_are_not_guessed() {
        let kinds = [
            ("PT", SeriesKind::Pet, "PET scan"),
            ("NM", SeriesKind::NuclearMedicine, "Nuclear medicine image"),
            ("US", SeriesKind::Ultrasound, "Ultrasound image"),
            ("CR", SeriesKind::Xray, "X-ray image"),
            ("DX", SeriesKind::Xray, "X-ray image"),
            ("MG", SeriesKind::Mammogram, "Mammogram (X-ray image)"),
            ("XA", SeriesKind::Fluoroscopy, "X-ray fluoroscopy or angiography"),
            ("RF", SeriesKind::Fluoroscopy, "X-ray fluoroscopy or angiography"),
        ];
        for (modality, kind, line) in kinds {
            let s = series(modality, Some("Series"), Acquisition::default());
            assert_eq!((series_kind(&s), series_line(&s)), (kind, Some(line)), "{modality}");
            let guide = describe_series(&s);
            assert_eq!(guide.headline, line);
            assert_eq!(guide.statements[0].basis, format!("Recorded in the images: modality {modality}."));
        }
        // Not recognised: no line in the browser, and the card says so.
        for modality in ["OT", "SR", "XYZ", ""] {
            let s = series(modality, Some("Something"), Acquisition::default());
            assert_eq!((series_kind(&s), series_line(&s)), (SeriesKind::Unknown, None), "{modality}");
            let guide = describe_series(&s);
            assert_eq!(guide.headline, "Unrecognised kind of images");
            assert_eq!(guide.statements[0].text, "This guide does not recognise what kind of images these are.");
            assert_eq!(guide.statements.len(), 1);
        }
    }

    #[test]
    fn synthetic_series_are_described_sensibly() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let study = &index.studies[0];
        let texts = |guide: &SeriesGuide| guide.statements.iter().map(|s| s.text.clone()).collect::<Vec<_>>();

        // CT chest, axial.
        let ct = describe_series(&study.series[0]);
        assert_eq!((ct.kind, ct.headline.as_str()), (SeriesKind::Ct, "CT scan"));
        let t = texts(&ct);
        assert!(t[1].starts_with("The slices are axial: horizontal cross-sections"), "{t:?}");
        assert!(t.contains(&"Each image shows a slice of the body 8.33 mm thick.".to_owned()), "{t:?}");
        assert!(t.iter().any(|s| s.starts_with("Computed with the reconstruction kernel “STANDARD”")), "{t:?}");
        assert!(
            t.contains(&"A contrast agent is recorded for this series: Iodinated contrast (synthetic).".to_owned())
        );

        // MR, sagittal, named T2 (its timing, TR 4000 / TE 96 ms, would say the same).
        let mr = describe_series(&study.series[1]);
        assert_eq!((mr.kind, mr.headline.as_str()), (SeriesKind::Mr, "MR scan, T2-weighted"));
        let weighting = mr.weighting.as_ref().expect("MR weighting");
        assert_eq!((weighting.contrast, weighting.evidence), (Some(MrContrast::T2), Evidence::Named));
        let t = texts(&mr);
        assert!(t[1].starts_with("The slices are sagittal: they divide the body into left and right"), "{t:?}");
        assert_eq!(t[2], "T2-weighted: on such images fluid tends to appear bright.");
        assert_eq!(
            mr.statements[2].basis,
            "Named by the scanner operator: the series description reads “T2 TSE SAG (synthetic)”."
        );
        assert_eq!(t[3], "The images were measured slice by slice (2D).");
        assert_eq!(mr.statements[3].basis, "Recorded in the images: MR acquisition type.");
        assert!(t.contains(&"Each image shows a slice of the body 4 mm thick.".to_owned()), "{t:?}");
        assert!(t.last().is_some_and(|s| s.starts_with("No contrast agent is recorded")), "{t:?}");
        // Without the name, the timing gives the same weighting, worded as likely.
        let unnamed = Series { description: None, ..study.series[1].clone() };
        let unnamed = Series { acquisition: Acquisition { protocol_name: None, ..unnamed.acquisition }, ..unnamed };
        assert_inferred(
            &classify_mr(&unnamed),
            MrContrast::T2,
            "long repetition time (4000 ms) and long echo time (96 ms)",
        );

        // Three-plane localizer.
        let localizer = describe_series(&study.series[2]);
        assert_eq!(
            (localizer.kind, localizer.headline.as_str()),
            (SeriesKind::Localizer, "Localizer (planning overview)")
        );
        assert_eq!(texts(&localizer)[1], "The images of this series lie in different directions.");

        // Colour secondary capture named "Dose report".
        let dose = describe_series(&study.series[3]);
        assert_eq!((dose.kind, dose.headline.as_str()), (SeriesKind::DoseReport, "Dose report (a stored picture)"));

        for (series, guide) in study.series.iter().zip([&ct, &mr, &localizer, &dose]) {
            assert_eq!(series_line(series), Some(guide.headline.as_str()));
            for statement in &guide.statements {
                assert_plain(&statement.text);
                assert_eq!(sentences(&statement.basis), 1, "{}", statement.basis);
                // Privacy: nothing that identifies the patient, with or without privacy mode.
                for text in guide.texts() {
                    for identifier in ["PHANTOM", "SYNTHETIC", PATIENT_ID, "1970", "DOE", "JANE", "Klinikum"] {
                        assert!(!text.contains(identifier), "{identifier:?} in {text:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn viewing_direction_follows_the_screen_axes() {
        let (x, y, z) = (DVec3::X, DVec3::Y, DVec3::Z);
        let say = |right: DVec3, down: DVec3| {
            let d = viewing_direction(right, down).expect("direction");
            (d.title, d.looking, d.screen_left, d.approximate)
        };
        // Axial, Image Orientation 1\0\0\0\1\0.
        assert_eq!(
            say(x, y),
            (
                "Axial image",
                "You are looking from the feet towards the head.",
                "The patient's right side is on the left of the screen.",
                false
            )
        );
        // Coronal, 1\0\0\0\0\-1.
        assert_eq!(
            say(x, -z),
            (
                "Coronal image",
                "You are looking from the front of the body towards the back.",
                "The patient's right side is on the left of the screen.",
                false
            )
        );
        // Sagittal, 0\1\0\0\0\-1.
        assert_eq!(
            say(y, -z),
            (
                "Sagittal image",
                "You are looking from the patient's left side towards the right.",
                "The front of the body is on the left of the screen.",
                false
            )
        );
        // Images are displayed as stored: a mirrored axial image is seen from the head.
        assert_eq!(
            say(-x, y),
            (
                "Axial image",
                "You are looking from the head towards the feet.",
                "The patient's left side is on the left of the screen.",
                false
            )
        );
        // The remaining directions.
        assert_eq!(say(-x, -z).1, "You are looking from the back of the body towards the front.");
        assert_eq!(say(-y, -z).1, "You are looking from the patient's right side towards the left.");
        assert_eq!(say(-y, -z).2, "The back of the body is on the left of the screen.");
        assert_eq!(say(z, y).2, "The feet end is on the left of the screen.");
        assert_eq!(say(-z, y).2, "The head end is on the left of the screen.");

        // An oblique plane, halfway between axial and coronal: nothing is stated as exact.
        let tilted = DVec3::new(0.0, 1.0, -1.0).normalize();
        assert_eq!(
            say(x, tilted),
            (
                "Tilted (oblique) image",
                "The image is tilted: you are looking roughly from the feet towards the head.",
                "The patient's right side is on the left of the screen.",
                true
            )
        );
        // A gantry tilt of 20 degrees is still called axial, but not exact.
        let (sin, cos) = 20f64.to_radians().sin_cos();
        let d = viewing_direction(x, DVec3::new(0.0, cos, sin)).expect("direction");
        assert_eq!((d.plane, d.title, d.approximate), (AnatomicalPlane::Axial, "Axial image", true));
        assert_eq!(d.looking, "The image is tilted: you are looking roughly from the feet towards the head.");
        // Five degrees are within what is called exact.
        let (sin, cos) = 5f64.to_radians().sin_cos();
        assert!(!viewing_direction(x, DVec3::new(0.0, cos, sin)).expect("direction").approximate);
        // An image rotated in its plane by 30 degrees: the left side is only roughly the patient's right.
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let d = viewing_direction(DVec3::new(cos, sin, 0.0), DVec3::new(-sin, cos, 0.0)).expect("direction");
        assert_eq!(d.looking, "You are looking from the feet towards the head.");
        assert_eq!(d.screen_left, "Roughly, the patient's right side is on the left of the screen.");
        assert!(d.approximate);
        // No direction, no sentence.
        assert_eq!(viewing_direction(DVec3::ZERO, y), None);
        assert_eq!(viewing_direction(x, x), None);
    }

    #[test]
    fn body_parts_map_to_regions_and_unknown_values_to_none() {
        use BodyRegion::*;
        let known = [
            ("HEAD", Head),
            ("BRAIN", Head),
            ("SKULL", Head),
            ("NECK", Neck),
            ("CHEST", Chest),
            ("THORAX", Chest),
            ("HEART", Chest),
            ("ABDOMEN", Abdomen),
            ("PELVIS", Pelvis),
            ("ABDOMENPELVIS", AbdomenPelvis),
            ("CHESTABDPELVIS", ChestAbdomenPelvis),
            ("CSPINE", CervicalSpine),
            ("TSPINE", ThoracicSpine),
            ("LSPINE", LumbarSpine),
            ("SPINE", WholeSpine),
            ("SHOULDER", Shoulder),
            ("ELBOW", Elbow),
            ("WRIST", Wrist),
            ("HAND", Hand),
            ("HIP", Hip),
            ("KNEE", Knee),
            ("ANKLE", Ankle),
            ("FOOT", Foot),
            ("LEG", WholeLeg),
            ("ARM", WholeArm),
            ("WHOLEBODY", WholeBody),
            // Spelling variants and anatomic region names of Enhanced objects.
            ("L SPINE", LumbarSpine),
            ("Lumbar spine", LumbarSpine),
            ("c-spine", CervicalSpine),
            ("Whole body", WholeBody),
            (" chest ", Chest),
            ("Upper arm", UpperArm),
            ("LOWER_LEG", LowerLeg),
        ];
        for (value, region) in known {
            assert_eq!(body_region(value), Some(region), "{value}");
        }
        // Not recognised: the figure falls back to the scanned block, it never guesses.
        for value in ["", "UNKNOWN", "EXTREMITY", "BODY", "CERVICAL", "PHANTOM", "CHEST OR ABDOMEN", "123"] {
            assert_eq!(body_region(value), None, "{value}");
        }
    }

    #[test]
    fn every_parameter_explanation_is_short_and_plain() {
        for key in EXPLAINED_PARAMETERS {
            let text = explain_parameter(key).unwrap_or_else(|| panic!("no explanation for {key}"));
            assert_plain(text);
        }
        let unique: std::collections::HashSet<_> = EXPLAINED_PARAMETERS.iter().collect();
        assert_eq!(unique.len(), EXPLAINED_PARAMETERS.len(), "a key is listed twice");
        // Units are explained where the row shows one.
        assert!(
            explain_parameter("RepetitionTime")
                .is_some_and(|t| t.contains("milliseconds (ms, thousandths of a second)"))
        );
        assert!(explain_parameter("KVP").is_some_and(|t| t.contains("kilovolts (kV, thousands of volts)")));
        assert!(explain_parameter("SliceThickness").is_some_and(|t| t.contains("millimetres (mm)")));
        assert!(explain_parameter("MagneticFieldStrength").is_some_and(|t| t.contains("tesla (T)")));
        // Rows about the patient are never explained, and unknown keys have no explanation.
        for key in
            ["PatientName", "PatientID", "PatientBirthDate", "PatientAge", "PatientSex", "PatientWeight", "PatientSize"]
        {
            assert_eq!(explain_parameter(key), None, "{key}");
        }
        for key in ["", "Repetition time (TR)", "repetitiontime", "StudyDescription", "Manufacturer"] {
            assert_eq!(explain_parameter(key), None, "{key}");
        }
    }

    #[test]
    fn every_statement_about_a_kind_a_weighting_or_a_plane_is_plain() {
        use SeriesKind::*;
        for kind in [
            Ct,
            Mr,
            Localizer,
            DoseReport,
            SecondaryCapture,
            Pet,
            NuclearMedicine,
            Ultrasound,
            Xray,
            Mammogram,
            Fluoroscopy,
            Unknown,
        ] {
            assert_plain(kind_statement(kind));
            assert_no_interpretation(kind_headline(kind));
        }

        use MrContrast::*;
        let contrasts = [T1, T2, ProtonDensity, T2Star, T2Flair, T1Flair, Stir, Diffusion, AdcMap];
        for contrast in contrasts.map(Some).into_iter().chain([None]) {
            for evidence in [Evidence::Named, Evidence::Recorded, Evidence::Inferred, Evidence::Unknown] {
                let weighting = MrWeighting { contrast, evidence, basis: String::new() };
                let (statement, headline) = (mr_statement(&weighting), mr_headline(&weighting));
                assert_plain(statement);
                assert_eq!(sentences(statement), 1, "{statement}");
                assert_no_interpretation(headline);
                // "Likely" exactly where a weighting is inferred (three contrasts are never inferred).
                let inferred =
                    evidence == Evidence::Inferred && !matches!(contrast, None | Some(T1Flair | Diffusion | AdcMap));
                assert_eq!(statement.starts_with("Likely "), inferred, "{statement}");
                assert_eq!(headline.contains("likely"), inferred, "{headline}");
            }
        }

        // The plane of the slices, from the orientation of every image.
        let plane = |orientation: [f64; 6]| ImagePlane::from_dicom([0.0; 3], orientation, [1.0, 1.0], 4, 4);
        let with = |planes: Vec<Option<ImagePlane>>| Series {
            instances: planes.into_iter().map(|plane| instance(CT_IMAGE, plane)).collect(),
            ..series("CT", None, Acquisition::default())
        };
        let (axial, coronal, sagittal) =
            ([1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0, 0.0, -1.0], [0.0, 1.0, 0.0, 0.0, 0.0, -1.0]);
        let half = 0.5f64.sqrt();
        let cases = [
            (vec![plane(axial), plane(axial)], Some("The slices are axial: ")),
            (vec![plane(coronal)], Some("The slices are coronal: ")),
            (vec![plane(sagittal)], Some("The slices are sagittal: ")),
            (vec![plane([1.0, 0.0, 0.0, 0.0, half, -half])], Some("The slices are tilted (oblique): ")),
            (vec![plane(axial), plane(coronal)], Some("The images of this series lie in different directions.")),
            // No position: nothing is said about a plane.
            (vec![None], None),
            (Vec::new(), None),
        ];
        for (planes, expected) in cases {
            let statement = plane_statement(&with(planes));
            assert_eq!(statement.is_some(), expected.is_some(), "{statement:?}");
            if let (Some(statement), Some(expected)) = (statement, expected) {
                assert!(statement.starts_with(expected), "{statement}");
                assert_plain(statement);
                assert_eq!(sentences(statement), 1);
            }
        }
    }

    /// End to end: the keys of the overview rows of the synthetic images
    /// lead to explanations, and no row about the patient does.
    #[test]
    fn overview_rows_of_the_synthetic_images_are_explained() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let explained = |series: usize| -> Vec<String> {
            let path = &index.studies[0].series[series].instances[0].path;
            let metadata = summrise_dicom::read_metadata(&summrise_dicom::Media::folder(), path).expect("metadata");
            let patient = metadata.summary.iter().find(|s| s.title == "Patient").expect("patient section");
            assert_eq!(patient.rows.len(), 5, "name, ID, birth date, age and sex");
            assert!(patient.rows.iter().all(|row| row.key.and_then(explain_parameter).is_none()));
            metadata
                .summary
                .iter()
                .flat_map(|section| &section.rows)
                .filter(|row| row.key.and_then(explain_parameter).is_some())
                .map(|row| row.label.clone())
                .collect()
        };
        let ct = explained(0);
        for label in [
            "Tube voltage",
            "Tube current",
            "Exposure",
            "Kernel",
            "Contrast agent",
            "Body part",
            "Patient position",
            "Slice thickness",
            "Pixel spacing",
            "Matrix",
            "Field of view",
            "Orientation",
            "Rescale",
            "Stored windows",
            "Transfer syntax",
            "Study UID",
        ] {
            assert!(ct.iter().any(|l| l == label), "CT row {label:?} has no explanation: {ct:?}");
        }
        let mr = explained(1);
        for label in [
            "Repetition time (TR)",
            "Echo time (TE)",
            "Flip angle",
            "Echo train length",
            "Imaging frequency",
            "Sequence name",
            "Scanning sequence",
            "Sequence variant",
            "Acquisition type",
            "Field strength",
            "Protocol",
            "Spacing between slices",
        ] {
            assert!(mr.iter().any(|l| l == label), "MR row {label:?} has no explanation: {mr:?}");
        }
    }

    #[test]
    fn every_preset_says_what_the_window_is_for() {
        let mut purposes = std::collections::HashSet::new();
        for preset in CT_PRESETS {
            let explanation = explain_preset(preset, Some("HU"));
            assert_ne!(explanation.purpose, "A standard CT window.", "{} has no explanation of its own", preset.name);
            assert!(purposes.insert(explanation.purpose.clone()), "{} shares an explanation", preset.name);
            assert_plain(&explanation.purpose);
            assert_plain(&explanation.range);
            assert_eq!((sentences(&explanation.purpose), sentences(&explanation.range)), (1, 1));
        }
        // The lung window, W 1500 / L -600 HU, is recognised by its values, whatever it is called.
        let lung =
            WindowPreset { name: "Poumon".into(), window: Window::new(-600.0, 1500.0), source: PresetSource::Clinical };
        let explanation = explain_preset(&lung, Some("HU"));
        assert!(explanation.purpose.contains("the air-filled lungs"), "{}", explanation.purpose);
        assert_eq!(
            explanation.range,
            "It spreads the grey scale over densities from -1350 to 150 HU (Hounsfield units: air is -1000, water 0); everything outside that range is shown as black or white."
        );
        // A built-in window with other values gets the general sentence, not a wrong one.
        let other = WindowPreset { window: Window::new(10.0, 123.0), ..lung.clone() };
        assert_eq!(explain_preset(&other, Some("HU")).purpose, "A standard CT window.");

        // Stored, automatic and full-range windows; values without a unit (MR).
        let stored = WindowPreset { name: "Lung".into(), source: PresetSource::Dicom, ..lung.clone() };
        assert!(explain_preset(&stored, Some("HU")).purpose.starts_with("A window stored with the images"));
        let mr =
            WindowPreset { name: "Window 1".into(), window: Window::new(700.0, 1400.0), source: PresetSource::Dicom };
        assert_eq!(
            explain_preset(&mr, None).range,
            "It spreads the grey scale over values from 0 to 1400; everything outside that range is shown as black or white."
        );
        let auto = WindowPreset { name: "Auto".into(), source: PresetSource::Auto, ..mr.clone() };
        assert!(explain_preset(&auto, None).purpose.starts_with("A window computed from the brightness values"));
        let full = WindowPreset { name: "Full range".into(), source: PresetSource::FullRange, ..mr.clone() };
        assert!(explain_preset(&full, Some("SUV")).purpose.contains("nothing is clipped"));
        assert!(explain_preset(&full, Some("SUV")).range.contains("values from 0 to 1400 SUV;"));
        for preset in [&stored, &mr, &auto, &full] {
            assert_plain(&explain_preset(preset, None).purpose);
            assert_plain(&explain_preset(preset, None).range);
        }
    }

    /// The uptake windows of a PET series in SUV and the rows of the
    /// inspector's uptake section are explained, plainly; the weight, a row
    /// about the patient, is not.
    #[test]
    fn uptake_windows_and_the_rows_of_the_uptake_section_are_explained() {
        let suv5 = WindowPreset {
            name: "SUV 0–5".into(),
            window: Window::from_range(0.0, 5.0),
            source: PresetSource::Clinical,
        };
        let explanation = explain_preset(&suv5, Some("SUV"));
        assert!(explanation.purpose.starts_with("A built-in window for uptake values (SUV) from SUV 0"));
        assert_eq!(
            explanation.range,
            "It spreads the grey scale over values from 0 to 5 SUV; everything outside that range is shown as black or white."
        );
        assert_plain(&explanation.purpose);
        assert_plain(&explanation.range);
        // Uptake values keep their decimals; the same window on a CT is not an uptake window.
        let auto =
            WindowPreset { name: "Auto".into(), window: Window::from_range(0.12, 4.86), source: PresetSource::Auto };
        assert!(explain_preset(&auto, Some("SUV")).range.contains("from 0.12 to 4.86 SUV;"));
        assert_eq!(explain_preset(&suv5, Some("HU")).purpose, "A standard CT window.");

        // SUV in two sentences: what it is, and that it rests on what staff entered.
        let suv = explain_parameter("SUVbw").expect("explained");
        assert_eq!(sentences(suv), 2);
        assert!(suv.contains("spread evenly through the whole body") && suv.contains("staff entered at the scanner"));

        // End to end: every keyed row of the section of the synthetic PET, but the weight.
        let dir = tempfile::tempdir().expect("tempdir");
        summrise_testkit::pet::write_pet_study(dir.path(), &Default::default()).expect("fixture");
        let index = summrise_dicom::scan(dir.path(), &summrise_dicom::Progress::new()).expect("scan");
        let meta = &index.studies[0].series[summrise_testkit::pet::PET];
        let data = crate::series::testing::load(meta);
        let section = crate::pet::uptake_section(meta, &data, false).expect("section");
        for row in section.rows.iter().filter(|row| row.key.is_some()) {
            let explanation = row.key.and_then(explain_parameter);
            assert_eq!(explanation.is_none(), row.key == Some("PatientWeight"), "{}", row.label);
            explanation.into_iter().for_each(assert_plain);
            assert_no_interpretation(&row.value);
        }
    }

    #[test]
    fn fixed_texts_and_direction_sentences_are_plain() {
        assert_plain(disclaimer());
        assert!(disclaimer().contains("not an interpretation of the images"));
        assert_plain(sampling_note());
        let axes = [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y, DVec3::Z, -DVec3::Z];
        // Far enough off the axes (more than 10 degrees) for the "roughly" sentences.
        let tilt = DVec3::new(0.3, 0.25, 0.2);
        let mut seen = std::collections::HashSet::new();
        for right in axes {
            for down in axes {
                for (right, down) in [(right, down), (right + tilt, down - tilt), (right - tilt, down + tilt)] {
                    let Some(direction) = viewing_direction(right, down) else { continue };
                    assert_plain(direction.looking);
                    assert_plain(direction.screen_left);
                    seen.extend([direction.looking, direction.screen_left]);
                }
            }
        }
        assert_eq!(seen.len(), 24, "six directions and six sides, exact and rough: {seen:?}");
    }

    #[test]
    fn scanned_block_and_placement_come_from_the_headers() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let study = &index.studies[0];

        // CT: 48 x 48 pixels of 5 mm, 24 slices of 8.33 mm from z = -95.83 to +95.83 mm.
        let ct = &study.series[0];
        let block = scanned_block(ct).expect("CT block");
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(block.min.x, -117.5) && close(block.max.x, 117.5), "{block:?}");
        assert!(close(block.min.y, -117.5) && close(block.max.y, 117.5), "{block:?}");
        assert!(close(block.min.z, -100.0 + 25.0 / 6.0) && close(block.max.z, 100.0 - 25.0 / 6.0), "{block:?}");

        // The most superior image (instance 1) lies at the head end of the block, edge-on from the front.
        let top = ct.instances[0].frame_planes[0].expect("plane");
        let corners = |p: &ImagePlane| {
            let across = p.row_dir * (f64::from(p.cols - 1) * p.col_spacing);
            let down = p.col_dir * (f64::from(p.rows - 1) * p.row_spacing);
            [p.position, p.position + across, p.position + across + down, p.position + down]
        };
        let plane = ScreenPlane { right: top.row_dir, down: top.col_dir, corners: corners(&top) };
        let placement = place(&block, &plane);
        assert_eq!(placement.corners, [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]);
        assert!(close(placement.size_mm[0], 235.0) && close(placement.size_mm[2], 200.0 - 25.0 / 3.0));
        // An image three quarters of the way up.
        let mut higher = top;
        higher.position.z = block.min.z + 0.75 * block.size().z;
        let placement = place(&block, &ScreenPlane { corners: corners(&higher), ..plane });
        assert!(placement.corners.iter().all(|c| close(c[2], 0.75)), "{placement:?}");

        // MR: sagittal slices from x = +60 mm (patient left) to -60 mm; the first lies at the left end.
        let mr = &study.series[1];
        let block = scanned_block(mr).expect("MR block");
        assert!(close(block.min.x, -60.0) && close(block.max.x, 60.0), "{block:?}");
        let first = mr.instances[0].frame_planes[0].expect("plane");
        let placement =
            place(&block, &ScreenPlane { right: first.row_dir, down: first.col_dir, corners: corners(&first) });
        assert!(placement.corners.iter().all(|c| close(c[0], 1.0)), "patient left is 1: {placement:?}");
        // Screen top is the head end, screen left the front of the body.
        assert_eq!((placement.corners[0][1], placement.corners[0][2]), (0.0, 1.0));

        // The three planes of the localizer span a block; the stored picture has no position.
        let block = scanned_block(&study.series[2]).expect("localizer block");
        assert!(block.size().min_element() > 250.0, "{block:?}");
        assert_eq!(scanned_block(&study.series[3]), None);

        // A single image: no extent across the slice, so the fraction is one half.
        let single = Series { instances: vec![ct.instances[0].clone()], ..ct.clone() };
        let block = scanned_block(&single).expect("block");
        let placement = place(&block, &plane);
        assert!(placement.corners.iter().all(|c| c[2] == 0.5), "{placement:?}");
        assert_eq!(placement.size_mm[2], 0.0);
        // Corners outside the block are clamped into it.
        let outside = ScreenPlane { corners: plane.corners.map(|c| c + DVec3::new(1000.0, 0.0, 0.0)), ..plane };
        assert!(place(&block, &outside).corners.iter().all(|c| c[0] == 1.0));
    }

    /// display-fidelity: the explanations of contrast tables and burned-in text are plain.
    #[test]
    fn contrast_table_and_burned_in_text_are_explained_plainly() {
        assert_no_interpretation(explain_contrast_table());
        assert_eq!(sentences(explain_contrast_table()), 2);
        assert!(explain_parameter("BurnedInAnnotation").is_some_and(|t| t.contains("cannot be hidden")));
    }
}
