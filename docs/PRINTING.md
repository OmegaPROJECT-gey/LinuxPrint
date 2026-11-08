# Printing guide

**English** | [Русский](PRINTING.ru.md)

How LinuxPrinterG sends jobs to CUPS, which options it sets and what to do
when something goes wrong. General overview: [../README.md](../README.md);
photo-specific behaviour: [AUTO-FIT.md](AUTO-FIT.md).

## How printing works

Standard CUPS user utilities — no root, no `lpadmin`:

* **options** — `lpoptions -p <printer> -l` fills the dropdowns with the
  printer's real choices (`PageSize`, `InputSlot`, `MediaType`, `Duplex`,
  `print-scaling` …);
* **submission** — `lp -d <printer> -n <copies> -o key=value … -- <file>`;
* **cancellation** — `cancel <printer>-<id>` (confirmation in the UI);
* **queue** — a built-in IPP client
  (`crates/linuxprinterg-core/src/services/ipp.rs`) posts HTTP
  `application/ipp` to the local cupsd (`/run/cups/cups.sock` or
  `127.0.0.1:631`, `CUPS_SERVER` overrides) for exact `job-state` values;
  when IPP is unavailable the app falls back to `lpstat -W … -o`.

Every choice is validated against the printer's supported values before
sending: an unsupported `media=…` or `print-scaling=…` is rejected with a
message listing what is actually available, instead of a failed `lp`.

## Accepted files

| Input | Handling |
|---|---|
| PDF | sent as is; preview = first page via `pdftoppm` (poppler-utils) |
| PNG / JPEG / TIFF / BMP / SVG / TXT / PS / EPS | sent as is (CUPS filters) |
| WebP | not all CUPS filters understand it → assembled into a PDF by the own encoder, the PDF is printed |
| DOCX / ODT / RTF / PPT / XLS … | converted by LibreOffice headless (`soffice --headless`); without LibreOffice — a hint to install it or save as PDF |
| anything else | rejected with "save the document as PDF" |

## Options

| UI row | CUPS option | Notes |
|---|---|---|
| Copies | `-n`, plus `Collate=true` when > 1 | 1–999 |
| Pages | `page-ranges` | `1-3,7`; parser in `domain/page_range.rs` |
| Paper size | `media` | from `lpoptions -p … -l` |
| Paper tray | `InputSlot` | |
| Media type | `MediaType` | "None" = not sent |
| Fit to page | `print-scaling` | sent **only** if the printer advertises the option; index 0 = "Printer default" (nothing sent) |
| Duplex | `sides` | only two-sided values are sent |
| Color mode | `print-color-mode` | color / grayscale / auto |
| Quality | `print-quality` | draft / normal / high / photo |
| Orientation | `orientation-requested` | portrait / landscape (+ reverse) |
| Print at | `job-hold-until` | delayed printing, see below |

**Photo profile** sets quality=photo, color, one-sided printing and a photo
media type when the printer reports one. **Photo 4×6 / 10×15** does the same
and additionally picks the sheet by the aliases `4x6`/`10x15`/`100x150`
(exact match → borderless → substring); if the printer has no such media, an
honest message is shown and the size stays unchanged.

## Delayed printing

Row «Print at» (`job-hold-until`): **Now** (nothing sent) / **Day-time** /
**Evening** / **Night** / **Weekend** → CUPS values `day-time`, `evening`,
`night`, `weekend`. Named values are checked against the printer's real
`job-hold-until` choices; a manual `HH:MM` time passes through untouched
(the choices list cannot contain it by definition).

## Queue

* exact IPP statuses: «Printing» (with «sheet X of Y» from
  `job-media-sheets-completed`/`job-media-sheets` when the printer reports
  them), «On hold», «Stopped», «Completed», «Cancelled», «Aborted»;
  pending jobs are shown without a status label;
* actions: cancel (confirmation), **Hold / Release** for active jobs and
  **Restart** for finished ones — IPP job operations with confirmation and a
  journal entry;
* auto-refresh every **3 s**; after **2 failures in a row** polling stops
  until «Refresh» is pressed manually (or a poll succeeds);
* a watched job reaching a terminal state → a system notification
  (freedesktop Notifications over D-Bus) plus a journal entry; the number of
  active jobs is the badge next to «Print» in the sidebar;
* the list holds active jobs and the **last 10 completed** ones; on the
  `lpstat` fallback only "active/completed" can be told apart.

## Troubleshooting

### The queue accepts nothing (printer paused)

```bash
lpstat -p             # "disabled" means the queue is paused
cupsaccept MyPrinter  # accept jobs again
cupsenable MyPrinter  # and enable the queue
```

### CUPS is stopped

The app shows a banner «CUPS scheduler is not running».

```bash
sudo systemctl status cups
sudo systemctl start cups
lpstat -r             # must answer "scheduler is running"
```

### A job ends up in an error state

```bash
lpstat -W all -o        # every job, including failed ones
lpstat -o               # jobs not completed
sudo tail -n 100 /var/log/cups/error_log
```

Set `LogLevel debug` in `/etc/cups/cupsd.conf` and restart cupsd for
details — the reason (missing driver, offline device, failed filter) is
logged there. The application's own log:
`~/.local/state/linuxprinterg/logs/linuxprinterg.log` (daily rotation).

### Printer not found

```bash
lpstat -v                    # URIs CUPS knows
avahi-browse -rtp _ipp._tcp  # network (Mopria) printers
```

Network printers are usually added as `ipp://host:631/printers/NAME` or
`ipps://host/ipp/print` (driverless IPP Everywhere). Add the device with
`system-config-printer` or the CUPS web interface `http://localhost:631/admin`
— both request polkit authorization themselves; the app never calls
`lpadmin`.

### Margins and cropping

* the sheet must match what the driver advertises:
  `lpoptions -p MyPrinter -l | grep -i pagesize`;
* wide white borders are the classic "photo on A4" case → turn on
  **Auto photo fit** ([AUTO-FIT.md](AUTO-FIT.md));
* print margins are driver CUPS options; their exact list depends on the
  printer and is not invented by the application.

### No driver for the printer

CUPS uses the drivers installed in the system; LinuxPrinterG ships none.

```bash
# Debian/Ubuntu
sudo apt install cups-filters printer-driver-gutenprint hplip
# Fedora
sudo dnf install cups-filters gutenprint hplip
# Arch
sudo pacman -S cups-filters gutenprint hplip
```

Modern printers usually work **driverless** (IPP Everywhere / Mopria) and
need no vendor driver at all; `cups-filters` provides the generic filters.

## Useful commands by hand

```bash
lpstat -t                  # everything at once
lpstat -p -v               # printers + their URIs
lpoptions -p MyPrinter -l  # supported options and choices
lp -d MyPrinter -o media=A4 -o print-scaling=fill photo.jpg
cancel MyPrinter-23        # cancel by job token
cupsdisable MyPrinter      # pause printing
```

See also: [../README.md](../README.md), [AUTO-FIT.md](AUTO-FIT.md),
[../ARCHITECTURE.md](../ARCHITECTURE.md).
