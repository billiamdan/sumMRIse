## ADDED Requirements

### Requirement: Document discovery
The importer SHALL index DICOM document objects (structured reports, Encapsulated PDF)
with their study, whether they are found through DICOMDIR records or a folder scan, and
SHALL find report files (PDF, RTF, text, HTML) on the medium outside folders that contain
executable software.

#### Scenario: DICOMDIR with a report record
- **WHEN** the DICOMDIR lists an SR DOCUMENT record under a study
- **THEN** the report is indexed as a document of that study and is not counted among the objects that are not shown

#### Scenario: Report file beside the viewer software
- **WHEN** the medium contains `REPORTS/report_001.rtf` and `VIEWER/readme.txt` next to `VIEWER/viewer.exe`
- **THEN** `report_001.rtf` is indexed as a document and `readme.txt` is not
