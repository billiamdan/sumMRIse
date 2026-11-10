## Decisions

- **The LGPL protocol definition stays, and is declared.** `wayland-protocols-plasma` is
  MIT, but code generated from KDE's `blur.xml` (LGPL-2.1-or-later) is linked into every
  winit application on Linux. The notices name it and give the text; SumMRIse's source is
  public, so the application can be rebuilt with a changed version. *Rejected:* patching
  winit to drop KDE blur — a fork to maintain for a feature SumMRIse does not call.
- **Recognise licences by their own words.** MIT by "obtaining a copy of this software and
  associated documentation files", Apache by the heading of its terms; a file that only
  mentions a licence is a notice and is always kept. *Rejected:* trusting file names alone —
  `COPYING` and `COPYRIGHT` files carry the copyright lines.
- **The standard library gets the MIT text and a pointer** to the toolchain's own list of
  notices (`COPYRIGHT-library.html`, 1.5 MB of HTML), rather than a copy of that list.
- **Dead worker, not a new job type.** `Job::is_finished()` after an empty `poll()` already
  means that the thread ended without a result (the export window used it); the scan, the
  image information and the printer search now do the same. The print preview and the 3D
  refinement keep their last picture: starting them again would repeat the fault.
- **Paths at debug level.** The path of what was opened is useful to someone looking for a
  fault, so it is kept at debug level, which the default filter does not show.
- **The scroll position of the licences window is set, not animated**, so Home and End
  reach the ends of ten thousand lines at once.
