//! Series-level acquisition parameters: how the images of a series were
//! made (timing, sequence, kernel, contrast agent…), as far as the headers
//! record it.
//!
//! They are read while the headers are indexed and kept once per series (the
//! values of the first file indexed for it), so that a series can be
//! described before any of its images is displayed. Classic attributes are
//! read first, then the functional groups of Enhanced multi-frame objects.
//! Nothing here identifies the patient.

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use crate::attrs;

/// What the headers record about the acquisition of a series. Every field is
/// optional: absent, empty and malformed values are `None` or empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Acquisition {
    /// Repetition Time (0018,0080) in ms.
    pub repetition_time_ms: Option<f64>,
    /// Echo Time (0018,0081), or Effective Echo Time (0018,9082) of Enhanced
    /// objects, in ms.
    pub echo_time_ms: Option<f64>,
    /// Inversion Time (0018,0082), or the first of the Inversion Times
    /// (0018,9079) of Enhanced objects, in ms.
    pub inversion_time_ms: Option<f64>,
    /// Flip Angle (0018,1314) in degrees.
    pub flip_angle_deg: Option<f64>,
    /// Scanning Sequence (0018,0020) codes: SE, IR, GR, EP, RM. For Enhanced
    /// MR objects, which do not have the attribute, the same codes derived
    /// from Echo Pulse Sequence, Inversion Recovery and Echo Planar Pulse
    /// Sequence.
    pub scanning_sequence: Vec<String>,
    /// Sequence Variant (0018,0021) codes, e.g. SK, SP, MP.
    pub sequence_variant: Vec<String>,
    /// Sequence Name (0018,0024), or Pulse Sequence Name (0018,9005).
    pub sequence_name: Option<String>,
    /// Protocol Name (0018,1030).
    pub protocol_name: Option<String>,
    /// MR Acquisition Type (0018,0023): `2D` or `3D`.
    pub mr_acquisition_type: Option<String>,
    /// Magnetic Field Strength (0018,0087) in tesla.
    pub field_strength_t: Option<f64>,
    /// Diffusion b-value (0018,9087) in s/mm².
    pub diffusion_b_value: Option<f64>,
    /// Contrast/Bolus Agent (0018,0010), or the code meaning of the first
    /// item of the Contrast/Bolus Agent Sequence (0018,0012).
    pub contrast_agent: Option<String>,
    /// Convolution Kernel (0018,1210); several values are joined with ", ".
    pub convolution_kernel: Option<String>,
    /// Slice Thickness (0018,0050) in mm.
    pub slice_thickness_mm: Option<f64>,
    /// KVP (0018,0060): the X-ray tube voltage in kV.
    pub kvp: Option<f64>,
    /// Image Type (0008,0008) values, or the Frame Type of Enhanced objects.
    pub image_type: Vec<String>,
    /// Body Part Examined (0018,0015), or for Enhanced objects the code
    /// meaning of the Anatomic Region of the Frame Anatomy.
    pub body_part: Option<String>,
    /// Recommended Display Frame Rate (0008,2144), in frames per second.
    pub recommended_frame_rate: Option<f64>,
    /// Cine Rate (0018,0040), in frames per second.
    pub cine_rate: Option<f64>,
    /// Frame Time (0018,1063): the time between two frames, in ms.
    pub frame_time_ms: Option<f64>,
}

impl Acquisition {
    /// The rate at which the images are meant to be played, in frames per
    /// second: the Recommended Display Frame Rate, else the Cine Rate, else one
    /// frame per Frame Time; `None` when none of them is recorded.
    pub fn display_frame_rate(&self) -> Option<f64> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        self.recommended_frame_rate
            .filter(|&r| positive(r))
            .or(self.cine_rate.filter(|&r| positive(r)))
            .or(self.frame_time_ms.filter(|&t| positive(t)).map(|t| 1000.0 / t))
    }
}

impl Acquisition {
    /// Reads the parameters from the data set of one file.
    pub fn read(obj: &InMemDicomObject) -> Self {
        Self {
            repetition_time_ms: float(obj, tags::MR_TIMING_AND_RELATED_PARAMETERS_SEQUENCE, tags::REPETITION_TIME),
            echo_time_ms: attrs::float(obj, tags::ECHO_TIME)
                .or_else(|| float(obj, tags::MR_ECHO_SEQUENCE, tags::EFFECTIVE_ECHO_TIME)),
            inversion_time_ms: attrs::float(obj, tags::INVERSION_TIME).or_else(|| {
                attrs::classic_or_functional(obj, tags::MR_MODIFIER_SEQUENCE)
                    .find_map(|o| attrs::floats(o, tags::INVERSION_TIMES).first().copied())
                    .filter(|v| v.is_finite())
            }),
            flip_angle_deg: float(obj, tags::MR_TIMING_AND_RELATED_PARAMETERS_SEQUENCE, tags::FLIP_ANGLE),
            scanning_sequence: scanning_sequence(obj),
            sequence_variant: codes(obj, tags::SEQUENCE_VARIANT),
            sequence_name: attrs::text(obj, tags::SEQUENCE_NAME)
                .or_else(|| attrs::text(obj, tags::PULSE_SEQUENCE_NAME)),
            protocol_name: attrs::text(obj, tags::PROTOCOL_NAME),
            mr_acquisition_type: attrs::text(obj, tags::MR_ACQUISITION_TYPE),
            field_strength_t: attrs::float(obj, tags::MAGNETIC_FIELD_STRENGTH),
            diffusion_b_value: float(obj, tags::MR_DIFFUSION_SEQUENCE, tags::DIFFUSION_B_VALUE),
            contrast_agent: attrs::text(obj, tags::CONTRAST_BOLUS_AGENT).or_else(|| {
                attrs::first_item(obj, tags::CONTRAST_BOLUS_AGENT_SEQUENCE)
                    .and_then(|item| attrs::text(item, tags::CODE_MEANING))
            }),
            convolution_kernel: attrs::classic_or_functional(obj, tags::CT_RECONSTRUCTION_SEQUENCE)
                .map(|o| codes(o, tags::CONVOLUTION_KERNEL))
                .find(|values| !values.is_empty())
                .map(|values| values.join(", ")),
            slice_thickness_mm: float(obj, tags::PIXEL_MEASURES_SEQUENCE, tags::SLICE_THICKNESS),
            kvp: float(obj, tags::CTX_RAY_DETAILS_SEQUENCE, tags::KVP),
            image_type: image_type(obj),
            body_part: attrs::text(obj, tags::BODY_PART_EXAMINED).or_else(|| {
                attrs::classic_or_functional(obj, tags::FRAME_ANATOMY_SEQUENCE)
                    .filter_map(|o| attrs::first_item(o, tags::ANATOMIC_REGION_SEQUENCE))
                    .find_map(|region| attrs::text(region, tags::CODE_MEANING))
            }),
            recommended_frame_rate: attrs::float(obj, tags::RECOMMENDED_DISPLAY_FRAME_RATE),
            cine_rate: attrs::float(obj, tags::CINE_RATE),
            frame_time_ms: attrs::float(obj, tags::FRAME_TIME),
        }
    }
}

/// A number held at the top level by classic objects and in the functional
/// group macro `macro_seq` by Enhanced multi-frame objects.
fn float(obj: &InMemDicomObject, macro_seq: Tag, tag: Tag) -> Option<f64> {
    attrs::classic_or_functional(obj, macro_seq).find_map(|o| attrs::float(o, tag))
}

/// The non-empty values of a multi-valued code attribute.
fn codes(obj: &InMemDicomObject, tag: Tag) -> Vec<String> {
    attrs::texts(obj, tag).into_iter().filter(|value| !value.is_empty()).collect()
}

fn scanning_sequence(obj: &InMemDicomObject) -> Vec<String> {
    let classic: Vec<String> =
        codes(obj, tags::SCANNING_SEQUENCE).into_iter().map(|code| code.to_ascii_uppercase()).collect();
    if !classic.is_empty() {
        return classic;
    }
    // Enhanced MR: the same facts are spread over the MR Pulse Sequence
    // module and the MR Modifier functional group.
    let is = |value: Option<String>, expected: &str| value.is_some_and(|v| v.eq_ignore_ascii_case(expected));
    let echo = attrs::text(obj, tags::ECHO_PULSE_SEQUENCE).map(|v| v.to_ascii_uppercase());
    let mut derived = Vec::new();
    if matches!(echo.as_deref(), Some("SPIN" | "BOTH")) {
        derived.push("SE".to_owned());
    }
    let inversion = attrs::classic_or_functional(obj, tags::MR_MODIFIER_SEQUENCE)
        .find_map(|o| attrs::text(o, tags::INVERSION_RECOVERY));
    if is(inversion, "YES") {
        derived.push("IR".to_owned());
    }
    if matches!(echo.as_deref(), Some("GRADIENT" | "BOTH")) {
        derived.push("GR".to_owned());
    }
    if is(attrs::text(obj, tags::ECHO_PLANAR_PULSE_SEQUENCE), "YES") {
        derived.push("EP".to_owned());
    }
    derived
}

fn image_type(obj: &InMemDicomObject) -> Vec<String> {
    let classic = codes(obj, tags::IMAGE_TYPE);
    if !classic.is_empty() {
        return classic;
    }
    [tags::MR_IMAGE_FRAME_TYPE_SEQUENCE, tags::CT_IMAGE_FRAME_TYPE_SEQUENCE]
        .into_iter()
        .flat_map(|macro_seq| attrs::classic_or_functional(obj, macro_seq))
        .map(|o| codes(o, tags::FRAME_TYPE))
        .find(|values| !values.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, Length, PrimitiveValue, VR};

    use super::*;

    type Element = DataElement<InMemDicomObject>;

    fn text(tag: Tag, vr: VR, value: &str) -> Element {
        DataElement::new(tag, vr, PrimitiveValue::from(value))
    }

    fn values(tag: Tag, vr: VR, values: &[&str]) -> Element {
        DataElement::new(tag, vr, PrimitiveValue::Strs(values.iter().map(|v| (*v).to_owned()).collect()))
    }

    fn double(tag: Tag, value: f64) -> Element {
        DataElement::new(tag, VR::FD, PrimitiveValue::from(value))
    }

    fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> Element {
        DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED))
    }

    fn object(elements: impl IntoIterator<Item = Element>) -> InMemDicomObject {
        InMemDicomObject::from_element_iter(elements)
    }

    #[test]
    fn classic_mr_attributes_are_read() {
        let obj = object([
            text(tags::REPETITION_TIME, VR::DS, "4000"),
            text(tags::ECHO_TIME, VR::DS, "96.5 "),
            text(tags::INVERSION_TIME, VR::DS, "150"),
            text(tags::FLIP_ANGLE, VR::DS, "150"),
            values(tags::SCANNING_SEQUENCE, VR::CS, &["SE", "IR"]),
            values(tags::SEQUENCE_VARIANT, VR::CS, &["SK", "SP"]),
            text(tags::SEQUENCE_NAME, VR::SH, "*tir2d1_15"),
            text(tags::PROTOCOL_NAME, VR::LO, "t2_tirm_cor"),
            text(tags::MR_ACQUISITION_TYPE, VR::CS, "2D"),
            text(tags::MAGNETIC_FIELD_STRENGTH, VR::DS, "1.5"),
            double(tags::DIFFUSION_B_VALUE, 1000.0),
            text(tags::SLICE_THICKNESS, VR::DS, "4"),
            values(tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "ND"]),
            text(tags::BODY_PART_EXAMINED, VR::CS, "KNEE"),
        ]);
        let a = Acquisition::read(&obj);
        assert_eq!(a.repetition_time_ms, Some(4000.0));
        assert_eq!(a.echo_time_ms, Some(96.5));
        assert_eq!(a.inversion_time_ms, Some(150.0));
        assert_eq!(a.flip_angle_deg, Some(150.0));
        assert_eq!(a.scanning_sequence, ["SE", "IR"]);
        assert_eq!(a.sequence_variant, ["SK", "SP"]);
        assert_eq!(a.sequence_name.as_deref(), Some("*tir2d1_15"));
        assert_eq!(a.protocol_name.as_deref(), Some("t2_tirm_cor"));
        assert_eq!(a.mr_acquisition_type.as_deref(), Some("2D"));
        assert_eq!(a.field_strength_t, Some(1.5));
        assert_eq!(a.diffusion_b_value, Some(1000.0));
        assert_eq!(a.slice_thickness_mm, Some(4.0));
        assert_eq!(a.image_type, ["ORIGINAL", "PRIMARY", "M", "ND"]);
        assert_eq!(a.body_part.as_deref(), Some("KNEE"));
        assert_eq!((a.contrast_agent, a.convolution_kernel, a.kvp), (None, None, None));
    }

    #[test]
    fn classic_ct_attributes_are_read() {
        let obj = object([
            text(tags::KVP, VR::DS, "120"),
            values(tags::CONVOLUTION_KERNEL, VR::SH, &["B30f", "3"]),
            text(tags::SLICE_THICKNESS, VR::DS, "1.25"),
            text(tags::CONTRAST_BOLUS_AGENT, VR::LO, "Iopromide 370"),
            values(tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "AXIAL"]),
            text(tags::BODY_PART_EXAMINED, VR::CS, "CHEST"),
        ]);
        let a = Acquisition::read(&obj);
        assert_eq!(a.kvp, Some(120.0));
        assert_eq!(a.convolution_kernel.as_deref(), Some("B30f, 3"));
        assert_eq!(a.slice_thickness_mm, Some(1.25));
        assert_eq!(a.contrast_agent.as_deref(), Some("Iopromide 370"));
        assert_eq!(a.image_type, ["ORIGINAL", "PRIMARY", "AXIAL"]);
        assert_eq!(a.body_part.as_deref(), Some("CHEST"));
        assert!(a.scanning_sequence.is_empty() && a.repetition_time_ms.is_none());
    }

    #[test]
    fn absent_empty_and_malformed_values_are_none() {
        assert_eq!(Acquisition::read(&InMemDicomObject::new_empty()), Acquisition::default());
        let obj = object([
            text(tags::REPETITION_TIME, VR::DS, "fast"),
            text(tags::ECHO_TIME, VR::DS, ""),
            text(tags::CONTRAST_BOLUS_AGENT, VR::LO, "  "),
            values(tags::SCANNING_SEQUENCE, VR::CS, &["", ""]),
            text(tags::BODY_PART_EXAMINED, VR::CS, ""),
            sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, Vec::new()),
            sequence(tags::CONTRAST_BOLUS_AGENT_SEQUENCE, Vec::new()),
        ]);
        assert_eq!(Acquisition::read(&obj), Acquisition::default());
    }

    /// An Enhanced MR object: nothing at the top level but the pulse sequence
    /// module; timing, thickness and anatomy in the shared functional groups.
    #[test]
    fn enhanced_mr_functional_groups_are_read() {
        let shared = object([
            sequence(
                tags::MR_TIMING_AND_RELATED_PARAMETERS_SEQUENCE,
                vec![object([double(tags::REPETITION_TIME, 9000.0), double(tags::FLIP_ANGLE, 90.0)])],
            ),
            sequence(tags::MR_ECHO_SEQUENCE, vec![object([double(tags::EFFECTIVE_ECHO_TIME, 120.0)])]),
            sequence(
                tags::MR_MODIFIER_SEQUENCE,
                vec![object([text(tags::INVERSION_RECOVERY, VR::CS, "YES"), double(tags::INVERSION_TIMES, 2500.0)])],
            ),
            sequence(tags::PIXEL_MEASURES_SEQUENCE, vec![object([text(tags::SLICE_THICKNESS, VR::DS, "5")])]),
            sequence(
                tags::FRAME_ANATOMY_SEQUENCE,
                vec![object([sequence(
                    tags::ANATOMIC_REGION_SEQUENCE,
                    vec![object([text(tags::CODE_MEANING, VR::LO, "Brain")])],
                )])],
            ),
            sequence(
                tags::MR_IMAGE_FRAME_TYPE_SEQUENCE,
                vec![object([values(tags::FRAME_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "NONE"])])],
            ),
        ]);
        let obj = object([
            text(tags::ECHO_PULSE_SEQUENCE, VR::CS, "SPIN"),
            text(tags::ECHO_PLANAR_PULSE_SEQUENCE, VR::CS, "NO"),
            text(tags::PULSE_SEQUENCE_NAME, VR::SH, "TIR"),
            text(tags::MR_ACQUISITION_TYPE, VR::CS, "2D"),
            sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![shared]),
        ]);
        let a = Acquisition::read(&obj);
        assert_eq!(a.repetition_time_ms, Some(9000.0));
        assert_eq!(a.echo_time_ms, Some(120.0));
        assert_eq!(a.inversion_time_ms, Some(2500.0));
        assert_eq!(a.flip_angle_deg, Some(90.0));
        assert_eq!(a.scanning_sequence, ["SE", "IR"], "derived from the Enhanced attributes");
        assert_eq!(a.sequence_name.as_deref(), Some("TIR"));
        assert_eq!(a.slice_thickness_mm, Some(5.0));
        assert_eq!(a.body_part.as_deref(), Some("Brain"));
        assert_eq!(a.image_type, ["ORIGINAL", "PRIMARY", "M", "NONE"]);
    }

    #[test]
    fn enhanced_values_per_frame_or_flat_in_the_shared_group_are_read() {
        // Per frame: the first frame stands for the series.
        let frame = |b: f64| {
            object([sequence(tags::MR_DIFFUSION_SEQUENCE, vec![object([double(tags::DIFFUSION_B_VALUE, b)])])])
        };
        let per_frame = object([
            text(tags::ECHO_PULSE_SEQUENCE, VR::CS, "BOTH"),
            text(tags::ECHO_PLANAR_PULSE_SEQUENCE, VR::CS, "YES"),
            sequence(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, vec![frame(800.0), frame(0.0)]),
        ]);
        let a = Acquisition::read(&per_frame);
        assert_eq!(a.diffusion_b_value, Some(800.0));
        assert_eq!(a.scanning_sequence, ["SE", "GR", "EP"]);

        // Flat in the shared group, as some files store functional group values.
        let flat = object([sequence(
            tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![object([
                text(tags::SLICE_THICKNESS, VR::DS, "0.625"),
                text(tags::KVP, VR::DS, "100"),
                text(tags::CONVOLUTION_KERNEL, VR::SH, "BONE"),
            ])],
        )]);
        let a = Acquisition::read(&flat);
        assert_eq!((a.slice_thickness_mm, a.kvp), (Some(0.625), Some(100.0)));
        assert_eq!(a.convolution_kernel.as_deref(), Some("BONE"));

        // The classic attribute wins over the functional groups.
        let both = object([
            text(tags::SLICE_THICKNESS, VR::DS, "3"),
            sequence(
                tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE,
                vec![object([sequence(
                    tags::PIXEL_MEASURES_SEQUENCE,
                    vec![object([text(tags::SLICE_THICKNESS, VR::DS, "9")])],
                )])],
            ),
        ]);
        assert_eq!(Acquisition::read(&both).slice_thickness_mm, Some(3.0));
    }

    #[test]
    fn display_frame_rate_prefers_the_recommended_rate() {
        let rate = |elements: Vec<Element>| Acquisition::read(&object(elements)).display_frame_rate();
        // Frame Time 40 ms: 25 frames per second.
        let cine = Acquisition::read(&object([text(tags::FRAME_TIME, VR::DS, "40")]));
        assert_eq!((cine.frame_time_ms, cine.display_frame_rate()), (Some(40.0), Some(25.0)));
        assert_eq!(rate(vec![text(tags::CINE_RATE, VR::IS, "30"), text(tags::FRAME_TIME, VR::DS, "40")]), Some(30.0));
        assert_eq!(
            rate(vec![
                text(tags::RECOMMENDED_DISPLAY_FRAME_RATE, VR::IS, "15"),
                text(tags::CINE_RATE, VR::IS, "30"),
                text(tags::FRAME_TIME, VR::DS, "40"),
            ]),
            Some(15.0)
        );
        // Zero or nothing recorded: no rate.
        assert_eq!(rate(vec![text(tags::FRAME_TIME, VR::DS, "0"), text(tags::CINE_RATE, VR::IS, "0")]), None);
        assert_eq!(rate(Vec::new()), None);
    }

    #[test]
    fn contrast_agent_falls_back_to_the_coded_agent() {
        let obj = object([sequence(
            tags::CONTRAST_BOLUS_AGENT_SEQUENCE,
            vec![object([text(tags::CODE_MEANING, VR::LO, "Gadolinium")])],
        )]);
        assert_eq!(Acquisition::read(&obj).contrast_agent.as_deref(), Some("Gadolinium"));
    }
}
