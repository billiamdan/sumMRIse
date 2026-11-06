//! PET uptake values: the header values a standardised uptake value needs,
//! the SUVbw calculation, and the one place where decoded values are scaled.
//!
//! [`suv_bw`] follows the vendor-neutral calculation published by QIBA
//! (UPICT/QIBA FDG-PET, "SUV calculation"), happy path only:
//!
//! 1. Units (0054,1001) `BQML`; Corrected Image (0028,0051) with `ATTN` and
//!    `DECY`; Decay Correction (0054,1102) `START`. Units `GML` (values that
//!    are SUV already) pass through with the factor 1 unless SUV Type
//!    (0054,1006) names another kind than body weight.
//! 2. Reference time: Series Date and Time. No image of the series may have
//!    been acquired before it (else the series time was rewritten later).
//! 3. Injection time: Radiopharmaceutical Start DateTime, else Start Time on
//!    the series date. The decay time (reference − injection) must not be
//!    negative, nor longer than ten half-lives.
//! 4. Decayed dose = Radionuclide Total Dose × 2^(−decay time / Radionuclide
//!    Half Life); factor = Patient Weight [kg] × 1000 / decayed dose [Bq];
//!    SUVbw = activity concentration [Bq/ml] × factor.
//!
//! A missing input is never guessed: each refusal is a [`SuvRefusal`]. The
//! factor is one number per series, applied by [`SuvScale::apply`] where the
//! decoded images enter the series, so that everything downstream (windows,
//! read-out, statistics, reformats) is in SUV. SUV is approximate: it rests on
//! the weight, dose and times entered by staff and on the scanner's
//! calibration.
//!
//! [`PetHeader`] holds the patient's weight, which is patient data: it is
//! never logged and is hidden in privacy mode wherever it is shown.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use summrise_i18n::tr;
use summrise_imaging::{FramePixels, Padding, VoxelBuffer, Window};

use crate::attrs;
use crate::decode::DecodedInstance;
use crate::index::Series;

/// The unit of values that are standardised uptake values.
pub const SUV_UNIT: &str = "SUV";

/// Most half-lives between injection and series time for which a decay
/// correction is plausible (less than 0.1 % of the dose is left after ten).
const MAX_HALF_LIVES: f64 = 10.0;

/// What the headers of a PET series record for its uptake values, from the
/// first file indexed; the acquisition time is the earliest of all files.
/// Absent, empty and malformed values are `None` or empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PetHeader {
    /// Units (0054,1001), e.g. `BQML`.
    pub units: Option<String>,
    /// Corrected Image (0028,0051) codes, e.g. `ATTN`, `DECY`.
    pub corrected_image: Vec<String>,
    /// Decay Correction (0054,1102): `START`, `ADMIN` or `NONE`.
    pub decay_correction: Option<String>,
    /// SUV Type (0054,1006) of values stored as uptake values.
    pub suv_type: Option<String>,
    /// Patient Weight (0010,1030) in kg — patient data: never logged, hidden
    /// in privacy mode.
    pub patient_weight_kg: Option<f64>,
    /// Series Date (0008,0021) as recorded.
    pub series_date: Option<String>,
    /// Series Time (0008,0031) as recorded.
    pub series_time: Option<String>,
    /// The earliest acquisition (Acquisition DateTime, or Acquisition Date
    /// and Time) of the files of the series, in seconds since 1970-01-01
    /// (scanner time).
    pub earliest_acquisition_s: Option<f64>,
    /// From the first item of the Radiopharmaceutical Information Sequence
    /// (0054,0016): Radiopharmaceutical (0018,0031), the tracer as entered.
    pub radiopharmaceutical: Option<String>,
    /// The code meaning of the Radionuclide Code Sequence (0054,0300).
    pub radionuclide: Option<String>,
    /// Radionuclide Total Dose (0018,1074) in Bq.
    pub injected_dose_bq: Option<f64>,
    /// Radionuclide Half Life (0018,1075) in s.
    pub half_life_s: Option<f64>,
    /// Radiopharmaceutical Start Time (0018,1072) as recorded.
    pub injection_time: Option<String>,
    /// Radiopharmaceutical Start DateTime (0018,1078) as recorded.
    pub injection_datetime: Option<String>,
}

impl PetHeader {
    /// Reads the values from the data set of one file.
    pub fn read(obj: &InMemDicomObject) -> Self {
        let drug = attrs::first_item(obj, tags::RADIOPHARMACEUTICAL_INFORMATION_SEQUENCE);
        let text = |tag| drug.and_then(|item| attrs::text(item, tag));
        let number = |tag| drug.and_then(|item| attrs::float(item, tag));
        Self {
            units: attrs::text(obj, tags::UNITS),
            corrected_image: attrs::texts(obj, tags::CORRECTED_IMAGE).into_iter().filter(|c| !c.is_empty()).collect(),
            decay_correction: attrs::text(obj, tags::DECAY_CORRECTION),
            suv_type: attrs::text(obj, tags::SUV_TYPE),
            patient_weight_kg: attrs::float(obj, tags::PATIENT_WEIGHT),
            series_date: attrs::text(obj, tags::SERIES_DATE),
            series_time: attrs::text(obj, tags::SERIES_TIME),
            earliest_acquisition_s: acquisition_seconds(obj),
            radiopharmaceutical: text(tags::RADIOPHARMACEUTICAL),
            radionuclide: drug
                .and_then(|item| attrs::first_item(item, tags::RADIONUCLIDE_CODE_SEQUENCE))
                .and_then(|code| attrs::text(code, tags::CODE_MEANING)),
            injected_dose_bq: number(tags::RADIONUCLIDE_TOTAL_DOSE),
            half_life_s: number(tags::RADIONUCLIDE_HALF_LIFE),
            injection_time: text(tags::RADIOPHARMACEUTICAL_START_TIME),
            injection_datetime: text(tags::RADIOPHARMACEUTICAL_START_DATE_TIME),
        }
    }

    /// The record of one file if it is a PET image (modality PT).
    pub(crate) fn of_file(obj: &InMemDicomObject) -> Option<Self> {
        attrs::text(obj, tags::MODALITY).is_some_and(|m| m.eq_ignore_ascii_case("PT")).then(|| Self::read(obj))
    }

    /// The series date and time, in seconds since 1970-01-01.
    pub fn reference_seconds(&self) -> Option<f64> {
        Some(day_seconds(self.series_date.as_deref()?)? + time_seconds(self.series_time.as_deref()?)?)
    }

    /// The injection time, in seconds since 1970-01-01: the Start DateTime,
    /// else the Start Time on the series date.
    pub fn injection_seconds(&self) -> Option<f64> {
        if let Some(datetime) = self.injection_datetime.as_deref().and_then(datetime_seconds) {
            return Some(datetime);
        }
        Some(day_seconds(self.series_date.as_deref()?)? + time_seconds(self.injection_time.as_deref()?)?)
    }
}

/// The earliest acquisition recorded in one file, in seconds since 1970-01-01.
fn acquisition_seconds(obj: &InMemDicomObject) -> Option<f64> {
    attrs::text(obj, tags::ACQUISITION_DATE_TIME).and_then(|dt| datetime_seconds(&dt)).or_else(|| {
        let date = attrs::text(obj, tags::ACQUISITION_DATE)?;
        let time = attrs::text(obj, tags::ACQUISITION_TIME)?;
        Some(day_seconds(&date)? + time_seconds(&time)?)
    })
}

/// How SUV was obtained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SuvBasis {
    /// Computed here from the recorded values (stored unit BQML).
    Computed {
        weight_kg: f64,
        injected_dose_bq: f64,
        half_life_s: f64,
        /// Time from the injection to the reference time, in s.
        decay_s: f64,
        /// The dose left at the reference time, in Bq.
        decayed_dose_bq: f64,
    },
    /// Stored as uptake values in the images (unit GML) and shown as they are.
    Stored,
}

/// The factor that turns the stored values of a PET series into SUVbw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuvScale {
    /// SUV per stored unit: ml/g per Bq/ml for BQML, 1 for stored SUV.
    pub factor: f64,
    pub basis: SuvBasis,
}

impl SuvScale {
    /// Values stored as SUV: shown as they are.
    pub const STORED: Self = Self { factor: 1.0, basis: SuvBasis::Stored };

    /// The SUV of a stored value.
    pub fn suv(&self, stored: f64) -> f64 {
        stored * self.factor
    }

    /// The range a stored value range spans in SUV.
    pub fn range(&self, (lo, hi): (f32, f32)) -> (f32, f32) {
        let (a, b) = (self.suv(f64::from(lo)) as f32, self.suv(f64::from(hi)) as f32);
        (a.min(b), a.max(b))
    }

    /// Scales every grey value of one decoded file, and the windows stored in
    /// it, to SUV. This is the one place where values are converted: it runs
    /// where decoded images enter a series (the viewer's series store and
    /// `load_series`), so everything downstream is in SUV.
    ///
    /// The padding values of each frame are scaled with the pixels, so that
    /// padding is still recognised. Contrast tables (VOI LUT Sequence) are
    /// dropped: they map whole stored-unit values one by one and cannot be
    /// scaled; the stored windows remain.
    pub fn apply(&self, decoded: &mut DecodedInstance) {
        let factor = self.factor;
        if factor == 1.0 {
            return;
        }
        let scale = |value: f32| (f64::from(value) * factor) as f32;
        for frame in &mut decoded.frames {
            if let FramePixels::Gray(buffer) = &mut frame.pixels {
                let values: Vec<f32> = (0..buffer.len()).map(|i| scale(buffer.get(i))).collect();
                *buffer = VoxelBuffer::compact(values);
            }
        }
        for frame in &mut decoded.extras.frames {
            if let Some(padding) = &mut frame.padding {
                *padding = Padding::new(scale(padding.low), scale(padding.high));
            }
        }
        decoded.extras.voi_luts.clear();
        for preset in &mut decoded.presets {
            preset.window.center *= factor;
            // A window is never narrower than its minimum width (as it is applied).
            preset.window.width = (preset.window.width * factor).max(Window::MIN_WIDTH);
        }
    }
}

/// Why SUV is not computed for a PET series. The values then stay in the
/// stored unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuvRefusal {
    /// Units (0054,1001) is absent.
    UnitsMissing,
    /// The values are stored in a unit other than BQML (or GML).
    Units(String),
    /// Units GML with an SUV Type other than body weight.
    StoredSuvType(String),
    /// Corrected Image lacks ATTN.
    NotAttenuationCorrected,
    /// Corrected Image lacks DECY.
    NotDecayCorrected,
    /// Decay Correction is absent.
    DecayCorrectionMissing,
    /// Decay Correction is not START.
    DecayCorrection(String),
    /// Patient Weight is absent or 0.
    WeightMissing,
    /// Radionuclide Total Dose is absent or not positive.
    DoseMissing,
    /// Radionuclide Half Life is absent or not positive.
    HalfLifeMissing,
    /// Neither Radiopharmaceutical Start DateTime nor Start Time (with the
    /// series date) is recorded.
    InjectionTimeMissing,
    /// Series Date or Series Time is absent.
    SeriesTimeMissing,
    /// The injection is later than the series time: a negative decay time.
    InjectionAfterSeries,
    /// An image was acquired before the series time.
    SeriesAfterAcquisition,
    /// More than ten half-lives between injection and series time.
    DecayTooLong,
}

impl SuvRefusal {
    /// Why SUV is not computed, as one sentence.
    pub fn reason(&self) -> String {
        match self {
            Self::UnitsMissing => tr!("The images do not record the unit of their values.").to_owned(),
            Self::Units(unit) => {
                tr!("The values are stored in the unit {unit}, not in becquerel per millilitre (BQML).", unit = unit)
            }
            Self::StoredSuvType(kind) => {
                tr!("The images hold uptake values of the kind {kind}, not by body weight.", kind = kind)
            }
            Self::NotAttenuationCorrected => {
                tr!("The images are not corrected for attenuation (the weakening of the signal inside the body).")
                    .to_owned()
            }
            Self::NotDecayCorrected => {
                tr!("The images are not corrected for the decay of the radioactivity.").to_owned()
            }
            Self::DecayCorrectionMissing => {
                tr!("The images do not record to which moment they are corrected for decay.").to_owned()
            }
            Self::DecayCorrection(code) => {
                tr!("The images are corrected for decay to {code}, not to the start of the scan (START).", code = code)
            }
            Self::WeightMissing => tr!("The patient's weight is not recorded.").to_owned(),
            Self::DoseMissing => tr!("The injected dose is not recorded.").to_owned(),
            Self::HalfLifeMissing => tr!("The half-life of the tracer is not recorded.").to_owned(),
            Self::InjectionTimeMissing => tr!("The injection time is not recorded.").to_owned(),
            Self::SeriesTimeMissing => tr!("The series date or time is not recorded.").to_owned(),
            Self::InjectionAfterSeries => tr!("The recorded injection time is later than the series time.").to_owned(),
            Self::SeriesAfterAcquisition => tr!(
                "The series time is later than the acquisition of its images: the series may have been processed later."
            )
            .to_owned(),
            Self::DecayTooLong => {
                tr!("More than ten half-lives lie between the recorded injection and the series time.").to_owned()
            }
        }
    }
}

impl std::fmt::Display for SuvRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

/// The SUVbw factor of a PET series from its header values (see the module
/// documentation), or why it cannot be computed.
pub fn suv_bw(header: &PetHeader) -> Result<SuvScale, SuvRefusal> {
    let code = |value: &Option<String>| value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    let units = code(&header.units).ok_or(SuvRefusal::UnitsMissing)?;
    if units.eq_ignore_ascii_case("GML") {
        return match code(&header.suv_type) {
            Some(kind) if !kind.eq_ignore_ascii_case("BW") => Err(SuvRefusal::StoredSuvType(kind)),
            _ => Ok(SuvScale::STORED),
        };
    }
    if !units.eq_ignore_ascii_case("BQML") {
        return Err(SuvRefusal::Units(units));
    }
    let corrected = |c: &str| header.corrected_image.iter().any(|v| v.trim().eq_ignore_ascii_case(c));
    if !corrected("ATTN") {
        return Err(SuvRefusal::NotAttenuationCorrected);
    }
    if !corrected("DECY") {
        return Err(SuvRefusal::NotDecayCorrected);
    }
    match code(&header.decay_correction) {
        None => return Err(SuvRefusal::DecayCorrectionMissing),
        Some(reference) if !reference.eq_ignore_ascii_case("START") => {
            return Err(SuvRefusal::DecayCorrection(reference));
        }
        Some(_) => {}
    }
    let positive = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
    let weight_kg = positive(header.patient_weight_kg).ok_or(SuvRefusal::WeightMissing)?;
    let injected_dose_bq = positive(header.injected_dose_bq).ok_or(SuvRefusal::DoseMissing)?;
    let half_life_s = positive(header.half_life_s).ok_or(SuvRefusal::HalfLifeMissing)?;
    let reference = header.reference_seconds().ok_or(SuvRefusal::SeriesTimeMissing)?;
    let injection = header.injection_seconds().ok_or(SuvRefusal::InjectionTimeMissing)?;
    if header.earliest_acquisition_s.is_some_and(|acquired| acquired < reference) {
        return Err(SuvRefusal::SeriesAfterAcquisition);
    }
    let decay_s = reference - injection;
    if decay_s < 0.0 {
        return Err(SuvRefusal::InjectionAfterSeries);
    }
    if decay_s > MAX_HALF_LIVES * half_life_s {
        return Err(SuvRefusal::DecayTooLong);
    }
    let decayed_dose_bq = injected_dose_bq * (-decay_s / half_life_s).exp2();
    let factor = weight_kg * 1000.0 / decayed_dose_bq;
    Ok(SuvScale {
        factor,
        basis: SuvBasis::Computed { weight_kg, injected_dose_bq, half_life_s, decay_s, decayed_dose_bq },
    })
}

/// The SUV outcome of `series`: `None` when it is not a PET series.
pub fn uptake(series: &Series) -> Option<Result<SuvScale, SuvRefusal>> {
    series.pet.as_ref().map(suv_bw)
}

/// The stored unit of PET values in words: BQML → "Bq/ml", GML → "g/ml",
/// CNTS → "counts"; any other code as recorded.
pub fn stored_unit(code: &str) -> String {
    let code = code.trim();
    match code.to_ascii_uppercase().as_str() {
        "BQML" => "Bq/ml".to_owned(),
        "GML" => "g/ml".to_owned(),
        // Translators: the unit of PET values that are raw detector counts.
        "CNTS" => tr!("counts").to_owned(),
        _ => code.to_owned(),
    }
}

/// The unit of the values of a PET series as shown: [`SUV_UNIT`] when the
/// factor applies, else the stored unit; `None` when the series is not PET
/// or records no unit.
pub fn display_unit(outcome: Option<&Result<SuvScale, SuvRefusal>>, header: Option<&PetHeader>) -> Option<String> {
    match outcome? {
        Ok(_) => Some(SUV_UNIT.to_owned()),
        Err(_) => header?.units.as_deref().filter(|u| !u.trim().is_empty()).map(stored_unit),
    }
}

// ---------------------------------------------------------------------------
// Dates and times (scanner time; seconds since 1970-01-01 00:00)

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The start of the day of a DA value (`YYYYMMDD`, or the legacy
/// `YYYY.MM.DD`), in seconds since 1970-01-01.
fn day_seconds(date: &str) -> Option<f64> {
    let digits: String = date.trim().chars().filter(|c| *c != '.').collect();
    if digits.len() != 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let field = |range: std::ops::Range<usize>| digits.get(range).and_then(|s| s.parse::<i64>().ok());
    let (year, month, day) = (field(0..4)?, field(4..6)?, field(6..8)?);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64 * 86_400.0)
}

/// Seconds since midnight of a TM value: `HH`, `HHMM`, `HHMMSS` or
/// `HHMMSS.F…` (also the legacy `HH:MM:SS`).
fn time_seconds(time: &str) -> Option<f64> {
    let time = time.trim();
    let (main, fraction) = time.split_once('.').map_or((time, None), |(main, fraction)| (main, Some(fraction)));
    let digits: String = main.chars().filter(|c| *c != ':').collect();
    if !matches!(digits.len(), 2 | 4 | 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let field = |range: std::ops::Range<usize>| digits.get(range).and_then(|s| s.parse::<u32>().ok());
    let hours = field(0..2)?;
    let minutes = if digits.len() >= 4 { field(2..4)? } else { 0 };
    let seconds = if digits.len() == 6 { field(4..6)? } else { 0 };
    if hours > 23 || minutes > 59 || seconds > 60 {
        return None;
    }
    let fraction = match fraction {
        None => 0.0,
        Some(f) if digits.len() == 6 && !f.is_empty() && f.len() <= 6 && f.bytes().all(|b| b.is_ascii_digit()) => {
            format!("0.{f}").parse::<f64>().ok()?
        }
        Some(_) => return None,
    };
    Some(f64::from(hours * 3600 + minutes * 60 + seconds) + fraction)
}

/// A DT value (`YYYYMMDDHH[MM[SS[.F…]]]`) in seconds since 1970-01-01. A
/// UTC offset at the end (`+ZZXX`, `-ZZXX`) is ignored: all times of a study
/// are the scanner's local time. At least the hour must be recorded.
fn datetime_seconds(datetime: &str) -> Option<f64> {
    let datetime = datetime.trim();
    let main = datetime.find(['+', '-']).map_or(datetime, |at| &datetime[..at]);
    if main.len() < 10 {
        return None;
    }
    Some(day_seconds(main.get(..8)?)? + time_seconds(main.get(8..)?)?)
}

#[cfg(test)]
mod tests {
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, Length, PrimitiveValue, Tag, VR};
    use summrise_imaging::{Frame, PresetSource, WindowPreset};

    use super::*;

    /// The testkit's numbers: 70 kg, 370 MBq of F-18 (half-life 6586.2 s)
    /// injected at 08:25:13.8, one half-life before the series time 10:15:00.
    fn fdg() -> PetHeader {
        PetHeader {
            units: Some("BQML".to_owned()),
            corrected_image: ["DECY", "ATTN", "SCAT", "DTIM"].map(str::to_owned).to_vec(),
            decay_correction: Some("START".to_owned()),
            suv_type: None,
            patient_weight_kg: Some(70.0),
            series_date: Some("20260115".to_owned()),
            series_time: Some("101500".to_owned()),
            earliest_acquisition_s: None,
            radiopharmaceutical: Some("Fluorodeoxyglucose".to_owned()),
            radionuclide: Some("^18^Fluorine".to_owned()),
            injected_dose_bq: Some(370e6),
            half_life_s: Some(6586.2),
            injection_time: Some("082513.8".to_owned()),
            injection_datetime: None,
        }
    }

    #[test]
    fn a_concentration_of_13214_bq_per_ml_is_suv_5() {
        let scale = suv_bw(&fdg()).expect("every input is recorded");
        let SuvBasis::Computed { decay_s, decayed_dose_bq, weight_kg, .. } = scale.basis else {
            panic!("computed, not stored");
        };
        assert!((decay_s - 6586.2).abs() < 1e-6, "one half-life: {decay_s} s");
        assert!((decayed_dose_bq - 185e6).abs() < 1.0, "half the dose is left: {decayed_dose_bq} Bq");
        assert_eq!(weight_kg, 70.0);
        // 13 214.3 Bq/ml × 70 000 g / 185 000 000 Bq = 5.000 005.
        assert!((scale.factor - 70_000.0 / 185e6).abs() < 1e-12, "{}", scale.factor);
        assert!((scale.suv(13_214.3) - 5.0).abs() < 1e-4, "{}", scale.suv(13_214.3));
        // The same with the injection recorded as a DateTime, which is used before the Start Time.
        let datetime = PetHeader { injection_datetime: Some("20260115082513.8".to_owned()), ..fdg() };
        assert_eq!(suv_bw(&datetime), Ok(scale));
        let with_offset = PetHeader { injection_datetime: Some("20260115082513.8+0100".to_owned()), ..fdg() };
        assert_eq!(suv_bw(&with_offset), Ok(scale), "a UTC offset is ignored");
        // Images acquired at or after the series time are what the happy path expects.
        let acquired = PetHeader { earliest_acquisition_s: fdg().reference_seconds(), ..fdg() };
        assert_eq!(suv_bw(&acquired), Ok(scale));
    }

    #[test]
    fn every_missing_or_unsupported_input_is_refused_with_its_reason() {
        use SuvRefusal::*;
        let cases: Vec<(PetHeader, SuvRefusal)> = vec![
            (PetHeader { units: None, ..fdg() }, UnitsMissing),
            (PetHeader { units: Some(" ".to_owned()), ..fdg() }, UnitsMissing),
            (PetHeader { units: Some("CNTS".to_owned()), ..fdg() }, Units("CNTS".to_owned())),
            (
                PetHeader { units: Some("GML".to_owned()), suv_type: Some("LBM".to_owned()), ..fdg() },
                StoredSuvType("LBM".to_owned()),
            ),
            (PetHeader { corrected_image: vec!["DECY".to_owned()], ..fdg() }, NotAttenuationCorrected),
            (PetHeader { corrected_image: vec!["ATTN".to_owned()], ..fdg() }, NotDecayCorrected),
            (PetHeader { decay_correction: None, ..fdg() }, DecayCorrectionMissing),
            (PetHeader { decay_correction: Some("ADMIN".to_owned()), ..fdg() }, DecayCorrection("ADMIN".to_owned())),
            (PetHeader { decay_correction: Some("NONE".to_owned()), ..fdg() }, DecayCorrection("NONE".to_owned())),
            (PetHeader { patient_weight_kg: None, ..fdg() }, WeightMissing),
            (PetHeader { patient_weight_kg: Some(0.0), ..fdg() }, WeightMissing),
            (PetHeader { injected_dose_bq: None, ..fdg() }, DoseMissing),
            (PetHeader { injected_dose_bq: Some(-1.0), ..fdg() }, DoseMissing),
            (PetHeader { half_life_s: None, ..fdg() }, HalfLifeMissing),
            (PetHeader { series_time: None, ..fdg() }, SeriesTimeMissing),
            (PetHeader { series_date: Some("2026".to_owned()), ..fdg() }, SeriesTimeMissing),
            (PetHeader { injection_time: None, ..fdg() }, InjectionTimeMissing),
            (PetHeader { injection_time: Some("8:25".to_owned()), ..fdg() }, InjectionTimeMissing),
            // Injected at 11:00, after the series time 10:15 on the same date: a negative time difference.
            (PetHeader { injection_time: Some("110000".to_owned()), ..fdg() }, InjectionAfterSeries),
            (PetHeader { injection_datetime: Some("20260116080000".to_owned()), ..fdg() }, InjectionAfterSeries),
            // An image acquired a minute before the series time: the series time was rewritten.
            (
                PetHeader { earliest_acquisition_s: fdg().reference_seconds().map(|t| t - 60.0), ..fdg() },
                SeriesAfterAcquisition,
            ),
            // Injected the evening before: more than ten half-lives of F-18 (18.3 hours).
            (PetHeader { injection_datetime: Some("20260114120000".to_owned()), ..fdg() }, DecayTooLong),
        ];
        for (header, expected) in cases {
            assert_eq!(suv_bw(&header), Err(expected.clone()), "{expected:?}");
            let reason = expected.reason();
            assert!(reason.ends_with('.') && !reason.contains('{'), "{reason}");
        }
        assert_eq!(WeightMissing.reason(), "The patient's weight is not recorded.");
        assert_eq!(
            Units("CNTS".to_owned()).to_string(),
            "The values are stored in the unit CNTS, not in becquerel per millilitre (BQML)."
        );
        assert_eq!(
            DecayCorrection("ADMIN".to_owned()).to_string(),
            "The images are corrected for decay to ADMIN, not to the start of the scan (START)."
        );
    }

    #[test]
    fn values_stored_as_suv_pass_through() {
        let stored = PetHeader { units: Some("GML".to_owned()), ..PetHeader::default() };
        assert_eq!(suv_bw(&stored), Ok(SuvScale::STORED));
        let by_weight = PetHeader { suv_type: Some("BW".to_owned()), ..stored.clone() };
        assert_eq!(suv_bw(&by_weight), Ok(SuvScale::STORED));
        assert_eq!(SuvScale::STORED.suv(3.25), 3.25);
        // Nothing is scaled.
        let mut decoded = decoded_with(vec![1.5, 2.25]);
        let before = decoded.clone();
        SuvScale::STORED.apply(&mut decoded);
        assert_eq!(decoded.frames, before.frames);
        assert_eq!(decoded.presets, before.presets);
    }

    fn decoded_with(values: Vec<f32>) -> DecodedInstance {
        let frame =
            Frame { width: values.len() as u32, height: 1, pixels: FramePixels::Gray(VoxelBuffer::compact(values)) };
        DecodedInstance {
            frames: vec![frame],
            monochrome1: false,
            presets: vec![WindowPreset {
                name: "Window 1".into(),
                window: Window::new(13_214.3, 26_428.6),
                source: PresetSource::Dicom,
            }],
            rescale_type: None,
            extras: Default::default(),
        }
    }

    #[test]
    fn decoded_values_and_stored_windows_are_scaled_once() {
        let scale = suv_bw(&fdg()).expect("scale");
        // Integer storage (as from a file without rescale) and floating-point values alike.
        let mut decoded = decoded_with(vec![0.0, 2_642.857_2, 13_214.3]);
        decoded.frames.push(Frame { width: 2, height: 1, pixels: FramePixels::Gray(VoxelBuffer::I16(vec![0, 2643])) });
        scale.apply(&mut decoded);
        let FramePixels::Gray(values) = &decoded.frames[0].pixels else { panic!("grey") };
        assert_eq!(values.get(0), 0.0);
        assert!((values.get(1) - 1.0).abs() < 1e-5, "{}", values.get(1));
        assert!((values.get(2) - 5.0).abs() < 1e-4, "{}", values.get(2));
        let FramePixels::Gray(integers) = &decoded.frames[1].pixels else { panic!("grey") };
        assert!((integers.get(1) - 1.0).abs() < 1e-3);
        let window = decoded.presets[0].window;
        assert!((window.center - 5.0).abs() < 1e-4 && (window.width - 10.0).abs() < 1e-4, "{window:?}");
        assert_eq!(decoded.presets[0].name, "Window 1");
        // A stored window narrower than one SUV keeps the minimum width that is applied.
        let mut narrow = decoded_with(vec![0.0]);
        narrow.presets[0].window = Window::new(2000.0, 1000.0);
        scale.apply(&mut narrow);
        assert_eq!(narrow.presets[0].window.width, Window::MIN_WIDTH);
        // The value-range estimate of the plan.
        let (lo, hi) = scale.range((0.0, 26_428.6));
        assert!(lo == 0.0 && (hi - 10.0).abs() < 1e-4, "{lo} {hi}");
    }

    #[test]
    fn padding_is_scaled_with_the_pixels_and_contrast_tables_are_dropped() {
        use summrise_imaging::{LookupTable, VoiLut};
        let scale = suv_bw(&fdg()).expect("scale");
        let mut decoded = decoded_with(vec![-100.0, 13_214.3]);
        let padding = Some(Padding::new(-100.0, -100.0));
        decoded.extras.frames = vec![crate::pixels::FrameExtras { padding, graphics: Vec::new() }];
        let table = LookupTable::new(0, 8, vec![0, 255]).expect("table");
        decoded.extras.voi_luts = vec![VoiLut { name: "Table".into(), table }];
        scale.apply(&mut decoded);
        let FramePixels::Gray(values) = &decoded.frames[0].pixels else { panic!("grey") };
        let padding = decoded.extras.padding(0).expect("padding");
        assert!(padding.contains(values.get(0)), "still padding: {padding:?}, {}", values.get(0));
        assert!(!padding.contains(values.get(1)), "a measured value is not padding");
        // The tables map stored-unit values and cannot follow; the windows are scaled.
        assert!(decoded.extras.voi_luts.is_empty());
    }

    #[test]
    fn units_in_words() {
        assert_eq!(stored_unit("BQML"), "Bq/ml");
        assert_eq!(stored_unit("GML"), "g/ml");
        assert_eq!(stored_unit("CNTS"), "counts");
        assert_eq!(stored_unit("PROPCPS"), "PROPCPS");
        let header = fdg();
        assert_eq!(display_unit(Some(&suv_bw(&header)), Some(&header)).as_deref(), Some("SUV"));
        let refused = PetHeader { patient_weight_kg: None, ..fdg() };
        assert_eq!(display_unit(Some(&suv_bw(&refused)), Some(&refused)).as_deref(), Some("Bq/ml"));
        let no_unit = PetHeader { units: None, ..fdg() };
        assert_eq!(display_unit(Some(&suv_bw(&no_unit)), Some(&no_unit)), None);
        assert_eq!(display_unit(None, None), None, "not a PET series");
    }

    #[test]
    fn dates_and_times_are_parsed_strictly() {
        assert_eq!(day_seconds("19700101"), Some(0.0));
        assert_eq!(day_seconds("19700102"), Some(86_400.0));
        assert_eq!(day_seconds("2026.01.15"), day_seconds("20260115"));
        // 2000 was a leap year, 2100 will not be.
        assert!(day_seconds("20000229").is_some() && day_seconds("21000229").is_none());
        for bad in ["", "2026", "20261301", "20260132", "2026011a", "２０２６０１１５"] {
            assert_eq!(day_seconds(bad), None, "{bad:?}");
        }
        assert_eq!(time_seconds("101500"), Some(36_900.0));
        assert_eq!(time_seconds("10:15:00"), Some(36_900.0));
        assert_eq!(time_seconds("1015"), Some(36_900.0));
        assert_eq!(time_seconds("10"), Some(36_000.0));
        assert!((time_seconds("082513.8").expect("time") - 30_313.8).abs() < 1e-9);
        for bad in ["", "1", "246000", "106000", "1015.5", "101500.", "10150x", "101500.1234567"] {
            assert_eq!(time_seconds(bad), None, "{bad:?}");
        }
        let day = day_seconds("20260115").expect("date");
        assert_eq!(datetime_seconds("2026011510"), Some(day + 36_000.0));
        assert_eq!(datetime_seconds("20260115101500-0500"), Some(day + 36_900.0));
        for bad in ["", "20260115", "202601151", "2026011510ü"] {
            assert_eq!(datetime_seconds(bad), None, "{bad:?}");
        }
    }

    fn element(tag: Tag, vr: VR, value: &str) -> DataElement<InMemDicomObject> {
        DataElement::new(tag, vr, PrimitiveValue::from(value))
    }

    #[test]
    fn header_values_are_read_from_a_data_set() {
        let drug = InMemDicomObject::from_element_iter([
            element(tags::RADIOPHARMACEUTICAL, VR::LO, "Fluorodeoxyglucose"),
            element(tags::RADIOPHARMACEUTICAL_START_TIME, VR::TM, "082513.8"),
            element(tags::RADIONUCLIDE_TOTAL_DOSE, VR::DS, "370000000"),
            element(tags::RADIONUCLIDE_HALF_LIFE, VR::DS, "6586.2"),
            DataElement::new(
                tags::RADIONUCLIDE_CODE_SEQUENCE,
                VR::SQ,
                DataSetSequence::new(
                    vec![InMemDicomObject::from_element_iter([element(tags::CODE_MEANING, VR::LO, "^18^Fluorine")])],
                    Length::UNDEFINED,
                ),
            ),
        ]);
        let obj = InMemDicomObject::from_element_iter([
            element(tags::MODALITY, VR::CS, "PT"),
            element(tags::UNITS, VR::CS, "BQML"),
            DataElement::new(
                tags::CORRECTED_IMAGE,
                VR::CS,
                PrimitiveValue::Strs(["DECY", "ATTN", "SCAT", "DTIM"].into_iter().map(str::to_owned).collect()),
            ),
            element(tags::DECAY_CORRECTION, VR::CS, "START"),
            element(tags::PATIENT_WEIGHT, VR::DS, "70"),
            element(tags::SERIES_DATE, VR::DA, "20260115"),
            element(tags::SERIES_TIME, VR::TM, "101500"),
            element(tags::ACQUISITION_DATE, VR::DA, "20260115"),
            element(tags::ACQUISITION_TIME, VR::TM, "101500"),
            DataElement::new(
                tags::RADIOPHARMACEUTICAL_INFORMATION_SEQUENCE,
                VR::SQ,
                DataSetSequence::new(vec![drug], Length::UNDEFINED),
            ),
        ]);
        let header = PetHeader::of_file(&obj).expect("a PET file");
        assert_eq!(header, PetHeader { earliest_acquisition_s: header.reference_seconds(), ..fdg() });
        assert!((suv_bw(&header).expect("scale").suv(13_214.3) - 5.0).abs() < 1e-4);
        // Other modalities have no PET record.
        let ct = InMemDicomObject::from_element_iter([element(tags::MODALITY, VR::CS, "CT")]);
        assert_eq!(PetHeader::of_file(&ct), None);
    }
}
