## ADDED Requirements

### Requirement: Plain-language series description
The application SHALL describe every series in plain language: one short line under the
series title in the series browser, and an "About this series" card in the inspector
with the same line as headline and statements about the kind of scan, the plane of the
slices in everyday words and, where recorded, the slice thickness, for MR whether the
images were measured slice by slice (2D) or as a block (3D), for CT the reconstruction
kernel as recorded and for CT and MR whether a contrast agent is recorded.

#### Scenario: CT chest series
- **WHEN** a CT series with Image Orientation (Patient) 1\0\0\0\1\0, Slice Thickness 1.25, Convolution Kernel "B30f" and Contrast/Bolus Agent "Iopromide" is selected
- **THEN** the browser line and the card headline read "CT scan", and the card states that the slices are axial (horizontal cross-sections), that each image shows a slice 1.25 mm thick, that the images were computed with the reconstruction kernel "B30f" as recorded by the scanner, and that a contrast agent is recorded for this series: Iopromide

#### Scenario: No contrast agent recorded
- **WHEN** a CT or MR series has no Contrast/Bolus Agent, or an empty one
- **THEN** the card states that no contrast agent is recorded for this series and that this does not rule one out

#### Scenario: Sagittal MR series
- **WHEN** an MR series with Image Orientation (Patient) 0\1\0\0\0\-1 is selected
- **THEN** the card states that the slices are sagittal: they divide the body into left and right, like a view from the side

#### Scenario: MR measured as a block
- **WHEN** an MR series records MR Acquisition Type 3D
- **THEN** the card states that a whole block of the body was measured at once (3D) and then divided into the slices shown

### Requirement: Special kinds of series
The description SHALL recognise localizers (Image Type LOCALIZER, or the words localizer,
localiser, scout, survey, topogram, scanogram, surview or loc in the series description or
protocol name), secondary captures (Secondary Capture SOP classes), dose report images
(a secondary capture whose description contains the word "dose"), PET, nuclear medicine,
ultrasound, X-ray projections, mammograms and X-ray fluoroscopy or angiography (by
modality), and SHALL say that it does not recognise the kind of images otherwise. Words
SHALL be matched as whole words.

#### Scenario: Three-plane localizer
- **WHEN** an MR series has Image Type ORIGINAL\PRIMARY\LOCALIZER and three images in different planes
- **THEN** the line reads "Localizer (planning overview)" and the card says that these are quick overview images used to plan the other series and that the images lie in different directions

#### Scenario: Dose report image
- **WHEN** a Secondary Capture series is described "Dose report"
- **THEN** the line reads "Dose report (a stored picture)" and the card says that it is a picture of the dose report table, not a scan of the body

#### Scenario: Scan named after its dose
- **WHEN** a CT image series is described "LOW DOSE CHEST"
- **THEN** it is described as a CT scan, not as a dose report

#### Scenario: Unrecognised modality
- **WHEN** a series has Modality OT and is not a secondary capture
- **THEN** the browser shows no line for it and the card says that the guide does not recognise what kind of images these are

### Requirement: MR weighting with honest uncertainty
For MR series the description SHALL state the weighting and what tends to appear bright
or dark with it. A weighting named in the series description or the protocol name SHALL
be reported as named by the scanner operator, quoting the name, and SHALL take
precedence over inference. A weighting inferred from the timing parameters SHALL be
worded "Likely …" and SHALL show the values it rests on. A recorded diffusion b-value
above 0 SHALL be reported as diffusion-weighted without "likely". When no rule applies
the description SHALL say that the weighting cannot be told, and why.

#### Scenario: Short repetition and echo time
- **WHEN** an unnamed spin-echo series (Scanning Sequence SE) has Repetition Time 500 and Echo Time 12
- **THEN** the line reads "MR scan, likely T1-weighted" and the basis shows "short repetition time (500 ms) and short echo time (12 ms)"

#### Scenario: Long repetition and echo time
- **WHEN** an unnamed spin-echo series has Repetition Time 4000 and Echo Time 100
- **THEN** the line reads "MR scan, likely T2-weighted" and the basis shows "long repetition time (4000 ms) and long echo time (100 ms)"

#### Scenario: Long repetition time and short echo time
- **WHEN** an unnamed spin-echo series has Repetition Time 2500 and Echo Time 20
- **THEN** the line reads "MR scan, likely proton-density-weighted"

#### Scenario: Long inversion time
- **WHEN** an unnamed inversion-recovery series has Repetition Time 9000, Echo Time 120 and Inversion Time 2500
- **THEN** the line reads "MR scan, likely T2 FLAIR (fluid suppressed)" and the basis shows the three values

#### Scenario: Short inversion time
- **WHEN** an unnamed inversion-recovery series has Repetition Time 4000, Echo Time 40 and Inversion Time 150
- **THEN** the line reads "MR scan, likely STIR (fat suppressed)" and the basis shows "a short inversion time (150 ms)"

#### Scenario: Gradient echo with a long echo and a small flip angle
- **WHEN** an unnamed gradient-echo series (Scanning Sequence GR) has Echo Time 20 and Flip Angle 20
- **THEN** the line reads "MR scan, likely T2*-weighted"

#### Scenario: Diffusion b-value
- **WHEN** an MR series records Diffusion b-value 1000
- **THEN** the line reads "MR scan, diffusion-weighted" and the basis reads "Recorded in the images: diffusion b-value 1000 s/mm²."

#### Scenario: Weighting named in the description
- **WHEN** an MR series is described "t1_mprage_sag", whatever its timing
- **THEN** the line reads "MR scan, T1-weighted", without "likely", and the basis reads "Named by the scanner operator: the series description reads “t1_mprage_sag”."

#### Scenario: Contradictory or missing values
- **WHEN** an unnamed spin-echo series has Repetition Time 500 and Echo Time 100, or records no Echo Time, or records no Scanning Sequence
- **THEN** the line reads "MR scan" and the card says that the weighting cannot be told from what is recorded, with the reason

#### Scenario: Gradient echo with a short echo time
- **WHEN** an unnamed gradient-echo series has Repetition Time 500 and Echo Time 12
- **THEN** it is not called T1-weighted: the card says that the timing does not settle the weighting

### Requirement: Statements carry their basis
Every statement of the card SHALL be shown with its basis: the attribute recorded in the
images, the name typed at the scanner, or the inference with its values. A card that
relies on acquisition settings SHALL say that they are read from one image of the series
and that other images of the series may differ.

#### Scenario: Basis of the plane
- **WHEN** the card states that the slices are axial
- **THEN** the line beneath it reads "Recorded in the images: image orientation."

### Requirement: No interpretation
The guide SHALL only describe how images were made and what kinds of tissue tend to
appear bright or dark with a technique. It SHALL NOT state what is normal or abnormal,
name a finding or a disease, or mention diagnosis or treatment. The card SHALL show the
fixed line "General information about how these images were made, not an interpretation
of the images." Explanations SHALL be one or two sentences.

#### Scenario: Texts are checked against interpretive words
- **WHEN** the tests run over every explanation and over the descriptions of the synthetic series
- **THEN** each text is non-empty, has at most two sentences and contains none of the words normal, abnormal, tumour, lesion, disease, diagnosis and the other words on the list

### Requirement: Contrast preset explanations
Each entry of the contrast preset menu SHALL say in its tooltip what the window is for
and which range of values it spreads the grey scale over. Built-in CT windows SHALL be
recognised by their width and level, not by their name.

#### Scenario: Lung window
- **WHEN** the user points at the built-in Lung preset (W 1500 / L −600) of a CT series
- **THEN** the tooltip says that the window is centred on low densities so that the air-filled lungs with their vessels and airways become visible, and that it spreads the grey scale over densities from −1350 to 150 HU

#### Scenario: Stored window of an MR series
- **WHEN** the user points at a stored window with centre 700 and width 1400 of an MR series
- **THEN** the tooltip says that the window was stored with the images and spreads the grey scale over values from 0 to 1400

### Requirement: Viewing direction
The inspector SHALL say from where the body is seen in the image of the active view and
which side of the body is on the left of the screen, computed from the patient-space
directions of screen-right and screen-down of that image. Directions more than 10° off a
patient axis SHALL be worded as approximate ("roughly", "tilted").

#### Scenario: Standard axial image
- **WHEN** the image on screen has Image Orientation (Patient) 1\0\0\0\1\0
- **THEN** the text reads "You are looking from the feet towards the head." and "The patient's right side is on the left of the screen."

#### Scenario: Sagittal image
- **WHEN** the image on screen has Image Orientation (Patient) 0\1\0\0\0\-1
- **THEN** the text reads "You are looking from the patient's left side towards the right." and "The front of the body is on the left of the screen."

#### Scenario: Coronal reformat
- **WHEN** the coronal reformat (screen-right towards patient left, screen-down towards the feet) is the active view
- **THEN** the text reads "You are looking from the front of the body towards the back." and "The patient's right side is on the left of the screen."

#### Scenario: Mirrored axial image
- **WHEN** an acquired image has Image Orientation (Patient) −1\0\0\0\1\0 and is displayed as stored
- **THEN** the text reads "You are looking from the head towards the feet." and "The patient's left side is on the left of the screen."

#### Scenario: Oblique plane
- **WHEN** the image plane is tilted 45° between axial and coronal
- **THEN** the title reads "Tilted (oblique) image" and the text reads "The image is tilted: you are looking roughly from the feet towards the head."

### Requirement: Position figure
The inspector SHALL show a schematic figure of where the image of the active view lies.
When Body Part Examined is a recognised value, the figure SHALL be a front view of a body
outline with the patient's right on the left, the examined region highlighted, and the
image drawn within the scanned block fitted into that region: as a line when the image
is seen edge-on and as a band otherwise. Otherwise the figure SHALL be the scanned block
with the image inside it and the letters R, L, A, P, S and I at its faces. The figure
SHALL be captioned as schematic and not to scale. For series without position
information the inspector SHALL say that the figure cannot show where the image lies.

#### Scenario: Axial slice of a chest CT
- **WHEN** a CT series with Body Part Examined CHEST covers z = −100 mm to +100 mm and the axial image at z = +50 mm is on screen
- **THEN** the chest of the outline is highlighted, R is marked on the left and L on the right, and a horizontal line lies three quarters of the way up the scanned block

#### Scenario: Sagittal slice
- **WHEN** a sagittal image at the patient's left end of the scanned block is on screen
- **THEN** the image is a vertical line at the right-hand end of the block in the figure

#### Scenario: Coronal image
- **WHEN** a coronal image is on screen
- **THEN** the image is drawn as a band over the front of the scanned block

#### Scenario: Unknown body part
- **WHEN** the series records no Body Part Examined, or a value that is not recognised
- **THEN** the figure is the scanned block with the image as a sheet inside it, with R on the left, L on the right, S at the top, I at the bottom, A at the front and P at the back, and no body outline

#### Scenario: Paired region
- **WHEN** Body Part Examined is KNEE
- **THEN** both knees are highlighted and the caption says that the images do not say which side was examined

#### Scenario: Stored picture
- **WHEN** a secondary capture without Image Position (Patient) is selected
- **THEN** no figure is drawn and the text says that the images carry no position information

### Requirement: Show explanations setting
A setting "Show explanations" SHALL be on by default, SHALL persist with the other
settings and, when off, SHALL hide the browser line, the card, the figure with its text,
the info marks of the inspector rows and the preset explanations.

#### Scenario: Switched off
- **WHEN** the user unticks "Show explanations of series, values and the viewing position" in the settings window
- **THEN** the browser, the inspector and the preset menu look as they did without the guide

#### Scenario: Settings stored by an earlier version
- **WHEN** stored settings do not contain the field
- **THEN** explanations are shown

### Requirement: Guide without patient data
The guide SHALL be derived from series, acquisition and geometry attributes only and
SHALL NOT read or show the patient's name, ID, birth date, sex or age, so that it is the
same with privacy mode on or off.

#### Scenario: Privacy mode
- **WHEN** privacy mode is switched on for a study of patient `PHANTOM^SYNTHETIC`, ID `SYN-0001`, born 1970-01-01
- **THEN** the card, the browser line and the figure are unchanged, and none of their texts contains the name, the ID or the birth date
