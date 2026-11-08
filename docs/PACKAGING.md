# Packaging & distribution

**English** | [Русский](PACKAGING.ru.md)

How LinuxPrinterG is built into `.deb` / `.rpm` / Flatpak / AppImage and how
it reaches the user. Overview: [../README.md](../README.md); release routine:
[MAINTENANCE.md](MAINTENANCE.md); translations: [LANGUAGES.md](LANGUAGES.md).

## deb and rpm (nfpm)

One description — two package formats, no `dpkg-deb`/`rpmbuild`, no root:

```bash
make deb        # dist/linuxprinterg_<version>_amd64.deb
make rpm        # dist/linuxprinterg-<version>-1.x86_64.rpm
make packages   # both
```

`make deb`/`make rpm` first run a release build, then
`scripts/build-packages.sh`:

* the version comes from the workspace `Cargo.toml` (the first
  `version = …` line — the single source of truth) and is substituted into
  the `__VERSION__` placeholder of `packaging/nfpm.yml`;
* the nfpm binary is downloaded once into `target/tools` by
  `scripts/get-nfpm.sh` (network required);
* pre-flight checks fail the build if the release binary, the hicolor icons
  (`python3 scripts/render-icons.py`) or any of the three `.mo` catalogs is
  missing;
* the architecture follows `uname -m` (`x86_64`→`amd64`/`x86_64`,
  `aarch64`→`arm64`/`aarch64`).

Package contents (from `packaging/nfpm.yml`): `/usr/bin/linuxprinterg`,
`.desktop`, metainfo, hicolor icons, `/usr/share/locale/{uk,de,fr}` trees,
`LICENSE`. Hard dependencies: `libgtk-4-1`/`gtk4` ≥ 4.14,
`libadwaita-1-0`/`libadwaita` ≥ 1.5, `cups-client`, `poppler-utils`;
Recommends: sane, LibreOffice, tesseract. `packaging/postinstall.sh` and
`preremove.sh` refresh the desktop/icon caches.

## rpm from source (rpmbuild)

`packaging/linuxprinterg.spec` builds the RPM on Fedora with the usual
tooling: `BuildRequires` cargo ≥ 1.85, gcc, `pkgconfig(gtk4)` ≥ 4.14,
`pkgconfig(libadwaita-1)` ≥ 1.5; `%build` runs `%make_build release`,
`%install` runs `%make_install DESTDIR=%{buildroot}`.

## Flatpak

```bash
make flatpak    # flatpak-builder --force-clean --user --install …
```

Manifest: `crates/linuxprinterg/data/org.linuxprinterg.LinuxPrinterG.flatpak.yml`
(GNOME runtime). The sandbox blocks USB and the local CUPS socket, so the
manifest carries **documented security exceptions**:

* `--device=all` — SANE needs USB access for scanning;
* `--share=network` — local CUPS (localhost:631) and network printers;
* `--filesystem=xdg-download`, `--filesystem=xdg-documents` — the scans
  folder.

These privileges are broader than a regular package's — this is a recorded
exception, and the **recommended distribution form is a system package**
(.deb/.rpm).

## AppImage

```bash
linuxdeploy --appdir AppDir -e target/release/linuxprinterg \
  --plugin gtk --output appimage
```

## System install

```bash
sudo make install   # binary + .desktop + metainfo + hicolor icons
```

## CI (.github/workflows/ci.yml)

| Job | What it runs | Artifact |
|---|---|---|
| `check` | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, integration tests (`--ignored`) | — |
| `build` | release build | `linuxprinterg` (binary) |
| `packages` | `./scripts/build-packages.sh all` (nfpm downloaded in CI) | `installers` — `dist/*.deb`, `dist/*.rpm` |
| `flatpak` | flatpak-builder in the GNOME SDK container | `linuxprinterg-x86_64.flatpak` |

The `packages` and `flatpak` jobs depend on `check`, so a red check blocks
every artifact.

## Distribution plans

* done: `.deb`/`.rpm` in the repository, CI installers, CI Flatpak bundle;
* roadmap ([../ROADMAP.md](../ROADMAP.md)): **Flathub, Copr, AUR** — so
  installation becomes one command on every target distribution;
* candidate channels not yet on the roadmap: a **PPA** for
  Debian/Ubuntu and an AUR-style repo for Arch — decide before publishing.

See also: [../README.md](../README.md), [../CONTRIBUTING.md](../CONTRIBUTING.md),
[MAINTENANCE.md](MAINTENANCE.md).
