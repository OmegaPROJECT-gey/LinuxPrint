# Security Policy

## Reporting a vulnerability

Please do **not** open a public issue for a security problem. Report it
privately through **GitHub Security Advisories** of this repository:

1. open <https://github.com/linuxprinterg/linuxprinterg/security>;
2. choose the **Security** tab → **Report a vulnerability**;
3. fill in the form: what is affected, how to reproduce, the impact you see,
   and a suggested fix if you have one.

A private advisory reaches only the maintainers; you will get an answer in the
same thread. If the report is confirmed, we will credit you in the advisory and
in [CHANGELOG.md](CHANGELOG.md) — unless you prefer to stay anonymous.

**Response time: 7 days.** Within a week you should receive an acknowledgement
with an initial assessment. After that we agree on disclosure timing; for a
fixable issue our goal is a patched release within the same cycle as other
non-security changes.

## What counts as a vulnerability

* **Command injection** — any path by which data from a file name, a printer
  name, scan parameters or a settings file can reach an external command as
  something other than an argument (the project forbids a shell, so a shell
  appearing anywhere is a defect by definition).
* **File path escapes** — writing or reading outside the user-chosen
  directory: traversal through `..`, absolute paths or template placeholders
  (`{base}`, `{device}`, …) bypassing `naming::safe_join`, symlink tricks in
  the scans folder.
* **Privilege problems** — the app must never need root: anything that
  escalates, runs a helper as another user, or writes to system locations
  outside the XDG directories is a vulnerability.
* **Data exposure** — journal, settings or logs leaking secrets or other
  users' data; an IPP/HTTP request to a host that is not the local cupsd
  without the user asking for it.
* **Denial of service from untrusted input** — a malicious PPD, a crafted
  `lpstat`/`scanimage` output or a broken IPP response hanging the UI,
  crashing the process or allocating without bound.

## Supported versions

Only the **latest released version** (the one in the workspace `Cargo.toml`
and in the latest tag) is supported. Backports to older releases are not
provided; if you are on an old build, upgrade first and re-check.

## What is not a vulnerability

* **The lack of root rights.** Not being able to administer CUPS system-wide
  without polkit is the intended design, not a security gap.
* **Behaviour of CUPS/SANE drivers and backends.** The project ships no
  drivers; a flaw inside a distribution's PPD, filter or SANE backend belongs
  to that distribution's bug tracker.
* **The documented Flatpak exceptions** (`--device=all`, `--share=network`,
  the download/documents filesystems). They are a declared, reviewed security
  exception in `org.linuxprinterg.LinuxPrinterG.flatpak.yml`; the recommended
  install remains a system package.
* **Missing features** (no encryption of the journal, no sandbox inside the
  app) and **social engineering** against the user.
* **A tool not installed** — the app reacts with a hint to install it; that
  is a normal, documented path, not a failure.

---

**English** | [Русский](SECURITY.ru.md)
