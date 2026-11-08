# Contributing to LinuxPrinterG

Thank you for considering a contribution. This document is the whole workflow:
toolchain, checks, style, translations, and how a change gets merged.

By contributing you agree that your contribution is distributed under the
project's [MIT license](LICENSE), the same as the rest of the code.

---

## Before you start

1. Read [PHILOSOPHY.md](PHILOSOPHY.md). A feature that contradicts it will be
   declined no matter how well it is implemented.
2. Read [ARCHITECTURE.md](ARCHITECTURE.md) — the boundary between
   `linuxprinterg-core` (no GTK) and the UI crate is not negotiable.
3. Pick a task from [ROADMAP.md](ROADMAP.md) or open an issue first if you are
   planning something large.

## Toolchain

* **Rust 1.85+** (workspace is on edition 2024).
* For the UI crate: GTK4 ≥ 4.14 and libadwaita ≥ 1.5 development packages
  (`libgtk-4-dev` + `libadwaita-0-dev` on Debian/Ubuntu, `gtk4-devel` +
  `libadwaita-devel` on Fedora, `gtk4` + `libadwaita` on Arch).
* Runtime tools used by tests: `cups-client`, `sane-utils`, `poppler-utils`;
  optionally `tesseract-ocr` and LibreOffice.
* In a rootless sandbox without GTK dev packages use
  `scripts/setup-fedora-dev-prefix.sh` and export `PKG_CONFIG_PATH` /
  `RUSTFLAGS` as described in the README.

## Checks that must pass

Run them locally — CI runs exactly the same commands:

```bash
cargo fmt --all -- --check        # or: make fmt-check
cargo clippy --workspace --all-targets -- -D warnings   # or: make clippy
cargo test --workspace            # or: make test
```

Integration tests that call real external tools are opt-in:

```bash
make test-int     # cargo test -p linuxprinterg-core -- --ignored --test-threads=1
```

Other useful targets: `make build`, `make run`, `make release`, `make fmt`
(rewrites formatting), `make po` and `make po-build` (translations),
`make deb` / `make rpm` / `make packages` (packaging).

## Style

* Formatting is entirely `rustfmt` with the repository's `rustfmt.toml`
  (edition 2024, `max_width = 100`) — never argue about whitespace in review.
* Clippy warnings are errors; if a lint is genuinely wrong, suppress it with a
  comment explaining **why**, not with `allow` silently.
* **Comments in the code are written in Russian.** That is the established
  convention of this repository: module doc comments (`//!`), `///` docs and
  inline comments are Russian; identifiers stay English.
* Doc comments explain intent and constraints, not the syntax.
* Keep the split: parsing, file formats and process calls belong in
  `linuxprinterg-core`; anything GTK-related belongs in the UI crate. Core
  logic must stay testable without a display server.

## Translations (i18n)

* The RU/EN dictionary lives in `crates/linuxprinterg-core/src/i18n.rs`.
  A new user-visible string is added **twice**: to the dictionary entry
  (`key → (en, ru)`) **and** to the `ALL_KEYS` array. Tests
  (`cargo test -p linuxprinterg-core`) fail if either is missing, or if the
  two locales diverge in placeholder count.
* uk/de/fr are gettext overlays. After touching the dictionary run
  `make po` — it regenerates `po/linuxprinterg.pot` and appends the missing
  keys to `po/uk.po`, `po/de.po`, `po/fr.po`. Fill in the new `msgstr`
  yourself or leave it empty: an empty or `#, fuzzy` entry falls back to
  English at runtime.
* `make po-build` compiles the `.po` files into
  `locale/<ll>/LC_MESSAGES/linuxprinterg.mo`.
* Never change a `msgid` (the pseudo-key) without updating the dictionary —
  the `.po` files are derived from it.

## Pull requests

1. Fork, create a branch, keep commits focused (one problem per PR).
2. Make sure `fmt-check`, `clippy` and `test` pass locally.
3. Describe **what** changed and **why**; for UI changes a screenshot helps;
   for parser changes mention the fixtures you added.
4. Update the docs you touched: README for user-visible features,
   CHANGELOG.md under `[Unreleased]`, ROADMAP.md checkboxes when a roadmap
   item is done. Each document exists in English and Russian — change both
   versions of a pair in the same PR.
5. Expect CI to run fmt · clippy · tests · release build · `.deb`/`.rpm` ·
   Flatpak, and be ready to fix what it finds.

## Issues

Open an issue when you found a bug or want to discuss a feature. A useful
issue contains: app version, distribution and desktop (Wayland/X11), printer or
scanner model, exact steps to reproduce, expected vs. actual behaviour, and
the tail of `~/.local/state/linuxprinterg/logs/linuxprinterg.log` (raise the
log level in Settings if needed). Feature requests should name the real-world
problem, not only the proposed solution.

Security issues do **not** go into public issues — see
[SECURITY.md](SECURITY.md).

## Code of conduct

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

---

**English** | [Русский](CONTRIBUTING.ru.md)
