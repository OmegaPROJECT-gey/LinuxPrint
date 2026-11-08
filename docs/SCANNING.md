# Scanning

**English** | [Русский](SCANNING.ru.md)

How LinuxPrinterG scans through SANE, what it can change and where the saved
files go. Overview: [../README.md](../README.md); printing:
[PRINTING.md](PRINTING.md).

## How scanning works

Everything goes through the system SANE utility `scanimage` — the application
ships no scanner drivers (`crates/linuxprinterg-core/src/services/sane.rs`):

* `scanimage -L` — the device list;
* `scanimage --help -d <id>` — resolutions, modes, sources, device bounds;
* `scanimage --device-name=… --resolution=… --mode=… --source=…
  --format=pnm --progress` — one run per page, the output is decoded by the
  own PNM reader.

Runs happen in background threads with hard timeouts and a `CancelHandle`
that kills `scanimage` when you press «Cancel scan».

## Backends and sources

* USB scanners work through whatever backend the system has;
* network and **AirScan/eSCL** scanners need the `escl` backend — then they
  appear in `scanimage -L` as `escl:https://…` (install `sane-airscan`);
* sources come from `scanimage --help`: **Flatbed**, **ADF**, **ADF Duplex**
  (only if the scanner advertises them). Flatbed always scans **one page per
  run** — place the sheet and press again; automatic feeding is the ADF's job;
* «Pages (0 = all from ADF)»: `0` reads until the feeder reports it is empty
  (with a 200-page guard against a loop); an empty feeder after page 1 ends
  the scan normally, with no pages at all → "No pages: document feeder is
  empty".

## Resolution, mode, progress

* the resolution list is parsed from `scanimage --help` (fallback
  `75 100 150 200 300 600 1200`); the request is clamped to 50–4800 dpi,
  the saved default to 50–2400;
* modes: Lineart / Grayscale / Color, also read from `--help`;
* progress is shown **per page and in percent** — `scanimage --progress`
  writes `Progress: 45.3%` to stderr and the app reads it as a stream.

## Crop and rotation

* «Crop area, mm»: X / Y / Width / Height in millimetres, passed to
  `scanimage` as `-l/-t/-w/-h` and clipped to the device bounds reported in
  `--help`; «Full page» resets the fields;
* rotation 0° / 90° / 180° / 270° is applied to every scanned page.

## Saving

| Format | How |
|---|---|
| **PDF** | own writer (`services/pdf.rs`): one file for the whole scan, raster pages compressed with zlib (`FlateDecode`), JPEG pages embedded without re-encoding (`DCTDecode`); verified with `pdftoppm` |
| **TIFF** | own multi-page writer (`services/tiff.rs`, IFD chain + Adobe Deflate), one file for the whole scan; verified with libtiff and Pillow |
| PNG / JPEG / WebP | one file per page (the codecs have no multi-page support) |

With several pages and a template without `{page}`, the app appends `_{page}`
itself. Default folder: `~/Documents/Scans` (changeable in the scan page and
in settings).

### File name template

Fields: `{base}` (source name), `{device}`, `{date}`, `{time}`, `{page}`,
`{n}`, `{ext}` — default `{date}_{time}_{base}`. The result is sanitized
(path separators and control characters → `_`) and made unique (`(2)`, `(3)`
…), and it can never escape the chosen folder (`naming::safe_join`).

## OCR (optional)

A switch next to the format: Tesseract writes a `.txt` next to the scan.
Languages offered: `rus+eng`, `rus`, `eng` (the string is sanitized before
calling tesseract, `--psm 1`, 300 s timeout). Language data is a separate
package — Debian/Ubuntu `tesseract-ocr-rus`, Fedora `tesseract-langpack-rus`,
Arch `tesseract-data-rus`.

## Troubleshooting

### The scanner is not visible

```bash
scanimage -L              # what SANE sees
sane-find-scanner         # USB detection without a backend
scanimage --help -d <id>  # does the device answer?
```

### No `escl` backend (network / AirScan scanner)

```bash
# Debian/Ubuntu, Fedora, Arch
sudo apt install sane-airscan     # или dnf / pacman с тем же именем
```

Then re-check `scanimage -L` — the device should appear as `escl:https://…`.

### USB permissions

If `scanimage -L` needs root, add the user to the `sane` group (and make sure
the scanner is not claimed by another process — the app reports "Scanner is
busy"). The application never runs as root.

### Network scanner and discovery

```bash
avahi-browse -rtp _uscan._tcp   # AirScan/eSCL scanners on the LAN
```

The Devices page shows them in the «Found on the network» group in addition
to the SANE list; without Avahi installed you get a hint instead of an error.

## Useful commands by hand

```bash
scanimage --device-name=escl:https://192.168.1.5:443 \
  --resolution 300 --mode Color --source Flatbed --format=pnm > out.pnm
tesseract out.pnm stdout -l rus+eng --psm 1  # OCR of a page
```

See also: [../README.md](../README.md), [../ARCHITECTURE.md](../ARCHITECTURE.md).
