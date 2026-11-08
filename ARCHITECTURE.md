# Architecture

**How LinuxPrinterG is built: a two-crate Rust workspace where every line of
logic lives outside GTK and is testable without a graphical session.**

The app ships no drivers of its own. It drives CUPS (`lp`, `lpstat`, `cancel`,
`lpoptions`, plus a mini IPP client over HTTP) and SANE (`scanimage`,
including the `escl` backend for AirScan/eSCL), and answers with a clear
instruction when a tool is missing.

---

## The workspace

The root `Cargo.toml` declares two crates (resolver 2, edition 2024,
Rust ≥ 1.85, `version` in `[workspace.package]`):

| Crate | Role | Links GTK |
|---|---|---|
| `linuxprinterg-core` | everything that is not a widget: domain types, CUPS/SANE/IPP integration, PDF/TIFF writers, naming, settings, journal, i18n | no |
| `linuxprinterg` | the application: GTK4 ≥ 4.14, libadwaita ≥ 1.5, pages, window, async bridge | yes |

The core never imports GTK; the UI crate never parses `lpstat` output. That
boundary is why `cargo test --workspace` passes on a headless CI runner with
no display server.

## Layers inside the core

* `domain/` — plain data: `Printer`, `Scanner`, `Job`, print/scan requests,
  `page_range.rs` (`1-3,7` → intervals).
* `services/` — one module per external tool or file format: `cups.rs`
  (runners + parsers), `ipp.rs` (HTTP `application/ipp` to the local cupsd,
  RFC 8010 written by hand), `sane.rs` (ADF, crop clipping), `fit.rs` (photo
  auto-fit), `discover.rs` (DNS-SD via `avahi-browse`), `pdf.rs`/`save.rs`/
  `tiff.rs` (own writers), `ocr.rs`, `convert.rs`, `preview.rs`, `naming.rs`,
  `proc.rs` (process spawning).
* `storage/` — JSON in XDG directories, atomic writes: `settings.rs`
  (serde + validation), `journal.rs` (log, CSV export, 1000-entry cap).
* `i18n.rs` / `po.rs` — RU/EN dictionary with `tr()`/`trf()` and an own
  PO/MO reader-writer for the uk/de/fr gettext overlay.
* `error.rs` — errors carry localized messages plus the "what to install"
  instruction; `logging.rs` — `tracing` to stderr and a daily-rotated file.

No widget and no main loop exists inside `core`.

## Layers inside the UI

`main.rs` (settings → logging → i18n → start), `app.rs` (`AdwApplication`,
`APP_ID`, accelerators `Ctrl+1..5`, `Ctrl+R`, `Ctrl+Q`), `window.rs`
(`AdwOverlaySplitView`, sidebar collapses under 760 px, lazy pages,
`confirm()` for destructive actions), `ctx.rs` (`AppCtx`: settings, journal,
toasts, device cache shared by the pages), `tasks.rs` (the bridge below) and
`pages/` — `print.rs`, `scan.rs`, `devices.rs`, `settings.rs`, `journal.rs`.

## Data flow

1. A widget handler calls `tasks::run_async(task, on_done)` or
   `tasks::run_with_progress(task, on_progress, on_done)`.
2. `tasks.rs` spawns a `std::thread`; the closure runs the blocking core call
   with hard timeouts.
3. The result returns over `async_channel`;
   `glib::MainContext::spawn_local` executes `on_done`/`on_progress` on the
   GTK main thread.
4. `on_done` updates widgets — nothing else may touch GTK off the main thread.

Scanning, OCR and conversion use a progress channel plus a result channel;
the progress channel closing means the task has ended.

**Cancellation.** The UI keeps a `CancelHandle` — `Arc` + `Weak` on the child
process; «Cancel» kills it and the core returns `Error::Cancelled`. Timeouts
in `proc.rs` do the same by themselves.

## External processes — `services/proc.rs`

`lp`, `lpstat`, `cancel`, `lpoptions`, `scanimage`, `tesseract`, `soffice`,
`pdftoppm`, `avahi-browse` — all go through one module with four rules:

* **no shell** — the program and its arguments are a `Vec`: nothing to inject;
* **hard timeout** — the child is killed and `Error::Timeout` is returned;
* **cancellation** — `CancelHandle` kills the process on demand from the UI;
* **`LC_ALL=C`** — locale-independent parsing, plus reader threads for
  stdout/stderr so a full pipe cannot deadlock the child.

## Directory tree

```
linuxprinterg/
├── Cargo.toml                      # workspace (edition 2024, Rust ≥ 1.85)
├── Makefile                        # build, tests, packaging
├── rustfmt.toml
├── .github/workflows/ci.yml        # fmt + clippy + test + build + deb/rpm + flatpak
├── LICENSE                         # MIT
├── README.md
├── packaging/
│   ├── nfpm.yml                    # .deb/.rpm description (nfpm)
│   ├── postinstall.sh              # desktop/icon caches
│   ├── preremove.sh
│   └── linuxprinterg.spec          # alternative RPM build (rpmbuild)
├── scripts/
│   ├── render-icons.py             # SVG → hicolor (librsvg + cairo)
│   ├── get-nfpm.sh                 # download nfpm into target/tools
│   ├── build-packages.sh           # build .deb and .rpm
│   └── setup-fedora-dev-prefix.sh  # GTK prefix for a rootless sandbox
└── crates/
    ├── linuxprinterg-core/         # CORE (no GTK)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── error.rs            # Error + localized messages/instructions
    │       ├── i18n.rs             # RU/EN dictionary, tr()/trf()
    │       ├── logging.rs          # tracing: stderr + rotating file
    │       ├── po.rs               # own PO/MO reader-writer (uk/de/fr overlay)
    │       ├── domain/
    │       │   ├── mod.rs          # Printer, Scanner, Job, print/scan requests
    │       │   └── page_range.rs   # "1-3,7" → intervals (+tests)
    │       ├── services/
    │       │   ├── proc.rs         # safe process spawning: no shell,
    │       │   │                    #   timeout, cancellation, LC_ALL=C
    │       │   ├── cups.rs         # lpstat/lp/cancel/lpoptions: runners+parsers
    │       │   ├── ipp.rs          # mini IPP client (HTTP POST to CUPS): exact
    │       │   │                    #   job states, manual RFC8010 encoding
    │       │   ├── sane.rs         # scanimage -L/--help/scan, ADF, crop clip
    │       │   ├── fit.rs          # photo auto-fit: media, orientation, crop
    │       │   ├── discover.rs     # DNS-SD via avahi-browse (Mopria, AirScan)
    │       │   ├── pdf.rs          # multi-page PDF writer (Flate/DCT)
    │       │   ├── save.rs         # PNG/JPEG/TIFF/WebP
    │       │   ├── tiff.rs         # multi-page TIFF (IFD chain + Adobe Deflate)
    │       │   ├── ocr.rs          # tesseract (language is sanitized)
    │       │   ├── convert.rs      # soffice --headless → PDF
    │       │   ├── preview.rs      # pdftoppm: first PDF page → PNG
    │       │   └── naming.rs       # name templates, uniqueness, sanitization
    │       └── storage/
    │           ├── mod.rs          # XDG paths, atomic writes
    │           ├── settings.rs     # Settings (serde) + validation
    │           └── journal.rs      # JournalHandle: log, export, cap 1000
    └── linuxprinterg/              # UI (GTK4 4.14+ / libadwaita 1.5+)
        ├── Cargo.toml
        ├── data/
        │   ├── org.linuxprinterg.LinuxPrinterG.desktop
        │   ├── org.linuxprinterg.LinuxPrinterG.metainfo.xml
        │   ├── org.linuxprinterg.LinuxPrinterG.flatpak.yml
        │   └── icons/              # app icon (SVG source + hicolor)
        └── src/
            ├── main.rs             # settings → logging → i18n → start
            ├── app.rs              # AdwApplication, APP_ID, accelerators
            ├── window.rs           # OverlaySplitView, lazy pages, confirm()
            ├── ctx.rs              # AppCtx: settings, journal, toasts, device cache
            ├── tasks.rs            # bridge "blocking task → GTK main thread"
            └── pages/
                ├── mod.rs
                ├── print.rs        # print: files, DnD, preview, queue
                ├── scan.rs         # scan: parameters, preview, PDF/OCR
                ├── devices.rs      # printers/scanners, adding, properties
                ├── settings.rs     # settings (PreferencesPage)
                └── journal.rs      # journal with auto-refresh and export
```

## What this buys us

Core tests run headless against fixtures of real tool output (and the IPP
client against a request captured from libcups); state lives in `AppCtx` and
cannot drift from the settings or the journal; a feature usually touches three
places — a `services/` module, `i18n.rs` + `ALL_KEYS` (plus `po/*.po` for
uk/de/fr), and a page.

---

**English** | [Русский](ARCHITECTURE.ru.md)
