# Maintenance

**English** | [Русский](MAINTENANCE.ru.md)

How releases, translations, icons and logs are kept healthy in this
repository. Overview: [../README.md](../README.md); packaging:
[PACKAGING.md](PACKAGING.md); translations: [LANGUAGES.md](LANGUAGES.md).

## Release process

1. **Version** — the workspace `Cargo.toml` (`[workspace.package]
   version = "…"` is the single source of truth; `scripts/build-packages.sh`
   reads it for the package names). Keep it in sync with
   `packaging/linuxprinterg.spec` (`Version:` + `%changelog`), which rpmbuild
   uses independently.
2. **Changelog** — move the `[Unreleased]` block of `CHANGELOG.md` to
   `## [X.Y.Z] — <date>` (Keep a Changelog, SemVer) and make the same change
   in `CHANGELOG.ru.md` — both twins in one commit.
3. **Checks** — everything below must be green before tagging.
4. **Tag** — `git tag vX.Y.Z && git push --tags`.
5. **GitHub Release** — create the release for the tag and attach the CI
   artifacts (see below): the `installers` artifact (`dist/*.deb`,
   `dist/*.rpm`), the `linuxprinterg` binary and `linuxprinterg-x86_64.flatpak`.

CI (`.github/workflows/ci.yml`) builds those artifacts on every push to
`main` and on pull requests; a red `check` job blocks them.

## Pre-release checklist

```bash
make fmt-check   # rustfmt
make clippy      # clippy -D warnings
make test        # unit tests (core, no GTK needed)
make test-int    # integration: real lpstat/scanimage/pdftoppm
make release     # optimized build (LTO, strip)
msgfmt --check po/uk.po po/de.po po/fr.po   # external validation only
```

Also: `make po-build` (the `.mo` files must exist — package builds refuse to
continue without them) and, after editing the SVG,
`python3 scripts/render-icons.py` (needs `python3-gi` + `librsvg2`); CI
fails if the scalable icon is missing from the tree.

## Updating translations

```bash
make po         # re-extract POT; append missing keys to po/{uk,de,fr}.po
# fill the new msgstr entries (existing translations are never touched)
msgfmt --check po/<ll>.po
make po-build   # rebuild locale/<ll>/LC_MESSAGES/linuxprinterg.mo
```

A dictionary change (`crates/linuxprinterg-core/src/i18n.rs`) and its
`.po`/`.mo` update belong in the same commit; completeness of RU/EN is
enforced by tests, uk/de/fr completeness is a human job — see
[LANGUAGES.md](LANGUAGES.md).

## Regenerating icons

```bash
python3 scripts/render-icons.py   # SVG → hicolor 16…256 + scalable
```

Run it whenever `crates/linuxprinterg/data/icons/` sources change; the
`.deb`/`.rpm` build checks that the result is present.

## Logs and diagnostics

| What | Where |
|---|---|
| Application log | `~/.local/state/linuxprinterg/logs/linuxprinterg.log` — daily rotation (a new file per day, old ones kept) |
| Log level | Settings → «Log level» (`error`/`warn`/`info`/`debug`) or the `RUST_LOG` variable (it overrides the setting) |
| Open the folder | Settings → «Open logs» |
| Journal (UI) | printing/scanning/system events, category filter, CSV export |
| Journal file | `~/.local/state/linuxprinterg/journal.json` (capped, atomic writes) |

Start the app from a terminal for stderr output:

```bash
RUST_LOG=debug linuxprinterg
```

CUPS-side problems are in `/var/log/cups/error_log`; scanner problems come
from `scanimage -L` / `--help`. Troubleshooting recipes:
[PRINTING.md](PRINTING.md), [SCANNING.md](SCANNING.md).

## Dependency updates

* `cargo update` refreshes `Cargo.lock` (it is committed); afterwards run
  `make fmt-check && make clippy && make test` and smoke-test the UI;
* toolchain floor: Rust **1.85** (edition 2024) — declared in
  `Cargo.toml` (`rust-version`) and in the spec; UI floor: GTK4 ≥ 4.14,
  libadwaita ≥ 1.5 — repeated in `README.md`, `packaging/nfpm.yml`,
  `packaging/linuxprinterg.spec` and CI; change all of them together;
* system dependencies of the runtime (cups-client, sane, poppler,
  tesseract, libreoffice) live in the README table, the nfpm `depends` /
  `recommends` and the spec — keep the three in step;
* the Flatpak runtime version (`runtime-version` in the manifest) is bumped
  deliberately: it must match the CI container image.

See also: [../README.md](../README.md), [../CONTRIBUTING.md](../CONTRIBUTING.md),
[../CHANGELOG.md](../CHANGELOG.md).
