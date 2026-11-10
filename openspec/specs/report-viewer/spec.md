# report-viewer Specification

## Purpose
Showing the reports and documents on a disc — structured reports, dose reports, encapsulated PDF, RTF, HTML and text files — as written, inside the application, without running anything from the disc.
## Requirements
### Requirement: Documents are listed with the study
The series browser SHALL list the documents of the active study — structured reports,
PDF documents stored in DICOM and dose reports — and the report files found on the
medium, each with its type, title and date.

#### Scenario: Structured report on the disc
- **WHEN** a study contains a Basic Text SR titled "MR Report" dated 2026-01-15
- **THEN** the Documents list shows "MR Report" with the type "Report" and the date 2026-01-15

#### Scenario: Loose PDF next to the images
- **WHEN** the disc root contains `REPORT.PDF`
- **THEN** the Documents list of every study on that disc shows "REPORT.PDF" with the type "PDF file"

#### Scenario: No documents
- **WHEN** neither the study nor the medium has documents
- **THEN** no Documents section is shown

### Requirement: Structured report display
The document view SHALL display a structured report in document order as readable text:
containers as headings, text items as paragraphs, coded and numeric items as "name:
value" with units, together with the report's title, date and completion and
verification status.

#### Scenario: Findings and impression
- **WHEN** a report has the containers "Findings" (text "No focal lesion.") and "Impression" (text "Normal study.")
- **THEN** the view shows the heading "Findings" followed by "No focal lesion." and the heading "Impression" followed by "Normal study."

#### Scenario: Numeric item with unit
- **WHEN** a report contains a numeric item "Diameter" with the value 12 and the unit mm
- **THEN** the view shows "Diameter: 12 mm"

#### Scenario: Preliminary report
- **WHEN** a report's completion flag is PARTIAL and it is unverified
- **THEN** the header shows "Preliminary — not verified"

### Requirement: Links from reports to images
Image references in a structured report SHALL be shown as links that open the referenced
image when it is present on the medium.

#### Scenario: Referenced image available
- **WHEN** the user activates a link to an image of series 2
- **THEN** the viewer returns to the images with series 2 selected and the referenced image displayed

#### Scenario: Referenced image missing
- **WHEN** the referenced image is not on the medium
- **THEN** the reference is shown as the text "image not on this disc" and is not a link

### Requirement: PDF display
The document view SHALL display PDF documents — stored in DICOM or as files — page by
page inside the application, with zoom and page navigation, without starting other
applications and without writing files.

#### Scenario: Two-page PDF stored in DICOM
- **WHEN** an Encapsulated PDF object with two pages is selected
- **THEN** both pages can be read and the text stays sharp at 200 % zoom

#### Scenario: PDF that cannot be rendered
- **WHEN** a PDF is damaged or uses an unsupported feature
- **THEN** the view says that the document could not be displayed, the application keeps running and, for a file on the medium, "Show in folder" is offered

### Requirement: Text documents
Text, RTF and HTML report files SHALL be displayed as plain readable text; scripts SHALL
NOT run and remote content SHALL NOT be loaded.

#### Scenario: HTML report with a script and a remote image
- **WHEN** an HTML report containing a script and an image URL is selected
- **THEN** its text is shown, the script does not run and no network connection is made

#### Scenario: RTF report in a Windows code page
- **WHEN** an RTF report written in Windows-1251 Cyrillic is selected
- **THEN** the Cyrillic text is displayed correctly

### Requirement: Dose report display
An X-Ray Radiation Dose report SHALL be displayed as a table of the recorded irradiation
events with their CTDIvol and dose-length product (DLP) and the recorded total, labelled
as values recorded by the scanner; the application SHALL NOT calculate an effective
dose.

#### Scenario: CT with two acquisitions
- **WHEN** the dose report records two acquisitions with DLP 250 mGy·cm and 320 mGy·cm and a total of 570 mGy·cm
- **THEN** the table shows both rows and "Total DLP 570 mGy·cm (as recorded by the scanner)"

### Requirement: Safe handling of documents
The application SHALL read documents into memory only, SHALL list only document types
it can display, and SHALL NOT execute files or open them in other applications without
an explicit user action.

#### Scenario: Viewer software on the disc
- **WHEN** the disc contains `AUTORUN.INF`, `VIEWER/viewer.exe` and `VIEWER/manual.pdf`
- **THEN** none of them is listed as a document

#### Scenario: Oversized file
- **WHEN** a loose PDF is larger than 50 MB
- **THEN** it is listed as "too large to display" and is not loaded

#### Scenario: Report nested deeper than is followed
- **WHEN** a structured report nests its content more than 64 levels deep
- **THEN** the items down to that level are shown and the view says that part of the report is not shown

#### Scenario: File nested too deeply to parse
- **WHEN** a DICOM file nests its sequences more than 128 levels deep
- **THEN** the application keeps running and the file is counted among the files that could not be read

### Requirement: Privacy mode hides document content
While privacy mode is on, the document view SHALL hide the content of a document until
the user chooses to reveal it.

#### Scenario: Selecting a report in privacy mode
- **WHEN** privacy mode is on and the user selects a report
- **THEN** the view shows "Hidden in privacy mode — reports contain personal details" with a "Show anyway" button, and reveals the text only after it is pressed

#### Scenario: The list of documents in privacy mode
- **WHEN** privacy mode is on and the disc holds a file `Report_Doe.pdf`
- **THEN** the list shows it as "PDF file", without its name

### Requirement: Reports are shown verbatim
The application SHALL display report text as written and SHALL NOT summarise, translate
or interpret it.

#### Scenario: Reading a report
- **WHEN** a report is displayed
- **THEN** every text item of the report appears unchanged and no generated commentary is added

