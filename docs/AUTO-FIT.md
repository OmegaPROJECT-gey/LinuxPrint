# Auto photo fit

**English** | [Русский](AUTO-FIT.ru.md)

The "Canon PRINT" behaviour: pick the sheet for the photo, fill it and say
in advance exactly what will be cropped. Code:
`crates/linuxprinterg-core/src/services/fit.rs` — pure logic, no IO, unit
tested. Printing mechanics: [PRINTING.md](PRINTING.md); overview:
[../README.md](../README.md).

## Why

A 3:2 photo printed "as is" on A4 leaves wide white borders — the printer
fits the whole image inside the sheet instead of covering it. Auto photo fit picks the sheet whose
aspect ratio is closest to the shot, prints it **filling the sheet** (edges
cut off) and shows the result as a caption before you press Print.

## How the sheet is chosen

Input: the pixel size of the loaded image (taken from the preview texture)
plus the printer's real `PageSize` list from `lpoptions -p <printer> -l`.
If the options cannot be read, a fallback list is used:
`A4 A5 A3 A6 Letter Legal Executive 4x6`.

`media_dims()` parses one value:

* named formats: `A3 A4 A5 A6 B4 B5 Letter Legal Executive Oficio Postcard`
  and `10x15` / `100x150` — all in millimetres;
* explicit millimetres: `215x345mm`, `100x150mm`;
* photo formats in inches: `4x6`, `5x7`, `3.5x5`, `8x10` (values ≥ 100″ are
  rejected, so garbage names like `A4x2` never parse);
* a trailing `.Borderless` is stripped and remembered — that is the
  borderless variant of the same sheet;
* `Custom.*`, envelopes (`Env…`) and fanfold paper return `None`: they are
  never a photo sheet.

`best_pick()` scores every candidate with `|ln(sheet_ratio / photo_ratio)|` —
a logarithmic scale, so "slightly larger" and "slightly smaller" deviations
cost the same. Both orientations are tried: if the rotated sheet is closer,
the plan is `landscape = true`. Ties go to the larger sheet, and with
`prefer_borderless` (always on in the UI) a `.Borderless` variant gets a
small bonus (−0.005), so `4x6.Borderless` wins over `4x6`.

## fill vs fit

* the printer advertises `print-scaling` **with the value `fill`** → mode
  `fill`: the scale factor is `max(sheet_w / photo_w, sheet_h / photo_h)`,
  the sheet is fully covered and the excess is cut **centred**; the crop is
  reported in millimetres;
* otherwise → mode `fit` (safe, no cropping at all), and the "Fit to page"
  row only changes when the printer declares that value.

`print-scaling` itself is sent only when the printer advertises the option;
with "Printer default" selected nothing is sent (see
[PRINTING.md](PRINTING.md)).

## Caption under the preview

```
4x6.Borderless · fills the sheet · crop 13 × 0 mm
```

Format · mode · crop in millimetres (the crop part disappears when both
values are 0; mode text is «fills the sheet» / «fits inside the sheet»). If
no sheet matches, the caption says "The printer did not report a matching
paper format".

## The switch and settings

The «Auto photo fit» row sits in the print parameters. The plan is
recalculated when: a file is selected, the printer changes, the printer's
options finish loading, or the switch is toggled. The state is stored in
`PrintDefaults.auto_fit` inside `~/.config/linuxprinterg/settings.json`
(default: **on**).

Manual edits made **after** a calculation are not rolled back: the plan is
applied to the dropdowns once and does not fight the user. Choosing another
file or another printer recalculates it.

## Limits

* **PDF has no pixel size** — auto-fit runs on images only; for PDF pick the
  sheet yourself (its preview is rendered with `pdftoppm`);
* the candidate list is whatever `lpoptions -p <printer> -l` reports — a
  format the driver does not advertise is never chosen;
* `fill` requires the printer to advertise it, otherwise the plan honestly
  says "fits inside the sheet";
* only portrait/landscape is set — reverse orientations are never touched.

## Check it by hand

```bash
lpoptions -p MyPrinter -l | tr ' ' '\n' | grep -iE 'A4|4x6|borderless'
lp -d MyPrinter -o media=4x6.Borderless -o print-scaling=fill photo.jpg
```

Related: [../README.md](../README.md), [PRINTING.md](PRINTING.md),
[../ARCHITECTURE.md](../ARCHITECTURE.md).
