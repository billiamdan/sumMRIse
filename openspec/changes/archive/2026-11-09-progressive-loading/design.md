## Context

`summrise_dicom::load_series` decodes all instances of a series in parallel and returns
a complete `LoadedSeries` (frames or volume, display order, value range, initial
window). `Session` runs it as one background job and the viewer shows a spinner until it
finishes. The index already holds, per frame, the image plane and the instance number
read from the headers, which is everything needed to order the images and to validate
the volume geometry before any pixel data is read.

## Goals / Non-Goals

**Goals:**
- Time to the first image is the time to read and decode one file.
- Scrolling stays responsive during loading.
- The final state is identical to today's.

**Non-Goals:**
- A cache on disk.
- Low-resolution previews (progressive JPEG 2000 layers).
- Pre-loading series that are not displayed (possible follow-up).
- Loading over a network.

## Decisions

### D1 — Plan from headers
A new `plan_series` computes the display order and the volume-geometry verdict from the
indexed frame planes and instance numbers. Decoding then only fills in pixels.

Alternative considered: keep determining the order after decoding — rejected: the order
would be unknown while loading and images could change place.

### D2 — Write-once slots
The series data is a vector of write-once slots (one per displayed image) shared between
the loader and the UI. Workers fill slots; the UI reads without locking; a failed image
stores its reason.

Alternatives considered:
- Publishing a new immutable snapshot every few images — rejected: copying or complex
  structural sharing.
- A read-write lock around a vector of options — rejected: the UI thread would contend
  with the workers.

### D3 — Prioritised loader
One coordinator per loading series holds the index of the displayed image and hands out
work nearest to it, in runs of consecutive images so that reads stay mostly sequential
on optical media. A single thread reads file bytes; decoding runs in parallel from
memory.

Alternative considered: parallel iteration in display order — rejected: seeks on optical
drives and no way to re-prioritise.

### D4 — Stable window and value range
The initial window is the first stored DICOM window of the first decoded image, or an
automatic window computed from that image; it is never recomputed on its own (the "Auto"
preset recomputes on request). Until loading completes, the value range used for drag
sensitivity is estimated from Bits Stored, Pixel Representation and the rescale values;
the "Full range" preset becomes available on completion.

### D5 — Volume on completion
The volume is assembled when the last slot is filled and the header verdict was
positive; reformats switch on at that moment. If an image failed to decode, the verdict
becomes "irregular spacing", as today.

### D6 — Pause, resume and eviction
Selecting another series pauses the coordinator after the image in progress. Filled
slots are kept, so returning resumes where loading stopped. The memory budget evicts a
paused series as a whole, never partly.

### D7 — Multi-frame files
A file with several frames is decoded as one unit and fills all its slots.

## Risks / Trade-offs

- [Concurrency errors] → one coordinator owns scheduling, slots are write-once, and tests
  use an artificially slow source.
- [Seeking on optical drives] → runs of consecutive files and a single reader; measured
  on a real drive as part of `field-validation`.
- [The user scrolls faster than loading] → placeholders with progress; scrolling never
  blocks.
- [An automatic window from one image is unrepresentative] → stored windows are used
  when present, which is usual for CT and MR; "Auto" recomputes from loaded images.
