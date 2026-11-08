# Philosophy

**LinuxPrinterG is a control panel for the printing and scanning stack that
Linux already has — not a new stack of its own.**

Everything below is a promise the code has to keep. A contribution that breaks
one of these principles is declined regardless of how well it is written; a
bug that breaks one of them is treated as a bug of the highest priority.

---

## 1. No proprietary drivers

The application ships **zero drivers**. Printing is CUPS, scanning is SANE —
the backends, PPDs and filters already installed by the distribution. If the
system sees the device, we drive it; if it does not, we say so honestly and
name the package that fixes the situation. No vendor blobs, no bundled
firmware, no downloads of closed-source printer utilities.

## 2. No root

Nothing in LinuxPrinterG needs administrator rights. Adding a printer goes
through the system helpers (`system-config-printer`, the CUPS web admin at
`http://localhost:631/admin`) which request polkit authorization themselves;
user defaults are set with `lpoptions -d`. The app never calls `lpadmin`, never
asks for a password and never writes outside the user's XDG directories.

## 3. Honest limitations instead of loud promises

"We support every printer and scanner" means: everything **CUPS and SANE**
support. A device without a driver produces a clear error with an instruction,
not a lie and not a silent failure. When the printer does not declare an
option (`print-scaling`, media type, tray), the option is not sent — and the
UI shows «default» rather than pretending it was applied. The README keeps an
explicit «Known limitations» section, and it is updated whenever reality
changes.

## 4. The UI never blocks

Every external call — `lpstat`, `scanimage`, `pdftoppm`, `tesseract`,
`soffice`, an IPP request — runs on a background thread with a hard timeout and
an available cancel button. The main loop stays responsive during scanning,
conversion and queue refreshes. If an operation cannot be made
non-blocking, it does not belong in the interface.

## 5. Errors that tell you what to do

A failed command is not «exit code 2». Each error carries a localized message
plus the instruction: install `cups-client`, install `sane-backends`, install
`poppler-utils`, install `tesseract-ocr`, install LibreOffice. The user should
never have to read a log file to learn the next step — although the log is
there (rotated daily, level configurable) when they do.

## 6. Destructive actions always ask

Cancelling a job, clearing the journal, resetting settings, discarding a
scan — every action that loses data or cannot be undone goes through a
confirmation dialog. This costs one click and saves the user from the kind of
mistake they will remember.

## 7. Accessibility is not optional

The whole app is operable from the keyboard: standard GTK4 widgets, visible
focus, accelerators `Ctrl+1..5`, `Ctrl+R`, `Ctrl+Q`. Labels and tooltips are
present for screen readers (GTK/AT-SPI), the Adwaita palette keeps contrast
readable in both themes, and on narrow windows the sidebar collapses instead of
cutting content off. A feature hidden behind a mouse-only interaction is a
defect.

## 8. Minimalism and speed

One window, five pages, no dashboards, no accounts, no telemetry. Configuration
is a single JSON file in XDG directories; the journal is capped at 1000
entries; queues refresh automatically but stop after repeated failures instead
of hammering cupsd forever. Startup loads only what the visible page needs.
Fast and quiet beats feature-rich and loud.

## 9. Free code, all of it

The project is MIT-licensed from the first line to the last document
([LICENSE](LICENSE)). No dual licensing, no "free for personal use", no
closed-source plugins. What you can download, you can read, modify and
redistribute.

---

**English** | [Русский](PHILOSOPHY.ru.md)
