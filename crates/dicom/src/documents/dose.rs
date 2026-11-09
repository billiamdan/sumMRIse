//! Radiation dose reports (TID 10011, CT Radiation Dose): the irradiation
//! events with their CTDIvol and dose-length product, and the recorded total.
//! Every value is what the scanner recorded, as it recorded it. Nothing is
//! calculated, not the total and not an effective dose.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::sr::{self, MAX_TREE_DEPTH, StructuredReport, children, measured_value, verbatim};
use crate::attrs;

/// CT Acquisition: one irradiation event.
const CT_ACQUISITION: &str = "113819";
/// CT Dose Length Product Total.
const CT_DLP_TOTAL: &str = "113813";
/// Mean CTDIvol.
const MEAN_CTDI_VOL: &str = "113830";
/// DLP.
const DLP: &str = "113838";
/// Acquisition Protocol.
const ACQUISITION_PROTOCOL: &str = "125203";
/// Target Region.
const TARGET_REGION: &str = "123014";

/// One recorded irradiation event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DoseEvent {
    /// The acquisition protocol or target region, as recorded.
    pub label: Option<String>,
    /// CTDIvol as recorded, with its unit.
    pub ctdi_vol: Option<String>,
    /// Dose-length product as recorded, with its unit.
    pub dlp: Option<String>,
}

/// A dose report: the table and the report it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoseReport {
    pub events: Vec<DoseEvent>,
    /// The total the scanner recorded; `None` when it recorded none.
    pub total_dlp: Option<String>,
    /// Everything the report holds, in document order.
    pub report: StructuredReport,
}

fn code_of(item: &InMemDicomObject) -> Option<String> {
    let name = attrs::first_item(item, tags::CONCEPT_NAME_CODE_SEQUENCE)?;
    attrs::text(name, tags::CODE_VALUE)
}

/// Fills `event` from what lies below an acquisition.
fn read_event(item: &InMemDicomObject, depth: usize, event: &mut DoseEvent) {
    if depth >= MAX_TREE_DEPTH {
        return;
    }
    for child in children(item) {
        match code_of(child).as_deref() {
            Some(MEAN_CTDI_VOL) if event.ctdi_vol.is_none() => event.ctdi_vol = measured_value(child),
            Some(DLP) if event.dlp.is_none() => event.dlp = measured_value(child),
            Some(ACQUISITION_PROTOCOL) => {
                if event.label.is_none() {
                    event.label = verbatim(child, tags::TEXT_VALUE);
                }
            }
            Some(TARGET_REGION) => {
                // The protocol names an acquisition better; the region is what is left without one.
                if event.label.is_none() {
                    event.label = attrs::first_item(child, tags::CONCEPT_CODE_SEQUENCE)
                        .and_then(|code| attrs::text(code, tags::CODE_MEANING));
                }
            }
            _ => read_event(child, depth + 1, event),
        }
    }
}

fn find_events(item: &InMemDicomObject, depth: usize, events: &mut Vec<DoseEvent>, total: &mut Option<String>) {
    if depth >= MAX_TREE_DEPTH {
        return;
    }
    for child in children(item) {
        match code_of(child).as_deref() {
            Some(CT_ACQUISITION) => {
                let mut event = DoseEvent::default();
                read_event(child, depth + 1, &mut event);
                events.push(event);
            }
            Some(CT_DLP_TOTAL) if total.is_none() => *total = measured_value(child),
            _ => find_events(child, depth + 1, events, total),
        }
    }
}

/// Reads the dose report in `obj`.
pub(super) fn read(obj: &InMemDicomObject) -> DoseReport {
    let mut events = Vec::new();
    let mut total_dlp = None;
    find_events(obj, 0, &mut events, &mut total_dlp);
    DoseReport { events, total_dlp, report: sr::read(obj) }
}

#[cfg(test)]
mod tests {
    use dicom_dictionary_std::uids;
    use dicom_object::OpenFileOptions;
    use summrise_testkit::reports::{
        DOSE_EVENTS, DOSE_REPORT_TITLE, ReportNode, ReportSpec, dose_content, write_structured_report,
    };

    use super::*;

    fn report_of(content: Vec<ReportNode>) -> DoseReport {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("SR");
        let spec = ReportSpec::new(uids::X_RAY_RADIATION_DOSE_SR_STORAGE, DOSE_REPORT_TITLE, content);
        write_structured_report(&path, &spec).expect("report");
        let file = OpenFileOptions::new().open_file(&path).expect("parse");
        read(&file)
    }

    /// The scenario of the specification.
    #[test]
    fn two_acquisitions_with_the_recorded_total() {
        let dose = report_of(dose_content(&DOSE_EVENTS, "570"));
        assert_eq!(dose.total_dlp.as_deref(), Some("570 mGy·cm"));
        assert_eq!(
            dose.events,
            [
                DoseEvent {
                    label: Some("Chest".to_owned()),
                    ctdi_vol: Some("12.5 mGy".to_owned()),
                    dlp: Some("250 mGy·cm".to_owned()),
                },
                DoseEvent {
                    label: Some("Abdomen".to_owned()),
                    ctdi_vol: Some("16 mGy".to_owned()),
                    dlp: Some("320 mGy·cm".to_owned()),
                },
            ]
        );
        // Everything else the report holds stays readable.
        assert!(!dose.report.items.is_empty());
    }

    /// Nothing is calculated: a total that does not add up is shown as recorded,
    /// and no total is made up when none was recorded.
    #[test]
    fn the_total_is_never_calculated() {
        let dose = report_of(dose_content(&DOSE_EVENTS, "999"));
        assert_eq!(dose.total_dlp.as_deref(), Some("999 mGy·cm"));
        let without: Vec<ReportNode> = dose_content(&DOSE_EVENTS, "570").into_iter().skip(1).collect();
        let dose = report_of(without);
        assert_eq!(dose.events.len(), 2);
        assert_eq!(dose.total_dlp, None);
    }

    #[test]
    fn a_dose_report_without_ct_data_has_no_table() {
        let dose = report_of(vec![ReportNode::number("Dose Area Product", "1.2", "Gy.m2")]);
        assert_eq!(dose.events, []);
        assert_eq!(dose.total_dlp, None);
        assert_eq!(dose.report.items.len(), 1);
    }

    #[test]
    fn an_event_without_values_stays_an_event() {
        let dose = report_of(vec![ReportNode::Coded { code: "113819", name: "CT Acquisition", children: Vec::new() }]);
        assert_eq!(dose.events, [DoseEvent::default()]);
    }
}
