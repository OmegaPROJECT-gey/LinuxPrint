# Roadmap

What is done, what is next, and in what order. Checked boxes are shipped in
the current code; unchecked ones are the plan. Dates are not promises —
see [VERSIONING.md](VERSIONING.md) for how releases happen.

---

## Done

- [x] **Own IPP client** (`services/ipp.rs`) — HTTP `application/ipp` to the
      local cupsd: exact job states (with an `lpstat` fallback), «sheet X of
      Y» progress from `job-media-sheets`, Hold/Release/Restart-Job operations
      with confirmation, RFC 8010 encoding written by hand.
- [x] **Queue auto-refresh every 3 s** with backoff after repeated failures and
      a manual «Refresh» button.
- [x] **Job completion notifications** — freedesktop Notifications over D-Bus
      (`gio::Notification`), a journal entry, and an active-jobs badge in the
      sidebar.
- [x] **Deferred printing** — «Now / Day / Evening / Night / Weekend» via
      `job-hold-until`, verified against the printer's real options.
- [x] **Journal: CSV export** (RFC 4180) and a category filter.
- [x] **i18n migration to gettext** — own PO/MO tooling, `make po` /
      `make po-build`, full translations for **uk, de, fr** on top of the
      RU/EN dictionary.
- [x] **Photo printing** — the «4×6 / 10×15» preset (aliases matched against
      the printer's real media list), the `print-scaling` dropdown (sent only
      when the printer declares the option), **ICC profiles** from the PPD with
      an honest note about automatic color management.
- [x] **Photo auto-fit, first pass** — `services/fit.rs`: paper format picked
      from the aspect ratio, `print-scaling fill` with crop in millimetres,
      landscape orientation, the resulting plan captioned under the preview.
- [x] **DNS-SD discovery** — Mopria printers and AirScan/eSCL scanners via
      `avahi-browse -rtp`, grouped as «Found on the network».
- [x] **Multi-page TIFF** (own writer, IFD chain + Adobe Deflate) and **WebP →
      PDF** assembly before submission to CUPS.
- [x] **Own multi-page PDF writer** for scans (Flate / DCTDecode, no
      re-encoding) and per-page progress with `scanimage --progress`.

## Next

- [ ] **Photo auto-fit to the level of Canon PRINT** — a live preview of the
      crop (what exactly will be cut off and by how much) and automatic
      selection of photo paper from what the printer actually reports, instead
      of relying on the user to pick the medium.
- [ ] **Ink / toner levels** — read IPP `marker-levels` where the printer
      exposes them, and fall back to pointing the user at the device's web
      admin page when it does not.
- [ ] **Direct subscription to cupsd events** (CUPS D-Bus) instead of polling
      the queue every 3 seconds: instant state changes, no wasted requests
      after the daemon stops.
- [ ] **Job percentage and «Continue printing»** — real %-of-completion for a
      job and the ability to resume a stopped/held job from the queue page.
- [ ] **Distribution: Flathub, Copr, AUR** — alongside the existing
      `.deb`/`.rpm` (nfpm) and the CI Flatpak bundle, so that installation is
      one command on every target distribution.
- [ ] **OCR mode «text PDF»** — scan straight into a searchable PDF (image
      layer + Tesseract text layer) instead of a separate `.txt` file.
- [ ] **AirScan scanners without `sane-airscan`** — talk eSCL directly so that
      network scanners work on distributions where the `sane-airscan` package
      is not packaged.
- [ ] **Machine-assisted cropping in the preview** (stretch goal) — today the
      app deliberately uses exact millimetre fields, which is more precise and
      more accessible; a smart suggestion would be an addition, not a
      replacement.

## Explicitly not planned

* Proprietary vendor drivers or firmware bundles — see
  [PHILOSOPHY.md](PHILOSOPHY.md).
* Requiring root, `lpadmin`-based printer setup inside the app, or a daemon of
  our own.
* Telemetry, accounts, ads, or any network call that is not made by CUPS,
  SANE, Avahi or the user's own printer.

When an item here is finished, it moves to
[CHANGELOG.md](CHANGELOG.md) and gets checked in the same pull request.

---

**English** | [Русский](ROADMAP.ru.md)
