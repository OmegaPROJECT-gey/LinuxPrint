# Архитектура

**Как устроен LinuxPrinterG: workspace из двух крейтов Rust, где каждая строка
логики живёт вне GTK и тестируется без графического окружения.**

Приложение не поставляет собственных драйверов: оно управляет CUPS (`lp`,
`lpstat`, `cancel`, `lpoptions` плюс мини-IPP-клиент поверх HTTP) и SANE
(`scanimage`, включая backend `escl` для AirScan/eSCL) и отвечает понятной
инструкцией, когда утилиты не хватает.

---

## Workspace

Корневой `Cargo.toml` объявляет два крейта (resolver 2, edition 2024,
Rust ≥ 1.85, `version` в `[workspace.package]`):

| Крейт | Роль | Связан с GTK |
|---|---|---|
| `linuxprinterg-core` | всё, что не является виджетом: доменные типы, интеграция с CUPS/SANE/IPP, писатели PDF/TIFF, имена файлов, настройки, журнал, i18n | нет |
| `linuxprinterg` | само приложение: GTK4 ≥ 4.14, libadwaita ≥ 1.5, страницы, окно, мост асинхронности | да |

Ядро никогда не импортирует GTK, а UI-крейт никогда не разбирает вывод
`lpstat`. Именно поэтому `cargo test --workspace` проходит на безголовом
CI-раннере без display-сервера.

## Слои ядра

* `domain/` — чистые данные: `Printer`, `Scanner`, `Job`, запросы печати и
  сканирования, `page_range.rs` («1-3,7» → интервалы).
* `services/` — по модулю на внешнюю утилиту или формат файла: `cups.rs`
  (запуск и парсеры), `ipp.rs` (HTTP `application/ipp` к локальному cupsd,
  RFC 8010 вручную), `sane.rs` (ADF, клип кропа), `fit.rs` (автоподгон фото),
  `discover.rs` (DNS-SD через `avahi-browse`), `pdf.rs`/`save.rs`/`tiff.rs`
  (собственные писатели), `ocr.rs`, `convert.rs`, `preview.rs`, `naming.rs`,
  `proc.rs` (запуск процессов).
* `storage/` — JSON в XDG-каталогах с атомарной записью: `settings.rs`
  (serde + валидация), `journal.rs` (лог, экспорт CSV, потолок 1000 записей).
* `i18n.rs` / `po.rs` — словарь RU/EN с `tr()`/`trf()` и собственный
  читатель/писатель PO/MO для gettext-overlay uk/de/fr.
* `error.rs` — ошибки несут локализованное сообщение плюс инструкцию
  «что установить»; `logging.rs` — `tracing` в stderr и в файл с посуточной
  ротацией.

Внутри `core` нет ни виджета, ни главного цикла.

## Слои UI

`main.rs` (настройки → логи → i18n → запуск), `app.rs` (`AdwApplication`,
`APP_ID`, акселераторы `Ctrl+1..5`, `Ctrl+R`, `Ctrl+Q`), `window.rs`
(`AdwOverlaySplitView`, сайдбар сворачивается на узких окнах < 760 px,
ленивые страницы, `confirm()` для разрушительных действий), `ctx.rs`
(`AppCtx`: настройки, журнал, тосты, кэш устройств, общий для страниц),
`tasks.rs` (мост ниже) и `pages/` — `print.rs`, `scan.rs`, `devices.rs`,
`settings.rs`, `journal.rs`.

## Поток данных

1. Обработчик виджета вызывает `tasks::run_async(task, on_done)` либо
   `tasks::run_with_progress(task, on_progress, on_done)`.
2. `tasks.rs` создаёт `std::thread`; замыкание выполняет блокирующий вызов
   ядра с жёсткими таймаутами.
3. Результат возвращается по `async_channel`;
   `glib::MainContext::spawn_local` запускает `on_done`/`on_progress` в
   главном потоке GTK.
4. `on_done` обновляет виджеты — больше никто не имеет права трогать GTK вне
   главного потока.

Сканирование, OCR и конвертация используют канал прогресса плюс канал
результата; закрытие канала прогресса означает конец задачи.

**Отмена.** UI хранит `CancelHandle` — `Arc` + `Weak` на дочерний процесс;
«Отмена» убивает его, ядро возвращает `Error::Cancelled`. То же по таймауту
делает сам `proc.rs`.

## Внешние процессы — `services/proc.rs`

`lp`, `lpstat`, `cancel`, `lpoptions`, `scanimage`, `tesseract`, `soffice`,
`pdftoppm`, `avahi-browse` — всё идёт через один модуль с четырьмя правилами:

* **без shell** — программа и аргументы это вектор: инъектировать нечего;
* **жёсткий таймаут** — дочерний процесс убивается, возвращается
  `Error::Timeout`;
* **отмена** — `CancelHandle` убивает процесс по требованию UI;
* **`LC_ALL=C`** — детерминированный разбор вывода независимо от локали плюс
  потоки на чтение stdout/stderr, чтобы забитый pipe не дал дедлок.

## Дерево каталогов

```
linuxprinterg/
├── Cargo.toml                      # workspace (edition 2024, Rust ≥ 1.85)
├── Makefile                        # сборка, тесты, упаковка
├── rustfmt.toml
├── .github/workflows/ci.yml        # fmt + clippy + test + build + deb/rpm + flatpak
├── LICENSE                         # MIT
├── README.md
├── packaging/
│   ├── nfpm.yml                    # описание .deb/.rpm (nfpm)
│   ├── postinstall.sh              # кэши рабочего стола/иконок
│   ├── preremove.sh
│   └── linuxprinterg.spec          # альтернативная RPM-сборка (rpmbuild)
├── scripts/
│   ├── render-icons.py             # SVG → hicolor (librsvg + cairo)
│   ├── get-nfpm.sh                 # загрузка nfpm в target/tools
│   ├── build-packages.sh           # сборка .deb и .rpm
│   └── setup-fedora-dev-prefix.sh  # префикс GTK для песочницы без root
└── crates/
    ├── linuxprinterg-core/         # ЯДРО (без GTK)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── error.rs            # Error + локализованные сообщения/инструкции
    │       ├── i18n.rs             # словарь RU/EN, tr()/trf()
    │       ├── logging.rs          # tracing: stderr + ротируемый файл
    │       ├── po.rs               # свой читатель/писатель PO/MO (overlay uk/de/fr)
    │       ├── domain/
    │       │   ├── mod.rs          # Printer, Scanner, Job, запросы печати/скана
    │       │   └── page_range.rs   # «1-3,7» → интервалы (+тесты)
    │       ├── services/
    │       │   ├── proc.rs         # безопасный запуск процессов: без shell,
    │       │   │                    #   таймаут, отмена, LC_ALL=C
    │       │   ├── cups.rs         # lpstat/lp/cancel/lpoptions: сбор+парсеры
    │       │   ├── ipp.rs          # мини-IPP-клиент (HTTP POST к CUPS): точные
    │       │   │                    #   статусы, ручное кодирование RFC8010
    │       │   ├── sane.rs         # scanimage -L/--help/scan, ADF, кроп-клип
    │       │   ├── fit.rs          # автоподгон фото: медиа, ориентация, обрезка
    │       │   ├── discover.rs     # DNS-SD через avahi-browse (Mopria, AirScan)
    │       │   ├── pdf.rs          # многопостраничный PDF-писатель (Flate/DCT)
    │       │   ├── save.rs         # PNG/JPEG/TIFF/WebP
    │       │   ├── tiff.rs         # мульти-TIFF (IFD-цепочка + Adobe Deflate)
    │       │   ├── ocr.rs          # tesseract (язык санитаризуется)
    │       │   ├── convert.rs      # soffice --headless → PDF
    │       │   ├── preview.rs      # pdftoppm: первая страница PDF → PNG
    │       │   └── naming.rs       # шаблоны имён, уникальность, санитаризация
    │       └── storage/
    │           ├── mod.rs          # XDG-пути, атомарная запись
    │           ├── settings.rs     # Settings (serde) + валидация
    │           └── journal.rs      # JournalHandle: лог, экспорт, cap 1000
    └── linuxprinterg/              # UI (GTK4 4.14+ / libadwaita 1.5+)
        ├── Cargo.toml
        ├── data/
        │   ├── org.linuxprinterg.LinuxPrinterG.desktop
        │   ├── org.linuxprinterg.LinuxPrinterG.metainfo.xml
        │   ├── org.linuxprinterg.LinuxPrinterG.flatpak.yml
        │   └── icons/              # иконка приложения (SVG-исходник + hicolor)
        └── src/
            ├── main.rs             # настройки → логи → i18n → запуск
            ├── app.rs              # AdwApplication, APP_ID, акселераторы
            ├── window.rs           # OverlaySplitView, ленивые страницы, confirm()
            ├── ctx.rs              # AppCtx: настройки, журнал, тосты, кэш устройств
            ├── tasks.rs            # мост «блокирующая задача → главный поток GTK»
            └── pages/
                ├── mod.rs
                ├── print.rs        # печать: файлы, DnD, предпросмотр, очередь
                ├── scan.rs         # скан: параметры, предпросмотр, PDF/OCR
                ├── devices.rs      # принтеры/сканеры, добавление, свойства
                ├── settings.rs     # настройки (PreferencesPage)
                └── journal.rs      # журнал с автообновлением и экспортом
```

## Что это даёт

Тесты ядра идут без графики на фикстурах реального вывода утилит (а
IPP-клиент — на запросе, захваченном у libcups); состояние живёт в `AppCtx` и
не может разойтись с настройками и журналом; новая фича обычно касается трёх
мест — модуль `services`, `i18n.rs` + `ALL_KEYS` (и `po/*.po` для uk/de/fr) и
страница.

---

[English](ARCHITECTURE.md) | **Русский**
