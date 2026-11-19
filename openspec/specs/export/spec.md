# export Specification

## Purpose
Saving, copying and printing what the user sees: pictures of the view, whole series as numbered files, printed sheets and PDFs, with patient details left out unless the user includes them, and nothing written anywhere the user did not choose.
## Requirements
### Requirement: Save the current view as an image
The viewer SHALL save the active view as a PNG or JPEG file through the system save
dialog, either as "image only" — the picture at its native resolution with the current
contrast window and no text — or "with annotations" — as displayed.

#### Scenario: Image-only export of a CT slice
- **WHEN** a 512 × 512 CT slice displayed with the Lung window (W 1500 / L −600) is saved as an "image only" PNG
- **THEN** the file is a 512 × 512 8-bit greyscale image in which −600 HU is mid-grey (127 or 128), −1350 HU and below are black, and +150 HU and above are white

#### Scenario: Non-square pixels
- **WHEN** an image of 256 columns × 128 rows with Pixel Spacing 1.0 \ 0.5 is saved as "image only"
- **THEN** the file is 256 × 256 pixels and shows the anatomy with correct proportions

#### Scenario: Export with annotations
- **WHEN** the user saves "with annotations" while zoomed to 200 %
- **THEN** the file shows the view as displayed, including orientation letters, scale bar and window values

#### Scenario: From a tile grid
- **WHEN** a 3 × 3 tile grid shows images 31 to 39 and the user saves the view
- **THEN** "with annotations" shows the grid as displayed, and "image only" is the picture of image 31, the current image

### Requirement: Patient details excluded by default
Exports SHALL NOT contain the patient's name, ID or birth date — in the picture, in the
suggested file name or in file metadata — unless the user enables "Include patient
details" for that export; the choice is not remembered.

#### Scenario: Default export with annotations
- **WHEN** the user saves "with annotations" without changing any option
- **THEN** the picture shows the study and series texts but no patient name, ID or birth date, and the suggested file name has the form `SumMRIse-CT-S1-031.png`

#### Scenario: Details switched on once
- **WHEN** the user ticks "Include patient details" for one export and later opens the export dialog again
- **THEN** the first file contains the patient lines and the option is unticked again

### Requirement: Commands rest while a document is shown
The commands to save, copy, export and print images SHALL be disabled while a report or
document replaces the images, and the copy shortcut SHALL then copy the document's selected
text.

#### Scenario: Shortcut over a report
- **WHEN** a report is shown and the user presses ⌘S or ⌘P
- **THEN** no window opens; after going back to the images the same shortcut opens it

### Requirement: Burned-in text warning
The viewer SHALL warn before exporting an image whose pixels may contain personal
details — Burned In Annotation is YES, or the image is a secondary capture or ultrasound
image — that such details cannot be removed, and SHALL let the user cancel.

#### Scenario: Scanned report page
- **WHEN** the user exports a secondary-capture image
- **THEN** a warning explains that the picture itself may show personal details, with the choices "Export anyway" and "Cancel"

### Requirement: Copy to the clipboard
The viewer SHALL copy the active view to the system clipboard as an image without
patient details, with ⌘C on macOS and Ctrl+C on Windows and Linux.

#### Scenario: Pasting into a message
- **WHEN** the user presses the copy shortcut and pastes into a mail or chat application
- **THEN** the image-only picture of the active view is inserted

### Requirement: Export a series as images
The viewer SHALL export all images of a fully loaded series as consecutively numbered
PNG or JPEG files into a folder chosen by the user, with progress and cancel, using the
current contrast window, and SHALL NOT overwrite existing files without confirmation.

#### Scenario: Sixty-slice series
- **WHEN** a 60-image series is exported as PNG
- **THEN** the folder contains 60 files numbered 001 to 060 in display order

#### Scenario: Cancelling
- **WHEN** the user cancels after 20 files
- **THEN** the export stops, the 20 files remain and a message states that 20 of 60 images were exported

### Requirement: Derived-image caption
Exports "with annotations" SHALL carry the caption "Not for diagnostic use"; "image
only" exports contain no added text.

#### Scenario: Caption present
- **WHEN** a view is saved "with annotations"
- **THEN** the caption "Not for diagnostic use" appears at the lower edge of the picture

### Requirement: Print images
The viewer SHALL open a print window with ⌘P on macOS and Ctrl+P on Windows and Linux, which
prints the image of the active view, or all images of a fully loaded series in display
order, on A4 or Letter, portrait or landscape, with 1, 2, 4, 6, 9 or 12 images per page. The
pictures SHALL show the current contrast window, inversion and colour map in correct
proportions, with image number, orientation letters, scale bar when the pixel spacing is
known, and window values. The window SHALL show a preview of the first page and the number
of pages.

#### Scenario: A series on sheets
- **WHEN** a 60-image series is printed with 9 images per page
- **THEN** there are 7 pages, the preview says "Page 1 of 7", the first page holds images 1 to 9 and the last page images 55 to 60

#### Scenario: Non-square pixels
- **WHEN** an image of 256 columns × 128 rows with Pixel Spacing 1.0 \ 0.5 is printed
- **THEN** its picture on the page is square, with correct proportions

#### Scenario: The scale bar on paper
- **WHEN** a CT slice with a 50 mm scale bar is printed with 4 images per page
- **THEN** the scale bar, measured on the page, keeps its proportion to the picture and is labelled "50 mm"

#### Scenario: Series still loading
- **WHEN** the series is not fully loaded
- **THEN** only the current image can be printed, and the window says that the whole series can be printed when it has loaded

### Requirement: Printed pages are private by default
Printed pages and saved PDFs SHALL NOT contain the patient's name, ID or birth date unless
the user ticks "Include patient details" for that print job; the choice SHALL NOT be
remembered. Every page SHALL carry the study date, series description, page number and
"Not for diagnostic use". The print job's title, the PDF's metadata and the suggested file
name SHALL hold no patient details. The burned-in text warning SHALL apply to printing.

#### Scenario: Default print job
- **WHEN** the user prints without changing any option
- **THEN** the pages show no patient name, ID or birth date, the job title is `SumMRIse`, and the suggested PDF name has the form `SumMRIse-CT-S1.pdf`

#### Scenario: Details switched on once
- **WHEN** the user ticks "Include patient details" for one print job and opens the print window again
- **THEN** the first job's pages show the patient lines and the option is unticked again

#### Scenario: Picture with burned-in text
- **WHEN** the user prints a secondary-capture image
- **THEN** the warning that the picture itself may show personal details appears first, with "Print anyway" and "Cancel"

### Requirement: Print without temporary files
"Print" SHALL hand the pages as a PDF held in memory to the system's print system, on
standard input of `lp` where it exists, to the printer chosen in the print window, without
writing a file of its own. Where no such print system exists, the same button SHALL read
"Save PDF and open…", ask where to save the PDF and open it in the default application for
PDFs. "Save as PDF…" SHALL always be offered. A failure SHALL be reported with what the print
system said, and nothing SHALL be retried by itself.

#### Scenario: Printing on macOS or Linux with CUPS
- **WHEN** the system lists the printers "Office" (the default) and "Home" and the user chooses "Home" and 2 copies
- **THEN** `lp` receives the PDF on standard input with that printer, 2 copies and the title `SumMRIse`, and the application writes no file

#### Scenario: No print system
- **WHEN** `lp` does not exist on the computer, as on Windows
- **THEN** the button reads "Save PDF and open…" and the print window says that printing continues in the application that opens the PDF

#### Scenario: Printer refuses the job
- **WHEN** `lp` exits with an error
- **THEN** a message gives its error text, the print window stays open, and "Save as PDF…" is still available

