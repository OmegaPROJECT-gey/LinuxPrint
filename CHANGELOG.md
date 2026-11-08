# Changelog

All notable changes to LinuxPrinterG are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
versions follow [Semantic Versioning 2.0.0](https://semver.org/) — see
[VERSIONING.md](VERSIONING.md).

---

## [Unreleased] — 2026-11-08

### Added

- **Photo auto-fit** — new module `services/fit.rs`: the paper format is picked
  from the aspect ratio of the shot, `print-scaling fill` is used with the crop
  calculated in millimetres, landscape orientation is chosen when the photo is
  wider than the sheet, and the resulting plan (format, orientation, crop) is
  shown as a caption under the preview.

## [0.1.0] — 2026-10-20

First release.

### Added

- **Printing**: file picker and drag-and-drop (PDF and images directly; WebP is
  assembled into a PDF through the image encoder; DOCX/ODT/RTF through headless
  LibreOffice when it is installed, otherwise a clear hint), preview of the
  first PDF page via `pdftoppm`, copies, page ranges, and paper size /
  orientation / margins / duplex / quality / tray / media lists taken from the
  printer's real `lpoptions` choices.
- **Own IPP client** (`services/ipp.rs`, HTTP `application/ipp` to the local
  cupsd): exact job states with a `lpstat` fallback, «sheet X of Y» progress
  from `job-media-sheets`, Hold/Release/Restart-Job operations, RFC 8010
  encoding written by hand, and a live test against a running CUPS.
- **Job completion notifications** — a watcher polls active jobs every 3 s and,
  on a transition to a terminal state, sends a freedesktop notification
  (D-Bus via `gio::Notification`) and writes a journal entry; the active-jobs
  count is a badge in the sidebar.
- **Deferred printing** — «Now / Day / Evening / Night / Weekend» through
  `job-hold-until`, with named values checked against the printer's real
  options.
- **Journal: CSV export** (RFC 4180, opens in LibreOffice/Excel), a category
  filter and a text export; clearing the journal still asks for confirmation.
- **gettext translations uk, de, fr** — own PO/MO reader-writer
  (`core/src/po.rs`), `make po` / `make po-build`, overlay loading at startup
  with a fallback to English for empty or fuzzy entries; the languages are
  selectable in Settings and are shipped inside .deb/.rpm/Flatpak.
- **Photo preset «4×6 / 10×15»** — the photo profile plus a sheet size; the
  `4x6` / `10x15` / `100x150` aliases are matched against the printer's real
  media list, and when the printer has no such format the app says so instead
  of silently doing nothing.
- **ICC profiles** — driver profiles read from the printer's PPD
  (`*cupsICCProfile`, downloaded with an anonymous GET so it works without
  membership in the `lp` group), shown with a note that CUPS applies them
  automatically and that neither CUPS nor IPP accepts an arbitrary ICC file.
- **DNS-SD discovery** — Mopria printers (`_ipp/_ipps._tcp`) and AirScan/eSCL
  scanners (`_uscan/_uscans._tcp`) through `avahi-browse -rtp` in parallel
  threads with deduplication, listed as a separate «Found on the network»
  group; a missing Avahi produces a hint, not an error.
- **Multi-page TIFF** — own writer (IFD chain + Adobe Deflate, verified
  against libtiff and Pillow), and WebP pages merged into a PDF before the job
  is submitted to CUPS.
- **Own PDF writer** for scans: Flate for the raster, DCTDecode for JPEG, no
  re-encoding; multi-page PDFs with `{page}` in the name when the format does
  not support several pages.
- **Scan progress** page by page and by percentage (`scanimage --progress`
  read as a stream from stderr), cancellation at any moment, crop in
  millimetres with clipping to the device bounds and rotation 0/90/180/270°.
- **OCR through Tesseract** with a language choice (the language is sanitized,
  a separate `.txt` is produced).
- **Storage and logging** — JSON in XDG directories with atomic writes, a
  journal capped at 1000 entries, `tracing` to stderr and to a daily-rotated
  file with a configurable level.

### Security

- external commands are executed without a shell, with arguments passed as a
  vector; `LC_ALL=C` for deterministic parsing; hard timeouts, kill and user
  cancellation; file names and the OCR language are sanitized and output paths
  are confined with `naming::safe_join`; no root is required anywhere.

[Unreleased]: https://github.com/linuxprinterg/linuxprinterg/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/linuxprinterg/linuxprinterg/releases/tag/v0.1.0

---

**English** | [Русский](CHANGELOG.ru.md)
