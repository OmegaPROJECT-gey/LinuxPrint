# Обслуживание

[English](MAINTENANCE.md) | **Русский**

Как в этом репозитории поддерживаются релизы, переводы, иконки и логи.
Обзор: [../README.ru.md](../README.ru.md); упаковка:
[PACKAGING.ru.md](PACKAGING.ru.md); переводы:
[LANGUAGES.ru.md](LANGUAGES.ru.md).

## Релизный процесс

1. **Версия** — workspace `Cargo.toml` (`[workspace.package] version = "…"` —
   единственный источник истины; `scripts/build-packages.sh` читает её для
   имён пакетов). Держите её в синхроне с `packaging/linuxprinterg.spec`
   (`Version:` + `%changelog`) — rpmbuild использует spec самостоятельно.
2. **Changelog** — блок `[Unreleased]` из `CHANGELOG.md` превращается в
   `## [X.Y.Z] — <дата>` (Keep a Changelog, SemVer), и то же самое
   делается в `CHANGELOG.ru.md` — оба файла в одном коммите.
3. **Проверки** — перед тегом должны быть зелёными все пункты ниже.
4. **Тег** — `git tag vX.Y.Z && git push --tags`.
5. **GitHub Release** — релиз для тега с артефактами CI (см. ниже): пакет
   `installers` (`dist/*.deb`, `dist/*.rpm`), бинарник `linuxprinterg` и
   `linuxprinterg-x86_64.flatpak`.

CI (`.github/workflows/ci.yml`) собирает эти артефакты на каждый push в
`main` и на pull request; красная задача `check` блокирует их.

## Чек-лист перед релизом

```bash
make fmt-check   # rustfmt
make clippy      # clippy -D warnings
make test        # юнит-тесты (core, без GTK)
make test-int    # интеграционные: настоящие lpstat/scanimage/pdftoppm
make release     # оптимизированная сборка (LTO, strip)
msgfmt --check po/uk.po po/de.po po/fr.po   # только внешняя проверка
```

Плюс: `make po-build` (файлы `.mo` обязаны существовать — сборка пакетов без
них останавливается) и после правки SVG —
`python3 scripts/render-icons.py` (нужны `python3-gi` + `librsvg2`); CI
падает, если в дереве нет масштабируемой иконки.

## Обновление переводов

```bash
make po         # перегенерировать POT; дописать недостающие ключи в po/{uk,de,fr}.po
# заполнить новые msgstr (существующие переводы не трогаются)
msgfmt --check po/<ll>.po
make po-build   # пересобрать locale/<ll>/LC_MESSAGES/linuxprinterg.mo
```

Изменение словаря (`crates/linuxprinterg-core/src/i18n.rs`) и обновление
`.po`/`.mo` принадлежат одному коммиту; полнота RU/EN закреплена тестами,
полнота uk/de/fr — задача человека, см. [LANGUAGES.ru.md](LANGUAGES.ru.md).

## Регенерация иконок

```bash
python3 scripts/render-icons.py   # SVG → hicolor 16…256 + scalable
```

Запускайте при любых изменениях источников в
`crates/linuxprinterg/data/icons/`; сборка `.deb`/`.rpm` проверяет наличие
результата.

## Логи и диагностика

| Что | Где |
|---|---|
| Лог приложения | `~/.local/state/linuxprinterg/logs/linuxprinterg.log` — ротация раз в сутки (новый файл каждый день, старые сохраняются) |
| Уровень логирования | Настройки → «Уровень логирования» (`error`/`warn`/`info`/`debug`) или переменная `RUST_LOG` (она перекрывает настройку) |
| Открыть каталог | Настройки → «Открыть логи» |
| Журнал (в UI) | события печати/сканирования/системы, фильтр по категориям, экспорт в CSV |
| Файл журнала | `~/.local/state/linuxprinterg/journal.json` (с ограничением, атомарная запись) |

Запускайте приложение из терминала, чтобы видеть stderr:

```bash
RUST_LOG=debug linuxprinterg
```

Проблемы CUPS — в `/var/log/cups/error_log`; сканер проверяется через
`scanimage -L` / `--help`. Рецепты устранения неполадок:
[PRINTING.ru.md](PRINTING.ru.md), [SCANNING.ru.md](SCANNING.ru.md).

## Обновление зависимостей

* `cargo update` обновляет закоммиченный `Cargo.lock`; после этого —
  `make fmt-check && make clippy && make test` и смоук-тест UI;
* пол версии: Rust **1.85** (edition 2024) — объявлен в `Cargo.toml`
  (`rust-version`) и в spec; пол UI: GTK4 ≥ 4.14, libadwaita ≥ 1.5 —
  повторены в `README.md`, `packaging/nfpm.yml`,
  `packaging/linuxprinterg.spec` и CI; меняйте всё вместе;
* системные зависимости рантайма (cups-client, sane, poppler, tesseract,
  libreoffice) описаны в таблице README, в nfpm `depends`/`recommends` и в
  spec — держите три источника в синхроне;
* версия рантайма Flatpak (`runtime-version` в манифесте) меняется
  осознанно: она должна совпадать с контейнером CI.

См. также: [../README.ru.md](../README.ru.md),
[../CONTRIBUTING.ru.md](../CONTRIBUTING.ru.md),
[../CHANGELOG.ru.md](../CHANGELOG.ru.md).
