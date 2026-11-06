# DICOM Conformance Statement — SumMRIse

| | |
| --- | --- |
| Product | SumMRIse, a desktop viewer for the image discs that hospitals hand to patients |
| Version | 0.2 (this statement describes the code it is kept with) |
| Date | 2026-11-06 |
| Platforms | macOS 11 or later, Windows 10 or later, Linux (X11 and Wayland) |

SumMRIse is **not a medical device** and is not intended for diagnosis. This statement
follows the layout of DICOM PS3.2 (Annex A), reduced to what applies to a viewer that
only reads files: it has no network services and creates no DICOM objects.

## 1 Conformance Statement Overview

SumMRIse reads DICOM media — CDs, DVDs, USB drives and folders, as well as ZIP archives
and ISO 9660 images of such media — and displays the images on them. It acts as a
File-set Reader only. It neither writes nor changes DICOM files and has no DICOM network
functions.

### Network services

| Networking SOP class | User of service (SCU) | Provider of service (SCP) |
| --- | --- | --- |
| None | — | — |

SumMRIse supports no DICOM network service (no storage, query/retrieve, worklist,
print or DICOMweb) and opens no network connection.

### Media services

| Media storage application profile | Write files (FSC or FSU) | Read files (FSR) |
| --- | --- | --- |
| General Purpose CD-R Interchange (STD-GEN-CD) | No | Yes |
| General Purpose Interchange on DVD (STD-GEN-DVD-JPEG, STD-GEN-DVD-J2K) | No | Yes |
| General Purpose USB and Flash Memory with Compression (STD-GEN-USB-JPEG, STD-GEN-USB-J2K) | No | Yes |
| Files in a folder, a ZIP archive or an ISO 9660 image (not a DICOM profile) | No | Yes |

File-set Creator (FSC) and File-set Updater (FSU) are not supported.

## 2 Table of Contents

1. Conformance Statement Overview
2. Table of Contents
3. Introduction
4. Networking
5. Media Interchange
6. Support of Character Sets
7. Security
8. Annexes

## 3 Introduction

### 3.1 Revision history

| Version | Date | Changes |
| --- | --- | --- |
| 1 | 2026-11-06 | First statement (OpenSpec change `display-fidelity`) |

### 3.2 Audience

People who want to know which DICOM files SumMRIse can show, and how: patients and
their relatives who receive a disc, hospital staff who produce discs, and developers.
Familiarity with DICOM terms is assumed in sections 5 to 8.

### 3.3 Remarks

- SumMRIse is not a medical device. It shows images for information; it is not suitable
  for diagnosis, and it makes no statement about what images show.
- The statement describes the behaviour of the source code it is kept with. Its
  transfer-syntax table is checked against the code by an automated test
  (`crates/dicom/tests/conformance.rs`).
- Reading a file is not the same as showing every part of it. Section 8.1 lists what is
  read and what is ignored.

### 3.4 Terms and definitions

Terms are those of DICOM PS3.1 to PS3.20. "Scanner graphics" is the interface's name for
overlay planes (groups 60xx). "Contrast table" is its name for a table of the VOI LUT
Sequence.

### 3.5 Basics of DICOM communication

Not applicable: SumMRIse does not communicate over a network.

### 3.6 Abbreviations

AE Application Entity · FSC File-set Creator · FSR File-set Reader · FSU File-set Updater ·
IOD Information Object Definition · LUT Lookup Table · SOP Service-Object Pair ·
VOI Value of Interest

### 3.7 References

DICOM PS3.2, PS3.3, PS3.4, PS3.5, PS3.10, PS3.11, PS3.12 and PS3.15 (2024 editions).

## 4 Networking

Not supported. SumMRIse implements no DICOM network service and no association
negotiation; it has no Application Entity title, port or presentation context. It opens
no network connection of any kind, and nothing it reads is sent anywhere.

## 5 Media Interchange

### 5.1 Implementation model

#### 5.1.1 Application data flow

The user opens a disc, a folder, a single file, a ZIP archive or an ISO 9660 image.
SumMRIse finds the DICOM files, reads their headers, groups them into patients' studies
and series, and decodes and displays the images of the series the user selects. All of
it happens in memory: no decoded image, thumbnail, extracted file or other data derived
from patient data is written to disk.

#### 5.1.2 Functional definition of the Application Entity

One Application Entity, **SumMRIse**, with the File-set Reader role:

- When the medium has a `DICOMDIR`, the files it references are read (directory records
  of the types PATIENT, STUDY, SERIES and IMAGE and the other image-level records); files
  not in the `DICOMDIR` are not read. Without a `DICOMDIR`, the folder is scanned and
  every file with the 128-byte preamble and the `DICM` prefix of PS3.10 is read.
- ZIP archives and ISO 9660 images are read in memory, never unpacked; they are searched
  the same way.

#### 5.1.3 Sequencing of real-world activities

Headers are indexed first. Images of the selected series are then decoded, the one shown
first, its neighbours next, so that the series can be viewed while it loads.

#### 5.1.4 File meta information

SumMRIse creates no files, so it has no Implementation Class UID of its own to write. It
reads the File Meta Information of every file (Transfer Syntax UID, Media Storage SOP
Class UID) and shows it in the attribute list.

### 5.2 AE specifications

The AE **SumMRIse** supports the application profiles of section 1 in the FSR role.
Discs and drives are read through the operating system, so any file system it mounts
(ISO 9660, UDF, FAT, exFAT, NTFS, APFS, HFS+, ext4 …) can be read. Disc image files are
read by SumMRIse itself: ISO 9660 with names from the Joliet tree when present (Rock
Ridge names are not read); images that hold only UDF are reported as such.

### 5.3 Augmented and private application profiles

No private application profile. Reading ZIP archives (stored, deflate and deflate64
entries) and ISO 9660 images directly is an addition outside the DICOM profiles: such
downloads of patient discs are common. Password-protected archives, other compression
methods, entries larger than 4 GB, archives nested in archives and other container
formats (7z, RAR, tar …) are refused with a message saying what to do instead.

### 5.4 Media configuration

None.

### 5.5 Supported SOP classes

Any SOP class whose objects hold an image (Rows, Columns and Pixel Data) is displayed, as
far as its transfer syntax and photometric interpretation are supported. These are the
classes found on patient discs; those marked "tested" are covered by synthetic test data.

| SOP class | SOP class UID | Displayed | Tested |
| --- | --- | --- | --- |
| CT Image Storage | 1.2.840.10008.5.1.4.1.1.2 | Yes | Yes |
| Enhanced CT Image Storage | 1.2.840.10008.5.1.4.1.1.2.1 | Yes | No |
| MR Image Storage | 1.2.840.10008.5.1.4.1.1.4 | Yes | Yes |
| Enhanced MR Image Storage | 1.2.840.10008.5.1.4.1.1.4.1 | Yes | Yes |
| Computed Radiography Image Storage | 1.2.840.10008.5.1.4.1.1.1 | Yes | Yes |
| Digital X-Ray Image Storage – For Presentation | 1.2.840.10008.5.1.4.1.1.1.1 | Yes | Yes |
| Digital Mammography X-Ray Image Storage – For Presentation | 1.2.840.10008.5.1.4.1.1.1.2 | Yes | No |
| X-Ray Angiographic Image Storage | 1.2.840.10008.5.1.4.1.1.12.1 | Yes | Yes |
| X-Ray Radiofluoroscopic Image Storage | 1.2.840.10008.5.1.4.1.1.12.2 | Yes | No |
| Ultrasound Image Storage | 1.2.840.10008.5.1.4.1.1.6.1 | Yes | Yes |
| Ultrasound Multi-frame Image Storage | 1.2.840.10008.5.1.4.1.1.3.1 | Yes | No |
| Nuclear Medicine Image Storage | 1.2.840.10008.5.1.4.1.1.20 | Yes | No |
| Positron Emission Tomography Image Storage | 1.2.840.10008.5.1.4.1.1.128 | Yes | No |
| Secondary Capture Image Storage | 1.2.840.10008.5.1.4.1.1.7 | Yes | Yes |
| Multi-frame Grayscale Byte / Word Secondary Capture Image Storage | 1.2.840.10008.5.1.4.1.1.7.2, 1.2.840.10008.5.1.4.1.1.7.3 | Yes | No |
| Multi-frame True Color Secondary Capture Image Storage | 1.2.840.10008.5.1.4.1.1.7.4 | Yes | No |

Objects without pixel data — structured reports, presentation states, key object
selections, encapsulated PDF and others — are counted ("non-image objects are not
shown") and not displayed. "For Processing" X-ray images are displayed like "For
Presentation" ones, without the processing a workstation would apply.

### 5.6 Transfer syntaxes

Every transfer syntax known to the DICOM library (dicom-rs 0.10) is listed. "Decoded"
says whether this build decodes pixel data in it; "Always lossy" whether the
compression always discards detail — such images, and images whose Lossy Image
Compression (0028,2110) is 01, carry a "Lossy" mark on screen. JPEG 2000, HTJ2K and JPEG
XL can be lossless or lossy; for those only Lossy Image Compression tells.

| Transfer syntax | UID | Decoded | Always lossy |
| --- | --- | --- | --- |
| Implicit VR Little Endian | `1.2.840.10008.1.2` | Yes | No |
| Explicit VR Little Endian | `1.2.840.10008.1.2.1` | Yes | No |
| Encapsulated Uncompressed Explicit VR Little Endian | `1.2.840.10008.1.2.1.98` | Yes | No |
| Deflated Explicit VR Little Endian | `1.2.840.10008.1.2.1.99` | Yes | No |
| Explicit VR Big Endian (retired) | `1.2.840.10008.1.2.2` | Yes | No |
| JPEG Baseline (Process 1) | `1.2.840.10008.1.2.4.50` | Yes | Yes |
| JPEG Extended (Process 2 & 4) | `1.2.840.10008.1.2.4.51` | Yes | Yes |
| JPEG Lossless, Non-Hierarchical (Process 14) | `1.2.840.10008.1.2.4.57` | Yes | No |
| JPEG Lossless, Non-Hierarchical, First-Order Prediction | `1.2.840.10008.1.2.4.70` | Yes | No |
| JPEG-LS Lossless Image Compression | `1.2.840.10008.1.2.4.80` | With the jpeg-ls build option | No |
| JPEG-LS Lossy (Near-Lossless) Image Compression | `1.2.840.10008.1.2.4.81` | With the jpeg-ls build option | Yes |
| JPEG 2000 Image Compression (Lossless Only) | `1.2.840.10008.1.2.4.90` | Yes | No |
| JPEG 2000 Image Compression | `1.2.840.10008.1.2.4.91` | Yes | No |
| JPEG 2000 Part 2 Multi-component Image Compression (Lossless Only) | `1.2.840.10008.1.2.4.92` | Yes | No |
| JPEG 2000 Part 2 Multi-component Image Compression | `1.2.840.10008.1.2.4.93` | Yes | No |
| JPIP Referenced | `1.2.840.10008.1.2.4.94` | No | No |
| JPIP Referenced Deflate | `1.2.840.10008.1.2.4.95` | Yes | No |
| MPEG2 Main Profile / Main Level | `1.2.840.10008.1.2.4.100` | No | Yes |
| Fragmentable MPEG2 Main Profile / Main Level | `1.2.840.10008.1.2.4.100.1` | No | Yes |
| MPEG2 Main Profile / High Level | `1.2.840.10008.1.2.4.101` | No | Yes |
| Fragmentable MPEG2 Main Profile / High Level | `1.2.840.10008.1.2.4.101.1` | No | Yes |
| MPEG-4 AVC/H.264 High Profile / Level 4.1 | `1.2.840.10008.1.2.4.102` | No | Yes |
| Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.1 | `1.2.840.10008.1.2.4.102.1` | No | Yes |
| MPEG-4 AVC/H.264 BD-Compatible High Profile / Level 4.1 | `1.2.840.10008.1.2.4.103` | No | Yes |
| Fragmentable MPEG-4 AVC/H.264 BD-Compatible High Profile / Level 4.1 | `1.2.840.10008.1.2.4.103.1` | No | Yes |
| MPEG-4 AVC/H.264 High Profile / Level 4.2 For 2D Video | `1.2.840.10008.1.2.4.104` | No | Yes |
| Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.2 For 2D Video | `1.2.840.10008.1.2.4.104.1` | No | Yes |
| MPEG-4 AVC/H.264 High Profile / Level 4.2 For 3D Video | `1.2.840.10008.1.2.4.105` | No | Yes |
| Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.2 For 3D Video | `1.2.840.10008.1.2.4.105.1` | No | Yes |
| MPEG-4 AVC/H.264 Stereo High Profile / Level 4.2 | `1.2.840.10008.1.2.4.106` | No | Yes |
| Fragmentable MPEG-4 AVC/H.264 Stereo High Profile / Level 4.2 | `1.2.840.10008.1.2.4.106.1` | No | Yes |
| HEVC/H.265 Main Profile / Level 5.1 | `1.2.840.10008.1.2.4.107` | No | Yes |
| HEVC/H.265 Main 10 Profile / Level 5.1 | `1.2.840.10008.1.2.4.108` | No | Yes |
| JPEG XL Lossless | `1.2.840.10008.1.2.4.110` | With the jpeg-xl build option | No |
| JPEG XL JPEG Recompression | `1.2.840.10008.1.2.4.111` | With the jpeg-xl build option | Yes |
| JPEG XL | `1.2.840.10008.1.2.4.112` | With the jpeg-xl build option | No |
| High-Throughput JPEG 2000 Image Compression (Lossless Only) | `1.2.840.10008.1.2.4.201` | Yes | No |
| High-Throughput JPEG 2000 with RPCL Options Image Compression (Lossless Only) | `1.2.840.10008.1.2.4.202` | Yes | No |
| High-Throughput JPEG 2000 Image Compression | `1.2.840.10008.1.2.4.203` | Yes | No |
| JPIP HTJ2K Referenced | `1.2.840.10008.1.2.4.204` | No | No |
| JPIP HTJ2K Referenced Deflate | `1.2.840.10008.1.2.4.205` | Yes | No |
| RLE Lossless | `1.2.840.10008.1.2.5` | Yes | No |
| SMPTE ST 2110-20 Uncompressed Progressive Active Video | `1.2.840.10008.1.2.7.1` | No | No |
| SMPTE ST 2110-20 Uncompressed Interlaced Active Video | `1.2.840.10008.1.2.7.2` | No | No |
| SMPTE ST 2110-30 PCM Digital Audio | `1.2.840.10008.1.2.7.3` | No | No |
| Deflated Image Frame Compression | `1.2.840.10008.1.2.8.1` | Yes | No |

Remarks:

- The released builds do not include the jpeg-ls and jpeg-xl build options (the first
  needs a C++ toolchain); series stored in those transfer syntaxes are listed with the
  name of the transfer syntax and a "not supported by this build" notice.
- JPIP Referenced Deflate data sets can be read, but they hold no pixel data (the pixels
  are on a JPIP server, which SumMRIse never contacts): their images are reported as not
  decodable.
- Retired lossy JPEG processes that dicom-rs does not register (1.2.840.10008.1.2.4.52 to
  .56 and .59 to .64) are not decoded; were they present, they would be marked lossy.

### 5.7 Photometric interpretations and pixel data

| Photometric interpretation | Supported | Notes |
| --- | --- | --- |
| MONOCHROME2 | Yes | 1, 8 or 16 bits allocated |
| MONOCHROME1 | Yes | Opens inverted (smallest value white) |
| PALETTE COLOR | Yes | Red, Green and Blue Palette Color Lookup Tables with 8- or 16-bit entries; segmented palettes are reported as not supported |
| RGB | Yes | 8 bits, planar configuration 0 or 1 |
| YBR_FULL, YBR_FULL_422 | Yes | Converted to RGB (8 bits) |
| Colour in compressed transfer syntaxes (YBR_ICT, YBR_RCT, YBR_PARTIAL_420 …) | Yes | Converted to RGB by the decoder |
| YBR_PARTIAL_422 and other colour spaces in uncompressed data | No | Reported as not supported |
| Supplemental Palette Color (grey-scale with colour ranges) | Partly | Shown in grey; the colour ranges are ignored |

Grey-scale samples are reduced to Bits Stored (shifted by High Bit + 1 − Bits Stored and
sign-extended when Pixel Representation is 1) before any other step, so bits outside Bits
Stored — overlay planes of the retired form, or unused bits — never change a value.
32-bit grey-scale and 16-bit colour images are not supported.

## 6 Support of Character Sets

Text values are decoded with the first value of Specific Character Set (0008,0005).
Supported: the default repertoire (ISO_IR 6), ISO_IR 100 (Latin-1), ISO_IR 101
(Latin-2), ISO_IR 109 (Latin-3), ISO_IR 110 (Latin-4), ISO_IR 144 (Cyrillic), ISO_IR 127
(Arabic), ISO_IR 126 (Greek), ISO_IR 138 (Hebrew), ISO_IR 166 (Thai), ISO_IR 13 and
ISO 2022 IR 87 (Japanese), ISO_IR 149 (Korean), ISO_IR 192 (UTF-8), GB18030 and GBK
(Chinese).

Code extensions with several values of Specific Character Set (ISO 2022 escape sequences
switching between repertoires inside a value) are not interpreted beyond the first
value. Values in a character set that is not supported are decoded with the default
repertoire. Person Name values are shown from their alphabetic component group, in the
order prefix, given, middle, family name, suffix.

## 7 Security

### 7.1 Security profiles

No DICOM security profile is supported. There is no network and no file is written, so
none applies.

### 7.2 Association-level security

Not applicable.

### 7.3 Application-level security and privacy

- No network access of any kind.
- Nothing derived from patient data is written to disk: no caches, temporary files,
  thumbnails or extracted archive entries. Archives and disc images are read in memory.
- The list of recently opened folders stores the path, the study description and date
  and the modalities only — never names, IDs or birth dates; it can be switched off.
- Patient-identifying values are never written to logs above debug level.
- Privacy mode hides the patient's name, ID and birth date everywhere in the interface.
  When an image records Burned In Annotation (0028,0301) as YES, privacy mode says that
  patient details may be part of the picture itself and cannot be hidden.
- Malformed files yield an error message for the image concerned, never a crash. ZIP
  archives and disc images are treated as untrusted: offsets and sizes are checked,
  directories are read once, and depth and counts are bounded.

## 8 Annexes

### 8.1 IOD contents

#### 8.1.1 Created SOP instances

None. SumMRIse creates no DICOM objects.

#### 8.1.2 Usage of attributes from received IODs

Attributes read and used (all others are shown in the attribute list of the inspector
only):

| Module or group | Attributes | Use |
| --- | --- | --- |
| Patient | Patient's Name, ID, Birth Date, Sex, Age, Weight, Size | Shown (hidden in privacy mode) |
| General Study | Study Instance UID, Date, Time, Description, Accession Number, Referring Physician's Name; Institution Name | Grouping, shown |
| General Series | Series Instance UID, Number, Description, Modality, Body Part Examined, Protocol Name, Patient Position, Laterality | Grouping, shown, plain-language guide |
| Image Plane, Plane Position/Orientation (also in Enhanced functional groups) | Image Position (Patient), Image Orientation (Patient), Pixel Spacing, Slice Thickness, Spacing Between Slices, Imager Pixel Spacing | Ordering, volumes, orientation letters, reformats, scale bar |
| Image Pixel | Samples per Pixel, Photometric Interpretation, Rows, Columns, Bits Allocated, Bits Stored, High Bit, Pixel Representation, Planar Configuration, Pixel Data, Pixel Padding Value, Pixel Padding Range Limit | Decoding; padding is left out of the automatic window, the value range and reformats, drawn black and read as "Outside the scanned area" |
| Palette Color Lookup Table | Red, Green, Blue Palette Color Lookup Table Descriptor and Data | Palette colour |
| Modality LUT | Rescale Intercept, Rescale Slope, Rescale Type (also in Pixel Value Transformation functional groups), Modality LUT Sequence (LUT Descriptor, Modality LUT Type, LUT Data) | Modality values; the Modality LUT Sequence replaces rescaling when present |
| VOI LUT | Window Center, Window Width, Window Center & Width Explanation, VOI LUT Function (also in Frame VOI LUT functional groups), VOI LUT Sequence (LUT Descriptor, LUT Explanation, LUT Data) | Contrast presets; each table is a contrast choice, the first one the default when no window is stored; tables apply to the acquired images, reformats use windows |
| Presentation LUT | Presentation LUT Shape | INVERSE opens the images inverted, like MONOCHROME1 |
| Overlay Plane (groups 6000–601E) | Overlay Rows, Columns, Type, Origin, Bits Allocated, Bit Position, Data, Number of Frames in Overlay, Image Frame Origin | Drawn over the acquired images as "scanner graphics", on by default, switched with G; both the Overlay Data form and the retired form in unused pixel bits |
| General Image | Instance Number, Image Type, Lossy Image Compression, Lossy Image Compression Ratio and Method, Burned In Annotation | Ordering; lossy mark on the image ("Lossy 10:1 (JPEG)"); burned-in notice in privacy mode |
| Multi-frame, Cine | Number of Frames, Frame Time, Cine Rate, Recommended Display Frame Rate | Frames, playback speed |
| MR, CT and Enhanced acquisition attributes | Repetition, Echo and Inversion Time, Flip Angle, Scanning Sequence and Variant, Sequence Name, Echo Numbers, Temporal Position Identifier, Trigger Time, Diffusion b-value, KVP, Convolution Kernel, Contrast/Bolus Agent and others | Shown, plain-language guide, splitting a series into stacks |
| File Meta Information, DICOMDIR | Transfer Syntax UID, Media Storage SOP Class UID; Directory Record Sequence, Referenced File ID | Decoding, discovery |

Not used: presentation states (GSPS and others), curve data (groups 50xx), waveforms,
segmentations and RT structures as overlays, ICC profiles (colour images are shown as
stored, without colour management), Supplemental Palette Color ranges, segmented palette
tables, bitmap overlays from other objects, Shutter attributes, Icon Image Sequence,
private attributes (listed in the attribute list as "Private (creator)").

#### 8.1.3 Attribute mapping

Not applicable.

#### 8.1.4 Coerced or modified fields

None: files are never changed.

### 8.2 Data dictionary of private attributes

None.

### 8.3 Coded terminology and templates

None.

### 8.4 Grayscale image consistency

SumMRIse does not implement the Grayscale Standard Display Function (PS3.14) and does
not calibrate the screen. Grey levels are sent to the operating system as 8-bit values.
A built-in display check (in the settings window and the About dialog) shows grey steps
from 0 to 100 %, a 5 % patch inside black and a 95 % patch inside white, a continuous
ramp, fine line pairs and a border, with instructions. It is a quick visual check in the
style of the SMPTE test pattern, not a calibration and not a statement of compliance with
any standard.

### 8.5 Standard extended, specialized and private SOP classes

None.

### 8.6 Private transfer syntaxes

None.
