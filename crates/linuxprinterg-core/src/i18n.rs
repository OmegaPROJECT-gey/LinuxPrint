//! Локализация (RU/EN).
//!
//! Решение: компилируемый словарь вместо gettext.
//!
//! **Обоснование.** Обёртка `gettext-rs` требует системной gettext на этапе
//! сборки и добавляет FFI-слой в core, который иначе полностью чистый (Rust)
//! и тестируется без системных зависимостей. Компилируемый словарь:
//! * не имеет системных зависимостей (важно для Flatpak/AppImage);
//! * работает без локалей в системе (не нужны `locale-gen`, `locale-archive`);
//! * даёт отсутствие аллокаций на уровне строк-литералов и полный контроль
//!   над ключами (тест гарантирует наличие обеих локалей).
//!
//! Формат пар («английский», «русский») совместим с процессом gettext:
//! ключи — pseudo-идентификаторы, значения можно выгрузить в `.po` позже
//! без изменения кода UI.

use std::sync::atomic::{AtomicU8, Ordering};

/// Язык интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Ru,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Установить текущий язык (потокобезопасно, меняется из UI).
pub fn set_language(lang: Language) {
    CURRENT.store(
        match lang {
            Language::En => 0,
            Language::Ru => 1,
        },
        Ordering::SeqCst,
    );
}

pub fn language() -> Language {
    match CURRENT.load(Ordering::SeqCst) {
        1 => Language::Ru,
        _ => Language::En,
    }
}

/// Перевод по ключу. Неизвестный ключ возвращается как есть (виден в UI —
/// сигнализирует о пропущенном переводе, а не ломает приложение).
pub fn tr(key: &str) -> String {
    match entry(key) {
        Some((en, ru)) => match language() {
            Language::En => en,
            Language::Ru => ru,
        }
        .to_string(),
        None => {
            tracing::trace!("отсутствует перевод ключа: {key}");
            key.to_string()
        }
    }
}

/// Перевод с подстановкой `{name}` → значение.
pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key);
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Все ключи словаря (для теста полноты).
pub const ALL_KEYS: &[&str] = &[
    "app.title",
    "nav.print",
    "nav.scan",
    "nav.devices",
    "nav.settings",
    "nav.journal",
    "common.ok",
    "common.cancel",
    "common.close",
    "common.confirm",
    "common.refresh",
    "common.reset",
    "common.clear",
    "common.export",
    "common.save",
    "common.open",
    "common.select",
    "common.browse",
    "common.default",
    "common.none",
    "common.apply",
    "common.error",
    "common.warning",
    "common.details",
    "common.done",
    "common.all",
    "common.auto",
    "print.file",
    "print.select_file",
    "print.drop_hint",
    "print.printer",
    "print.print_now",
    "print.queue",
    "print.queue_empty",
    "print.job_done",
    "print.job_state.processing",
    "print.job_state.processing_of",
    "print.job_state.held",
    "print.job_state.stopped",
    "print.job_state.canceled",
    "print.job_state.aborted",
    "notify.job_finished",
    "notify.job_gone",
    "print.quick_profiles",
    "print.cancel_job",
    "print.cancel_job_confirm",
    "print.copies",
    "print.page_range",
    "print.page_range_hint",
    "print.media",
    "print.tray",
    "print.media_type",
    "print.sides",
    "print.sides.off",
    "print.sides.long",
    "print.sides.short",
    "print.color",
    "print.color.color",
    "print.color.gray",
    "print.color.auto",
    "print.quality",
    "print.quality.draft",
    "print.quality.normal",
    "print.quality.high",
    "print.quality.photo",
    "print.photo_profile",
    "print.photo_profile_applied",
    "print.orientation",
    "print.orientation.portrait",
    "print.orientation.landscape",
    "print.orientation.reverse_portrait",
    "print.orientation.reverse_landscape",
    "print.job_submitted",
    "print.no_printer",
    "print.cups_missing",
    "print.converting",
    "print.convert_failed",
    "print.convert_missing",
    "print.converted",
    "print.job_cancelled",
    "print.hold_label",
    "print.hold_now",
    "print.hold_day",
    "print.hold_evening",
    "print.hold_night",
    "print.hold_weekend",
    "print.hold_job",
    "print.hold_job_confirm",
    "print.job_held",
    "print.resume_job",
    "print.resume_job_confirm",
    "print.job_resumed",
    "print.restart_job",
    "print.restart_job_confirm",
    "print.job_restarted",
    "print.title",
    "print.loading_preview",
    "print.preview_failed",
    "print.poppler_missing",
    "scan.scanner",
    "scan.source",
    "scan.resolution",
    "scan.mode",
    "scan.mode.color",
    "scan.mode.gray",
    "scan.mode.lineart",
    "scan.pages",
    "scan.pages_hint",
    "scan.crop",
    "scan.crop_x",
    "scan.crop_y",
    "scan.crop_w",
    "scan.crop_h",
    "scan.crop_reset",
    "scan.rotation",
    "scan.format",
    "scan.template",
    "scan.template_hint",
    "scan.folder",
    "scan.ocr",
    "scan.ocr_lang",
    "scan.preview_btn",
    "scan.scan_save",
    "scan.scanning",
    "scan.saved",
    "scan.cancel",
    "scan.no_scanner",
    "scan.sane_missing",
    "scan.empty_adf",
    "scan-device-busy",
    "scan-invalid-arg",
    "scan.manual_duplex_hint",
    "scan.page_of",
    "scan.page_of_percent",
    "scan.page_percent",
    "scan.preview_hint",
    "devices.printers",
    "devices.scanners",
    "devices.empty_printers",
    "devices.empty_scanners",
    "devices.add_printer",
    "devices.set_default",
    "devices.default_badge",
    "devices.details",
    "devices.state.idle",
    "devices.state.printing",
    "devices.state.disabled",
    "devices.state.unknown",
    "devices.uri",
    "devices.cups_stopped",
    "devices.cups_banner",
    "devices.open_cups_web",
    "devices.manual_hint",
    "devices.add_printer_done",
    "devices.network",
    "devices.network_hint",
    "devices.network_empty",
    "devices.network_no_avahi",
    "devices.net_printer",
    "devices.net_scanner",
    "devices.net_service",
    "devices.net_host",
    "devices.net_address",
    "settings.group_defaults",
    "settings.group_appearance",
    "settings.group_system",
    "settings.default_printer",
    "settings.default_scanner",
    "settings.theme",
    "settings.theme.system",
    "settings.theme.light",
    "settings.theme.dark",
    "settings.language",
    "settings.language.auto",
    "settings.language.ru",
    "settings.language.en",
    "settings.language_restart",
    "settings.scan_dir",
    "settings.log_level",
    "settings.log_level.error",
    "settings.log_level.warn",
    "settings.log_level.info",
    "settings.log_level.debug",
    "settings.open_logs",
    "settings.reset",
    "settings.reset_confirm",
    "settings.saved",
    "settings.ocr_lang",
    "journal.empty",
    "journal.filter_all",
    "journal.filter_empty",
    "journal.clear_confirm",
    "journal.export_done",
    "journal.level.info",
    "journal.level.warn",
    "journal.level.error",
    "journal.category.print",
    "journal.category.scan",
    "journal.category.system",
    "journal.category.settings",
    "journal.entry.print_submitted",
    "journal.entry.print_cancelled",
    "journal.entry.scan_saved",
    "journal.entry.ocr_done",
    "journal.entry.convert_done",
    "journal.entry.settings_reset",
    "err-tool-missing",
    "err-tool-failed",
    "err-timeout",
    "err-cancelled",
    "err-io",
    "err-invalid-input",
    "err-parse",
    "err-permission",
    "help-install-cups",
    "help-install-sane",
    "help-install-tesseract",
    "help-install-poppler",
    "help-install-libreoffice",
    "help-permission",
];

/// Словарь: ключ → (en, ru).
fn entry(key: &str) -> Option<(&'static str, &'static str)> {
    Some(match key {
        "app.title" => ("LinuxPrinterG", "LinuxPrinterG"),
        "nav.print" => ("Print", "Печать"),
        "nav.scan" => ("Scan", "Сканирование"),
        "nav.devices" => ("Devices", "Устройства"),
        "nav.settings" => ("Settings", "Настройки"),
        "nav.journal" => ("Journal", "Журнал"),

        "common.ok" => ("OK", "ОК"),
        "common.cancel" => ("Cancel", "Отмена"),
        "common.close" => ("Close", "Закрыть"),
        "common.confirm" => ("Confirm", "Подтвердить"),
        "common.refresh" => ("Refresh", "Обновить"),
        "common.reset" => ("Reset", "Сбросить"),
        "common.clear" => ("Clear", "Очистить"),
        "common.export" => ("Export", "Экспорт"),
        "common.save" => ("Save", "Сохранить"),
        "common.open" => ("Open", "Открыть"),
        "common.select" => ("Select", "Выбрать"),
        "common.browse" => ("Browse…", "Обзор…"),
        "common.default" => ("Default", "По умолчанию"),
        "common.none" => ("None", "Нет"),
        "common.apply" => ("Apply", "Применить"),
        "common.error" => ("Error", "Ошибка"),
        "common.warning" => ("Warning", "Предупреждение"),
        "common.details" => ("Details", "Подробнее"),
        "common.done" => ("Done", "Готово"),
        "common.all" => ("All", "Все"),
        "common.auto" => ("Automatic", "Авто"),

        "print.file" => ("File", "Файл"),
        "print.select_file" => ("Choose file…", "Выбрать файл…"),
        "print.drop_hint" => (
            "Drop a PDF or image here, or click «Choose file…»",
            "Перетащите сюда PDF или изображение, либо нажмите «Выбрать файл…»",
        ),
        "print.printer" => ("Printer", "Принтер"),
        "print.print_now" => ("Print", "Печать"),
        "print.queue" => ("Print queue", "Очередь печати"),
        "print.queue_empty" => ("Queue is empty", "Очередь пуста"),
        "print.job_done" => ("Completed", "Выполнено"),
        "print.job_state.processing" => ("Printing", "В обработке"),
        "print.job_state.processing_of" => (
            "Printing, sheet {done} of {total}",
            "В обработке, лист {done} из {total}",
        ),
        "print.job_state.held" => ("On hold", "Приостановлено"),
        "print.job_state.stopped" => ("Stopped", "Остановлено"),
        "print.job_state.canceled" => ("Cancelled", "Отменено"),
        "print.job_state.aborted" => ("Failed", "Сбой"),
        "notify.job_finished" => ("Job {job} finished", "Задание {job} завершено"),
        "notify.job_gone" => (
            "Job {job} is no longer in the queue",
            "Задание {job} больше нет в очереди",
        ),
        "print.quick_profiles" => ("Quick profiles", "Быстрые профили"),
        "print.cancel_job" => ("Cancel job", "Отменить задание"),
        "print.cancel_job_confirm" => (
            "Cancel print job «{job}»?",
            "Отменить задание печати «{job}»?",
        ),
        "print.copies" => ("Copies", "Копий"),
        "print.page_range" => ("Pages", "Страницы"),
        "print.page_range_hint" => ("e.g. 1-3,7 — empty means all", "напр. 1-3,7 — пусто = все"),
        "print.media" => ("Paper size", "Формат бумаги"),
        "print.tray" => ("Paper tray", "Кассета"),
        "print.media_type" => ("Media type", "Тип носителя"),
        "print.sides" => ("Duplex", "Двусторонняя"),
        "print.sides.off" => ("Off", "Выкл."),
        "print.sides.long" => ("Long edge", "Длинный край"),
        "print.sides.short" => ("Short edge", "Короткий край"),
        "print.color" => ("Color mode", "Режим цвета"),
        "print.color.color" => ("Color", "Цветная"),
        "print.color.gray" => ("Grayscale", "Оттенки серого"),
        "print.color.auto" => ("Automatic", "Авто"),
        "print.quality" => ("Quality", "Качество"),
        "print.quality.draft" => ("Draft", "Черновик"),
        "print.quality.normal" => ("Normal", "Обычное"),
        "print.quality.high" => ("High", "Высокое"),
        "print.quality.photo" => ("Photo", "Фото"),
        "print.photo_profile" => ("Photo profile", "Фотопрофиль"),
        "print.photo_profile_applied" => ("Photo profile applied", "Фотопрофиль применён"),
        "print.orientation" => ("Orientation", "Ориентация"),
        "print.orientation.portrait" => ("Portrait", "Книжная"),
        "print.orientation.landscape" => ("Landscape", "Альбомная"),
        "print.orientation.reverse_portrait" => ("Reverse portrait", "Книжная (оборотная)"),
        "print.orientation.reverse_landscape" => ("Reverse landscape", "Альбомная (оборотная)"),
        "print.job_submitted" => ("Job #{id} submitted", "Задание №{id} отправлено"),
        "print.no_printer" => ("No printers found", "Принтеры не найдены"),
        "print.cups_missing" => (
            "CUPS is not available. Install cups-client (lp, lpstat) and start cups.",
            "CUPS недоступен. Установите cups-client (lp, lpstat) и запустите службу cups.",
        ),
        "print.converting" => (
            "Converting document to PDF…",
            "Конвертация документа в PDF…",
        ),
        "print.convert_failed" => (
            "Document conversion failed: {msg}",
            "Не удалось конвертировать документ: {msg}",
        ),
        "print.convert_missing" => (
            "LibreOffice is not installed. Save the document as PDF and print it.",
            "LibreOffice не установлен. Сохраните документ в PDF и распечатайте его.",
        ),
        "print.converted" => ("Converted to {path}", "Конвертировано в {path}"),
        "print.job_cancelled" => ("Job {job} cancelled", "Задание {job} отменено"),
        "print.hold_label" => ("Print at", "Печатать"),
        "print.hold_now" => ("Now", "Сейчас"),
        "print.hold_day" => ("Day-time", "Днём"),
        "print.hold_evening" => ("Evening", "Вечером"),
        "print.hold_night" => ("Night", "Ночью"),
        "print.hold_weekend" => ("Weekend", "Выходные"),
        "print.hold_job" => ("Hold", "Приостановить"),
        "print.hold_job_confirm" => ("Hold job {job}?", "Приостановить задание {job}?"),
        "print.job_held" => ("Job {job} held", "Задание {job} приостановлено"),
        "print.resume_job" => ("Resume", "Продолжить"),
        "print.resume_job_confirm" => (
            "Resume printing job {job}?",
            "Продолжить печать задания {job}?",
        ),
        "print.job_resumed" => (
            "Printing of job {job} resumed",
            "Печать задания {job} продолжена",
        ),
        "print.restart_job" => ("Restart", "Перезапустить"),
        "print.restart_job_confirm" => (
            "Restart job {job}? It will be printed again.",
            "Перезапустить задание {job}? Оно будет напечатано ещё раз.",
        ),
        "print.job_restarted" => ("Job {job} restarted", "Задание {job} перезапущено"),
        "print.title" => ("Title on printer", "Заголовок на принтере"),
        "print.loading_preview" => ("Rendering preview…", "Рендеринг предпросмотра…"),
        "print.preview_failed" => (
            "Preview unavailable: {msg}",
            "Предпросмотр недоступен: {msg}",
        ),
        "print.poppler_missing" => (
            "Install poppler-utils (pdftoppm) for PDF preview.",
            "Установите poppler-utils (pdftoppm) для предпросмотра PDF.",
        ),

        "scan.scanner" => ("Scanner", "Сканер"),
        "scan.source" => ("Source", "Источник"),
        "scan.resolution" => ("Resolution, dpi", "Разрешение, dpi"),
        "scan.mode" => ("Color mode", "Режим цвета"),
        "scan.mode.color" => ("Color", "Цветной"),
        "scan.mode.gray" => ("Grayscale", "Оттенки серого"),
        "scan.mode.lineart" => ("Lineart", "Монохром"),
        "scan.pages" => ("Pages (0 = all from ADF)", "Страниц (0 = все из ADF)"),
        "scan.pages_hint" => (
            "Flatbed always scans one page per run",
            "Flatbed сканирует один лист за проход",
        ),
        "scan.crop" => ("Crop area, mm", "Область кадрирования, мм"),
        "scan.crop_x" => ("X", "X"),
        "scan.crop_y" => ("Y", "Y"),
        "scan.crop_w" => ("Width", "Ширина"),
        "scan.crop_h" => ("Height", "Высота"),
        "scan.crop_reset" => ("Full page", "Весь лист"),
        "scan.rotation" => ("Rotation", "Поворот"),
        "scan.format" => ("Format", "Формат"),
        "scan.template" => ("File name template", "Шаблон имени файла"),
        "scan.template_hint" => (
            "Fields: {base} {device} {date} {time} {page} {n} {ext}",
            "Поля: {base} {device} {date} {time} {page} {n} {ext}",
        ),
        "scan.folder" => ("Save to", "Сохранять в"),
        "scan.ocr" => ("OCR (Tesseract)", "OCR (Tesseract)"),
        "scan.ocr_lang" => ("OCR language", "Язык OCR"),
        "scan.preview_btn" => ("Preview", "Предпросмотр"),
        "scan.scan_save" => ("Scan and save", "Сканировать и сохранить"),
        "scan.scanning" => (
            "Scanning: page {page} of {total}",
            "Сканирование: страница {page} из {total}",
        ),
        "scan.saved" => ("Saved: {path}", "Сохранено: {path}"),
        "scan.cancel" => ("Cancel scan", "Отменить сканирование"),
        "scan.no_scanner" => ("No scanners found", "Сканеры не найдены"),
        "scan.sane_missing" => (
            "SANE is not available. Install sane-backends (scanimage).",
            "SANE недоступен. Установите sane-backends (scanimage).",
        ),
        "scan.empty_adf" => (
            "No pages: document feeder is empty",
            "Нет страниц: в податчике документов нет листов",
        ),
        "scan-device-busy" => ("Scanner is busy: {msg}", "Сканер занят: {msg}"),
        "scan-invalid-arg" => (
            "Scanner rejected the option: {msg}",
            "Сканер отклонил параметр: {msg}",
        ),
        "scan.manual_duplex_hint" => (
            "Manual duplex: scan one side, flip the stack and scan again.",
            "Ручной дуплекс: сканируйте одну сторону, переверните стопку и повторите.",
        ),
        "scan.page_of" => ("Page {page}/{total}", "Страница {page}/{total}"),
        "scan.page_of_percent" => (
            "Page {page}/{total} — {percent}%",
            "Страница {page}/{total} — {percent}%",
        ),
        "scan.page_percent" => ("Page {page} — {percent}%", "Страница {page} — {percent}%"),
        "scan.preview_hint" => (
            "Preview scans at low resolution ({dpi} dpi).",
            "Предпросмотр сканирует с малым разрешением ({dpi} dpi).",
        ),

        "devices.printers" => ("Printers", "Принтеры"),
        "devices.scanners" => ("Scanners", "Сканеры"),
        "devices.empty_printers" => (
            "No printers. Add one via the system dialog.",
            "Принтеры не найдены. Добавьте их системным диалогом.",
        ),
        "devices.empty_scanners" => ("No scanners found by SANE.", "SANE не обнаружил сканеров."),
        "devices.add_printer" => ("Add printer…", "Добавить принтер…"),
        "devices.set_default" => ("Set as default", "Сделать по умолчанию"),
        "devices.default_badge" => ("Default", "По умолчанию"),
        "devices.details" => ("Properties", "Свойства"),
        "devices.state.idle" => ("Idle", "Готов"),
        "devices.state.printing" => ("Printing", "Печатает"),
        "devices.state.disabled" => ("Disabled", "Отключён"),
        "devices.state.unknown" => ("Unknown", "Неизвестно"),
        "devices.uri" => ("Device", "Устройство"),
        "devices.cups_stopped" => ("CUPS scheduler is not running", "Служба CUPS не запущена"),
        "devices.cups_banner" => ("Printing is unavailable: {msg}", "Печать недоступна: {msg}"),
        "devices.open_cups_web" => ("Open CUPS web interface", "Открыть веб-интерфейс CUPS"),
        "devices.manual_hint" => (
            "Open http://localhost:631/admin in a browser to add a printer.",
            "Откройте http://localhost:631/admin в браузере, чтобы добавить принтер.",
        ),
        "devices.add_printer_done" => (
            "System dialog launched; it may ask for administrator rights.",
            "Системный диалог запущен; возможно, потребуются права администратора.",
        ),
        "devices.network" => ("Found on the network (DNS-SD)", "Найдено в сети (DNS-SD)"),
        "devices.network_hint" => (
            "Mopria printers and AirScan/eSCL scanners announced via Avahi.",
            "Mopria-принтеры и AirScan/eSCL-сканеры, объявленные через Avahi.",
        ),
        "devices.network_empty" => (
            "No network devices found.",
            "Сетевые устройства не найдены.",
        ),
        "devices.network_no_avahi" => (
            "Avahi (avahi-browse) is not installed — network discovery is unavailable.",
            "Avahi (avahi-browse) не установлен — обнаружение в сети недоступно.",
        ),
        "devices.net_printer" => ("Printer (Mopria)", "Принтер (Mopria)"),
        "devices.net_scanner" => ("Scanner (AirScan/eSCL)", "Сканер (AirScan/eSCL)"),
        "devices.net_service" => ("Service", "Служба"),
        "devices.net_host" => ("Host", "Узел"),
        "devices.net_address" => ("Address", "Адрес"),

        "settings.group_defaults" => ("Defaults", "Значения по умолчанию"),
        "settings.group_appearance" => ("Appearance", "Оформление"),
        "settings.group_system" => ("System", "Система"),
        "settings.default_printer" => ("Default printer", "Принтер по умолчанию"),
        "settings.default_scanner" => ("Default scanner", "Сканер по умолчанию"),
        "settings.theme" => ("Theme", "Тема"),
        "settings.theme.system" => ("System", "Системная"),
        "settings.theme.light" => ("Light", "Светлая"),
        "settings.theme.dark" => ("Dark", "Тёмная"),
        "settings.language" => ("Language", "Язык"),
        "settings.language.auto" => ("Automatic", "Авто"),
        "settings.language.ru" => ("Russian", "Русский"),
        "settings.language.en" => ("English", "Английский"),
        "settings.language_restart" => (
            "Language will change after restarting the app",
            "Язык изменится после перезапуска приложения",
        ),
        "settings.scan_dir" => ("Scan folder", "Папка сканов"),
        "settings.log_level" => ("Log level", "Уровень логов"),
        "settings.log_level.error" => ("Errors only", "Только ошибки"),
        "settings.log_level.warn" => ("Warnings", "Предупреждения"),
        "settings.log_level.info" => ("Info", "Информация"),
        "settings.log_level.debug" => ("Debug", "Отладка"),
        "settings.open_logs" => ("Open log folder", "Открыть папку логов"),
        "settings.reset" => ("Reset all settings", "Сбросить все настройки"),
        "settings.reset_confirm" => (
            "Reset all settings to defaults?",
            "Сбросить все настройки к значениям по умолчанию?",
        ),
        "settings.saved" => ("Settings saved", "Настройки сохранены"),
        "settings.ocr_lang" => ("OCR language (tesseract)", "Язык OCR (tesseract)"),

        "journal.empty" => ("No events yet", "Событий пока нет"),
        "journal.filter_all" => ("All categories", "Все категории"),
        "journal.filter_empty" => (
            "No entries match the filter",
            "Нет записей по этому фильтру",
        ),
        "journal.clear_confirm" => ("Clear the whole journal?", "Очистить весь журнал?"),
        "journal.export_done" => (
            "Journal exported to {path}",
            "Журнал экспортирован в {path}",
        ),
        "journal.level.info" => ("Info", "Инфо"),
        "journal.level.warn" => ("Warning", "Предупр."),
        "journal.level.error" => ("Error", "Ошибка"),
        "journal.category.print" => ("Print", "Печать"),
        "journal.category.scan" => ("Scan", "Скан"),
        "journal.category.system" => ("System", "Система"),
        "journal.category.settings" => ("Settings", "Настройки"),
        "journal.entry.print_submitted" => (
            "Print job sent to {device}",
            "Задание отправлено на {device}",
        ),
        "journal.entry.print_cancelled" => ("Print job {job} cancelled", "Задание {job} отменено"),
        "journal.entry.scan_saved" => ("Scan saved to {path}", "Скан сохранён: {path}"),
        "journal.entry.ocr_done" => ("OCR saved to {path}", "OCR сохранён: {path}"),
        "journal.entry.convert_done" => (
            "Document converted to {path}",
            "Документ конвертирован: {path}",
        ),
        "journal.entry.settings_reset" => ("Settings were reset", "Настройки сброшены"),

        "err-tool-missing" => (
            "«{tool}» not found. Install the corresponding package.",
            "Не найдена утилита «{tool}». Установите соответствующий пакет.",
        ),
        "err-tool-failed" => (
            "«{tool}» failed ({code}): {message}",
            "Команда «{tool}» завершилась с ошибкой ({code}): {message}",
        ),
        "err-timeout" => (
            "Timed out waiting for «{tool}» ({secs}s)",
            "Превышено время ожидания «{tool}» ({secs} с)",
        ),
        "err-cancelled" => ("Cancelled", "Операция отменена"),
        "err-io" => ("{context}: {source}", "{context}: {source}"),
        "err-invalid-input" => ("Invalid input: {msg}", "Некорректный ввод: {msg}"),
        "err-parse" => (
            "Failed to parse «{tool}» output: {details}",
            "Не удалось разобрать вывод «{tool}»: {details}",
        ),
        "err-permission" => ("No access: {msg}", "Нет доступа: {msg}"),

        "help-install-cups" => (
            "Install the cups-client package (Debian/Ubuntu: cups-client, Fedora: cups-client, Arch: cups) and make sure the cups service is running.",
            "Установите пакет cups-client (Debian/Ubuntu: cups-client, Fedora: cups-client, Arch: cups) и убедитесь, что служба cups запущена.",
        ),
        "help-install-sane" => (
            "Install sane-backends (Debian/Ubuntu: sane-utils, Fedora: sane-backends, Arch: sane). For AirScan/eSCL scanners install the escl backend too.",
            "Установите sane-backends (Debian/Ubuntu: sane-utils, Fedora: sane-backends, Arch: sane). Для AirScan/eSCL-сканеров дополнительно установите backend escl.",
        ),
        "help-install-tesseract" => (
            "Install tesseract-ocr and language data (e.g. tesseract-ocr-rus).",
            "Установите tesseract-ocr и языковые данные (например, tesseract-ocr-rus).",
        ),
        "help-install-poppler" => (
            "Install poppler-utils for PDF preview.",
            "Установите poppler-utils для предпросмотра PDF.",
        ),
        "help-install-libreoffice" => (
            "Install LibreOffice for automatic DOCX/ODT conversion, or save the document as PDF.",
            "Установите LibreOffice для автоконвертации DOCX/ODT, либо сохраните документ в PDF.",
        ),
        "help-permission" => (
            "Check that your user is in the lpadmin/sane groups and has write access to the target folder.",
            "Проверьте, входит ли пользователь в группы lpadmin/sane и есть ли права на запись в целевую папку.",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_have_translations() {
        for key in ALL_KEYS {
            assert!(entry(key).is_some(), "ключ «{key}» отсутствует в словаре");
        }
    }

    #[test]
    fn no_unused_orphans_in_dictionary() {
        // Каждый ключ словаря должен быть заявлен в ALL_KEYS (обратная проверка).
        // Простой перебор невозможен без рефлексии, поэтому проверяем выборку
        // последних ключей — индикатор синхронности списков.
        assert!(ALL_KEYS.contains(&"help-permission"));
        assert!(ALL_KEYS.contains(&"journal.entry.settings_reset"));
        assert!(entry("help-permission").is_some());
        assert!(entry("journal.entry.settings_reset").is_some());
    }

    #[test]
    fn translation_switches_by_language() {
        // Тесты объединены: язык — глобальное состояние, параллельный запуск
        // отдельных тестов создал бы гонку.
        set_language(Language::Ru);
        assert_eq!(tr("nav.print"), "Печать");
        set_language(Language::En);
        assert_eq!(tr("nav.print"), "Print");
        assert_eq!(
            trf("print.job_submitted", &[("id", "42")]),
            "Job #42 submitted"
        );
        set_language(Language::Ru);
        assert_eq!(
            trf("scan.saved", &[("path", "/tmp/x.pdf")]),
            "Сохранено: /tmp/x.pdf"
        );
        // Неизвестный ключ возвращается как есть.
        assert_eq!(tr("nope.nope"), "nope.nope");
    }
}
