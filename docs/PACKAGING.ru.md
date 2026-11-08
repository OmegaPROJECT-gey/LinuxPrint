# Упаковка и распространение

[English](PACKAGING.md) | **Русский**

Как LinuxPrinterG превращается в `.deb` / `.rpm` / Flatpak / AppImage и как
попадает к пользователю. Обзор: [../README.ru.md](../README.ru.md); релизный
процесс: [MAINTENANCE.ru.md](MAINTENANCE.ru.md); переводы:
[LANGUAGES.ru.md](LANGUAGES.ru.md).

## deb и rpm (nfpm)

Одно описание — два формата пакета, без `dpkg-deb`/`rpmbuild` и без root:

```bash
make deb        # dist/linuxprinterg_<version>_amd64.deb
make rpm        # dist/linuxprinterg-<version>-1.x86_64.rpm
make packages   # и то, и другое
```

`make deb`/`make rpm` сначала делают release-сборку, затем запускается
`scripts/build-packages.sh`:

* версия берётся из workspace `Cargo.toml` (первая строка `version = …` —
  единственный источник истины) и подставляется в плейсхолдер `__VERSION__`
  файла `packaging/nfpm.yml`;
* бинарник nfpm один раз скачивается в `target/tools` скриптом
  `scripts/get-nfpm.sh` (нужна сеть);
* проверки перед сборкой падают, если нет release-бинарника, иконок hicolor
  (`python3 scripts/render-icons.py`) или любого из трёх каталогов `.mo`;
* архитектура следует за `uname -m` (`x86_64`→`amd64`/`x86_64`,
  `aarch64`→`arm64`/`aarch64`).

Содержимое пакета (из `packaging/nfpm.yml`): `/usr/bin/linuxprinterg`,
`.desktop`, metainfo, иконки hicolor, деревья `/usr/share/locale/{uk,de,fr}`,
`LICENSE`. Жёсткие зависимости: `libgtk-4-1`/`gtk4` ≥ 4.14,
`libadwaita-1-0`/`libadwaita` ≥ 1.5, `cups-client`, `poppler-utils`;
Recommends: sane, LibreOffice, tesseract. `packaging/postinstall.sh` и
`preremove.sh` обновляют кэши рабочего стола и иконок.

## rpm из исходников (rpmbuild)

`packaging/linuxprinterg.spec` собирает RPM на Fedora штатными средствами:
`BuildRequires` cargo ≥ 1.85, gcc, `pkgconfig(gtk4)` ≥ 4.14,
`pkgconfig(libadwaita-1)` ≥ 1.5; `%build` выполняет `%make_build release`,
`%install` — `%make_install DESTDIR=%{buildroot}`.

## Flatpak

```bash
make flatpak    # flatpak-builder --force-clean --user --install …
```

Манифест: `crates/linuxprinterg/data/org.linuxprinterg.LinuxPrinterG.flatpak.yml`
(рантайм GNOME). Песочница блокирует USB и локальный сокет CUPS, поэтому в
манифесте зафиксированы **документированные исключения безопасности**:

* `--device=all` — SANE нужен доступ к USB для сканирования;
* `--share=network` — локальный CUPS (localhost:631) и сетевые принтеры;
* `--filesystem=xdg-download`, `--filesystem=xdg-documents` — папка сканов.

Эти права шире, чем у обычного пакета — это записанное исключение, а
**рекомендованная форма распространения — системный пакет** (.deb/.rpm).

## AppImage

```bash
linuxdeploy --appdir AppDir -e target/release/linuxprinterg \
  --plugin gtk --output appimage
```

## Установка в систему

```bash
sudo make install   # бинарник + .desktop + metainfo + иконки hicolor
```

## CI (.github/workflows/ci.yml)

| Задача | Что делает | Артефакт |
|---|---|---|
| `check` | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, интеграционные тесты (`--ignored`) | — |
| `build` | release-сборка | `linuxprinterg` (бинарник) |
| `packages` | `./scripts/build-packages.sh all` (nfpm скачивается в CI) | `installers` — `dist/*.deb`, `dist/*.rpm` |
| `flatpak` | flatpak-builder в контейнере с GNOME SDK | `linuxprinterg-x86_64.flatpak` |

Задачи `packages` и `flatpak` зависят от `check`: красный `check` блокирует
все артефакты.

## Планы распространения

* сделано: `.deb`/`.rpm` в репозитории, установщики из CI, Flatpak-бандл из
  CI;
* дорожная карта ([../ROADMAP.ru.md](../ROADMAP.ru.md)): **Flathub, Copr,
  AUR** — чтобы установка была одной командой на каждом целевом
  дистрибутиве;
* кандидаты, которых пока нет в дорожной карте: **PPA** для Debian/Ubuntu и
  репозиторий в духе AUR для Arch — решение нужно до публикации.

См. также: [../README.ru.md](../README.ru.md),
[../CONTRIBUTING.ru.md](../CONTRIBUTING.ru.md),
[MAINTENANCE.ru.md](MAINTENANCE.ru.md).
