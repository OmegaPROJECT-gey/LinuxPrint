# LinuxPrinterG

**Быстрый минималистичный центр управления печатью и сканированием для Linux.**

LinuxPrinterG работает с принтерами и сканерами через системные службы Linux —
**CUPS** (`lp`, `lpstat`, `cancel`, `lpoptions`) и **SANE** (`scanimage`,
включая backend `escl` для AirScan/eSCL) — и использует уже установленные в
системе драйверы. Приложение **не содержит собственных проприетарных
драйверов** и не требует root-прав: добавление принтера выполняется
системным мастером (`system-config-printer`) или веб-интерфейсом CUPS, которые
сами запрашивают polkit-разрешение.

Целевые ОС: Ubuntu 24.04+, Fedora 40+, Debian 13+, Arch Linux.
Работает на Wayland и X11. Темы: светлая, тёмная, системная. Интерфейс:
русский и английский.

---

## Возможности

### Печать
* выбор принтера, drag-and-drop и выбор файлов: **PDF, изображения** —
  напрямую; **DOCX/ODT/RTF** — опциональная конвертация через
  LibreOffice headless (если установлен), иначе понятная подсказка;
* предпросмотр: изображения показываются напрямую, PDF рендерится первой
  страницей через `pdftoppm` (poppler-utils);
* параметры: копии, диапазон страниц (`1-3,7`), формат бумаги, ориентация,
  поля (через CUPS-опции драйвера), цвет/Ч-Б, duplex, качество, кассета,
  тип носителя — списки заполняются из **реальных** опций принтера
  (`lpoptions -p … -l`) с безопасными значениями по умолчанию;
* очередь печати: задания (активные и последние выполненные), размер,
  владелец, **точный IPP-статус** («в обработке», «приостановлено»,
  «выполнено», «сбой» …) через собственный IPP-клиент с откатом на `lpstat`,
  отмена задания (с подтверждением);
  **автообновление каждые 3 с** (прекращается после неудач, возобновляется
  кнопкой «Обновить»);
* **фотопрофиль в один клик**: качество «Фото», цвет, односторонняя печать и
  фотобумага — если принтер сообщает тип носителя;

### Сканирование
* выбор сканера (USB, сетевые, AirScan/eSCL через backend `escl`);
* источник (Flatbed / ADF / ADF Duplex — если заявлен сканером), разрешение,
  цветовой режим — списки из `scanimage --help`;
* многостраничное сканирование с ADF (в т.ч. «все страницы» с защитой от
  бесконечной петли), прогресс постранично **и по проценту**
  (`scanimage --progress` читается потоком из stderr), отмена в любой момент;
* кадрирование (область в мм, клип по границам устройства) и поворот 0/90/180/270°;
* сохранение: **PDF** (многостраничный, собственный писатель PDF: zlib для
  растра, DCTDecode для JPEG — без перекодирования), PNG, JPEG, TIFF, WebP;
* шаблон имени файла (`{base} {device} {date} {time} {page} {n} {ext}`) с
  санитаризацией и гарантией уникальности;
* опциональный **OCR через Tesseract** с выбором языка (отдельный .txt).

### Устройства
* список принтеров (состояние, URI, принтер по умолчанию) и сканеров;
* свойства устройства, установка принтера по умолчанию для пользователя
  (`lpoptions -d`, без root);
* «Добавить принтер»: `system-config-printer` → веб-интерфейс CUPS
  (`http://localhost:631/admin`) → текстовая инструкция;
* баннер при остановленной/недоступной службе CUPS.

### Настройки
* принтер и сканер по умолчанию, параметры печати и сканирования по умолчанию;
* тема (системная/светлая/тёмная), язык (авто/RU/EN), папка сканов,
  уровень логов, открытие папки логов, сброс настроек (с подтверждением);
* хранение — JSON в XDG-каталогах (`~/.config/linuxprinterg`,
  `~/.local/state/linuxprinterg`), атомарная запись.

### Журнал
* история операций: печать, сканирование, системные события, ошибки,
  пути сохранённых файлов; автообновление, **фильтр по категории**,
  экспорт в **CSV** (RFC 4180, открывается в LibreOffice/Excel) или
  текстовый файл, очистка (с подтверждением).

### Надёжность
* UI никогда не блокируется: все внешние вызовы — в фоновых потоках с
  доставкой результата в главный цикл GTK;
* жёсткие таймауты и отмена (kill) для `lp`, `scanimage`, `tesseract`,
  `soffice`, `pdftoppm`;
* понятные ошибки с инструкцией «что установить» (cups-client, sane-backends,
  tesseract, poppler-utils, libreoffice);
* все потенциально разрушающие действия — с подтверждением (отмена задания,
  очистка журнала, сброс настроек).

### Доступность
* полное управление с клавиатуры (все элементы — стандартные виджеты GTK4),
  акселераторы `Ctrl+1..5`, `Ctrl+R`, `Ctrl+Q`;
* labels/tooltip'ы для screen reader (GTK/AT-SPI), тёмная/светлая темы,
  читаемый контраст (палитра Adwaita);
* адаптивность: на узких окнах (< 760 px) боковая панель сворачивается
  в выдвижную (`AdwOverlaySplitView`).

---

## Архитектура

Workspace из двух крейтов — логика отделена от UI и полностью тестируется
без графического окружения:

```
linuxprinterg/
├── Cargo.toml                      # workspace (edition 2024, Rust ≥ 1.85)
├── Makefile                        # сборка, тесты, упаковка
├── rustfmt.toml
├── .github/workflows/ci.yml        # fmt + clippy + test + build
├── LICENSE                         # MIT
├── README.md
├── packaging/
│   └── linuxprinterg.spec          # RPM
└── crates/
    ├── linuxprinterg-core/         # ЯДРО (без GTK)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── error.rs            # Error + локализованные сообщения/инструкции
    │       ├── i18n.rs             # словарь RU/EN, tr()/trf()
    │       ├── logging.rs          # tracing: stderr + ротируемый файл
    │       ├── domain/
    │       │   ├── mod.rs          # Printer, Scanner, Job, запросы печати/скана
    │       │   └── page_range.rs   # «1-3,7» → интервалы (+тесты)
    │       ├── services/
    │       │   ├── proc.rs         # безопасный запуск процессов: без shell,
    │       │   │                    #   таймаут, отмена (CancelHandle), LC_ALL=C
    │       │   ├── cups.rs         # lpstat/lp/cancel/lpoptions: сбор+парсеры
    │       │   ├── ipp.rs          # мини-IPP-клиент (HTTP POST к CUPS): точные
    │       │   │                    #   статусы заданий, ручное кодирование RFC8010
    │       │   ├── sane.rs         # scanimage -L/--help/scan, ADF, кроп-клип
    │       │   ├── pdf.rs          # многопостраничный PDF-писатель (Flate/DCT)
    │       │   ├── save.rs         # PNG/JPEG/TIFF/WebP
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
        │   └── org.linuxprinterg.LinuxPrinterG.flatpak.yml
        └── src/
            ├── main.rs             # загрузка настроек → логи → i18n → запуск
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

**Поток данных.** UI → `tasks::run_async` (std::thread) → `linuxprinterg-core`
(синхронные вызовы утилит с таймаутами) → `async-channel` → главный поток GTK
(`glib::MainContext::spawn_local`). Отмена — через `CancelHandle`
(`Arc` + `Weak` на дочерний процесс, kill по требованию UI).

---

## Зависимости

Системные (все целевые дистрибутивы):

| Зачем | Debian/Ubuntu | Fedora | Arch |
|---|---|---|---|
| Печать | `cups-client`, `cups` | `cups-client`, `cups` | `cups`, `cups-client` |
| Сканирование | `sane-utils` | `sane-backends` | `sane` |
| AirScan/eSCL | `sane-airscan` | `sane-airscan` | `sane-airscan` |
| Предпросмотр PDF | `poppler-utils` | `poppler-utils` | `poppler` |
| OCR (опц.) | `tesseract-ocr`, `tesseract-ocr-rus` | `tesseract`, `tesseract-langpack-rus` | `tesseract`, `tesseract-data-rus` |
| DOCX→PDF (опц.) | `libreoffice` | `libreoffice*` | `libreoffice-still` |

Для сборки UI: `libgtk-4-dev >= 4.14` и `libadwaita-0-dev >= 1.5`
(Debian/Ubuntu) либо `gtk4-devel`, `libadwaita-devel` (Fedora),
`gtk4`, `libadwaita` (Arch). Rust **1.85+** (edition 2024).

---

## Сборка и запуск

```bash
# debug-сборка и запуск
cargo run --release --workspace

# или через Makefile
make build     # cargo build --workspace
make run       # cargo run --release
make release   # оптимизированная сборка (LTO, strip)

# Нет dev-пакетов GTK и нет root (песочница)? Извлеките их в префикс:
scripts/setup-fedora-dev-prefix.sh
export PKG_CONFIG_PATH="/tmp/gtkprefix/usr/lib64/pkgconfig:/tmp/gtkprefix/usr/share/pkgconfig"
export RUSTFLAGS="-L /tmp/gtkprefix/usr/lib64"
```

## Тесты и качество

```bash
make test       # cargo test --workspace        (юнит-тесты ядра)
make test-int   # интеграционные: реальные lpstat/scanimage/pdftoppm
make fmt-check  # rustfmt
make clippy     # clippy -D warnings
```

Покрыто тестами: парсеры `lpstat`/`lpoptions`/`scanimage -L/--help`
(на фикстурах реального вывода), разбор заданий очереди (активные/выполненные),
IPP-клиент целиком — кодирование Get-Jobs roundtrip-разбором, фикстура
**захваченного запроса libcups**, разбор групп ответа и `job-state` (enum
0x23), HTTP-разделение по Content-Length, имена серверов CUPS, живой тест
против запущенного CUPS (`cargo test -- --ignored`), потоковый вывод stderr
(колбэки прогресса), процент прогресса
`scanimage --progress`, диапазоны страниц, структура PDF
(корректность xref проверяется программно и рендером через `pdftoppm`),
шаблоны имён/санитаризация путей, журнал (текстовый и CSV-экспорт с
экранированием RFC 4180 и фильтром по категории), настройки (serde + валидация),
таймауты/отмена процессов, OCR-санитаризация языка, i18n (полнота словаря
RU/EN).

## Упаковка

```bash
# Flatpak (см. раздел «Ограничения Flatpak»)
make flatpak

# .deb (через cargo-deb: cargo install cargo-deb)
make deb

# RPM (через cargo-rpm; есть packaging/linuxprinterg.spec)
make rpm

# AppImage: linuxdeploy с GTK-плагином
linuxdeploy --appdir AppDir -e target/release/linuxprinterg \
  --plugin gtk --output appimage

# Системная установка
sudo make install          # /usr/bin/linuxprinterg + .desktop + metainfo
```

### Ограничения Flatpak

Песочница Flatpak закрывает доступ к USB и к локальному CUPS-сокету, поэтому
манифест `org.linuxprinterg.LinuxPrinterG.flatpak.yml` содержит явно
задокументированные исключения:

* `--device=all` — доступ к USB нужен SANE для сканирования;
* `--share=network` — локальный CUPS (localhost:631) и сетевые принтеры;
* `--filesystem=xdg-download`, `--filesystem=xdg-documents` — папки сканов.

Эти привилегии шире, чем у обычного пакета (.deb/.rpm), — это documented
security exception для Flatpak-сборки. Рекомендуемая форма распространения —
системный пакет.

---

## Конфигурация и данные

| Что | Где |
|---|---|
| Настройки | `~/.config/linuxprinterg/settings.json` |
| Журнал | `~/.local/state/linuxprinterg/journal.json` |
| Логи | `~/.local/state/linuxprinterg/logs/linuxprinterg.log` (ротация по суткам) |
| Кэш (предпросмотр, конвертация) | `~/.cache/linuxprinterg/` |
| Сканы по умолчанию | `~/Documents/Scans` |

Уровень логирования: настройка «Уровень логов» или переменная `RUST_LOG`.

## Локализация

Интерфейс: русский, английский (переключение в настройках, применяется сразу
для уведомлений и после перезапуска для всех строк). Словарь —
`crates/linuxprinterg-core/src/i18n.rs`, ключ → (en, ru); тест гарантирует
полноту обеих локалей. *Почему не gettext:* см. комментарий в модуле — нулевые
системные зависимости (важно для Flatpak/AppImage), отсутствие привязки к
`locale-archive`, полная проверяемость типов; формат пар совместим с
процессом gettext для будущей миграции.

---

## Безопасность

* внешние команды вызываются **без shell**, аргументы — вектором
  (защита от инъекций); `LC_ALL=C` для детерминированного разбора;
* жёсткие таймауты, kill зависших процессов, отмена пользователем;
* санитаризация имён файлов и языка OCR; пути к файлам ограничены
  выбранным каталогом (`naming::safe_join`);
* root не требуется: `lpadmin` не вызывается, пользовательские настройки —
  через `lpoptions`; все разрушительные действия — с подтверждением;
* Flatpak-исключения задокументированы выше.

## Известные ограничения (честно)

* «Поддержка всех принтеров и сканеров» = поддержка всего, что видит CUPS и
  SANE. Устройство без CUPS-драйвера или SANE-backend покажет понятную ошибку
  с инструкцией — приложение не обещает невозможного.
* Задания очереди: активные и последние выполненные (до 10) с точными
  IPP-статусами через собственный IPP-клиент (HTTP `application/ipp` к
  локальному CUPS); отмена — только для нетерминальных состояний. Если IPP
  недоступен (старый CUPS, выключенный сокет) — откат на `lpstat -W … -o`,
  который различает только грубые «активное/выполнено». %-готовность
  задания и «продолжить печать» — в дорожной карте.
* Многостраничный TIFF/WebP не поддерживается кодеками `image` — при
  многопостраничном сканировании создаются отдельные файлы с `{page}` в имени.
* Многостраничный скан с flatbed возможен только вручную (положил лист —
  нажал «Сканировать»); автоматическая подача — через ADF.
* Поля (поля печати) передаются как CUPS-опции драйвера; конкретный список
  зависит от принтера.

## Дорожная карта

1. **IPP-клиент** — базовая версия сделана (Get-Jobs, точные состояния
   заданий; см. `services/ipp.rs`): далее — %-готовность задания
   (`job-media-sheets-completed`), задержки, «продолжить печать» и
   D-Bus-уведомления о завершении (см. п. 2).
2. ~~Фоновое обновление очереди~~ — сделано опросом каждые 3 с; далее —
   уведомления о завершении задания через D-Bus CUPS и индикатор в боковой
   панели.
3. Мышинное кадрирование в предпросмотре (текущая реализация — точные поля
   в мм, что удобнее и доступнее).
4. Многостраничный TIFF, WebP-страницы в PDF через image-энкодер.
5. Mopria/eSCL-обнаружение через DNS-SD в addition к SANE-списку.
6. Печать фотографий: ICC-профили, 4×6/10×15 пресеты, бесконтрастная
   подгонка под лист.
7. ~~Экспорт журнала в CSV, фильтры по категории~~ — сделано.
8. Миграция i18n на gettext (извлечение .po из словаря), переводы (uk, de, fr).

## Лицензия

MIT — см. [LICENSE](LICENSE).
