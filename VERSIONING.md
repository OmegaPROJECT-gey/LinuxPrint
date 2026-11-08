# Versioning

LinuxPrinterG follows **[Semantic Versioning 2.0.0](https://semver.org/)**:
`MAJOR.MINOR.PATCH` with pre-release labels. This document explains what each
segment means for this project in particular.

---

## The source of truth

The version lives in **one place**: `[workspace.package] version` in the root
`Cargo.toml` (today `0.1.0`). Makefiles, packaging scripts, CI and the
Flatpak manifest read it from there — no file carries a hand-written version
of its own. If the documents and `Cargo.toml` ever disagree, `Cargo.toml` is
right.

The current release is always recorded in
[CHANGELOG.md](CHANGELOG.md) under its own section.

## What the API is

For a library-like project the "public API" is the **public interface of the
`linuxprinterg-core` crate**: exported functions, structs and enums, their
fields and semantics, and the behaviour that documented code relies on.

* A change that breaks a caller — removing or renaming a public item,
  changing a signature, changing what a parser accepts or returns — is a
  **breaking** change.
* The GTK application crate is an end product, not a library: its internals
  change freely, but anything that changes user-visible behaviour (defaults,
  file formats, journal structure, command-line expectations) is treated as an
  API change and goes through the same rules.

## Pre-1.0 rules

While the version is `0.x`:

* **MINOR** may carry breaking changes — this is the SemVer contract for
  `0.y.z`;
* **PATCH** never does: it is fixes, translations, packaging and performance
  only;
* once `1.0.0` is declared, breaking changes move to **MAJOR**, and the `0.x`
  behaviour above becomes history.

## Patch numbers are calendar-driven

Patch numbers are handed out in release order: every dated release adds one to
the `PATCH` segment, regardless of how small it is. Releases are dated in
[CHANGELOG.md](CHANGELOG.md), so the version number and the changelog share
one timeline — you can always tell which of two builds is newer from the
number alone, without checking timestamps.

## Pre-release labels

Work that is not ready for general use is marked with SemVer pre-release
identifiers, ordered oldest to newest:

| Label | Meaning |
|---|---|
| `0.2.0-alpha.1` | early build, API and behaviour may still change |
| `0.2.0-beta.1` | feature-complete, testing for regressions |
| `0.2.0-rc.1` | release candidate, only fixes are accepted |

The ordering `alpha.N < beta.N < rc.N < final` is the one SemVer defines
(`alpha` sorts before `beta`, hyphenated identifiers sort before the release).
Pre-releases are not installed over a final version by package managers
unless the user explicitly asks for that channel.

## Tags and artifacts

* A release is marked by an annotated tag **`vX.Y.Z`** (for example
  `v0.1.0`) — the leading `v` is part of the tag, not of the version.
* The tag, the `Cargo.toml` version and the CHANGELOG section must be equal at
  the moment the tag is pushed; CI builds `.deb`/`.rpm` named with exactly
  that version (`linuxprinterg_X.Y.Z_amd64.deb`,
  `linuxprinterg-X.Y.Z-1.x86_64.rpm`) and a Flatpak bundle from the same
  commit.
* Pre-release tags carry the label too: `v0.2.0-rc.1`.
* Back-ports, if they ever happen, get an extra suffix on a branch of their
  own and are not published as newer than the main line.

## Relationship to the changelog

Every release section in [CHANGELOG.md](CHANGELOG.md) is titled with the same
`X.Y.Z` and the date it was tagged. Anything user-visible lands under
`[Unreleased]` first and moves into the release section at tag time — so
reading a diff between two tags answers both "what changed" and "why the
number moved".

---

**English** | [Русский](VERSIONING.ru.md)
