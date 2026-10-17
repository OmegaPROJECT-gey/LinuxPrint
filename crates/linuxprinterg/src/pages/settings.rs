//! Страница «Настройки»: значения по умолчанию, тема, язык, папки, логи, сброс.

use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use linuxprinterg_core::domain::{ColorMode, OutputFormat};
use linuxprinterg_core::i18n;
use linuxprinterg_core::storage::journal::Category;
use linuxprinterg_core::storage::settings::{LangChoice, Settings, Theme};

use crate::ctx::Ctx;
use crate::window::{PageHandle, confirm, make_handle};

const RESOLUTIONS: &[u32] = &[75, 100, 150, 200, 300, 600, 1200, 2400];
const MEDIA: &[&str] = &[
    "A4",
    "A5",
    "A3",
    "A6",
    "Letter",
    "Legal",
    "Executive",
    "4x6",
];
const FORMATS: &[&str] = &["PDF", "PNG", "JPEG", "TIFF", "WebP"];
const MODES: &[&str] = &["Color", "Grayscale", "Lineart"];
const OCR_LANGS: &[&str] = &["rus+eng", "rus", "eng", "deu+eng", "fra+eng", "spa+eng"];
const LOG_LEVELS_KEYS: &[&str] = &[
    "settings.log_level.error",
    "settings.log_level.warn",
    "settings.log_level.info",
    "settings.log_level.debug",
];
const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug"];

pub fn build(ctx: &Ctx) -> (gtk::Widget, PageHandle) {
    let page = adw::PreferencesPage::new();

    // --- Группа: значения по умолчанию ------------------------------------
    let defaults = adw::PreferencesGroup::builder()
        .title(&i18n::tr("settings.group_defaults"))
        .build();

    let printer_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.default_printer"))
        .model(&gtk::StringList::new(&[]))
        .build();
    let scanner_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.default_scanner"))
        .model(&gtk::StringList::new(&[]))
        .build();
    let media_row = adw::ComboRow::builder()
        .title(&i18n::tr("print.media"))
        .model(&gtk::StringList::new(MEDIA))
        .build();
    let resolutions: Vec<String> = RESOLUTIONS.iter().map(|r| r.to_string()).collect();
    let resolution_refs: Vec<&str> = resolutions.iter().map(String::as_str).collect();
    let resolution_row = adw::ComboRow::builder()
        .title(&i18n::tr("scan.resolution"))
        .model(&gtk::StringList::new(&resolution_refs))
        .build();
    let mode_row = adw::ComboRow::builder()
        .title(&i18n::tr("scan.mode"))
        .model(&gtk::StringList::new(MODES))
        .build();
    let format_row = adw::ComboRow::builder()
        .title(&i18n::tr("scan.format"))
        .model(&gtk::StringList::new(FORMATS))
        .build();
    let ocr_row = adw::SwitchRow::builder()
        .title(&i18n::tr("scan.ocr"))
        .build();
    let ocr_lang_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.ocr_lang"))
        .model(&gtk::StringList::new(OCR_LANGS))
        .build();
    let template_row = adw::EntryRow::builder()
        .title(&i18n::tr("scan.template"))
        .show_apply_button(true)
        .build();

    let folder_row = adw::ActionRow::builder()
        .title(&i18n::tr("settings.scan_dir"))
        .subtitle(&ctx.settings.borrow().scan.scan_dir)
        .activatable(true)
        .build();
    let folder_btn = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    folder_row.add_suffix(&folder_btn);

    defaults.add(&printer_row);
    defaults.add(&scanner_row);
    defaults.add(&media_row);
    defaults.add(&resolution_row);
    defaults.add(&mode_row);
    defaults.add(&format_row);
    defaults.add(&ocr_row);
    defaults.add(&ocr_lang_row);
    defaults.add(&template_row);
    defaults.add(&folder_row);
    page.add(&defaults);

    // --- Группа: оформление -----------------------------------------------
    let appearance = adw::PreferencesGroup::builder()
        .title(&i18n::tr("settings.group_appearance"))
        .build();
    let theme_labels = [
        i18n::tr("settings.theme.system"),
        i18n::tr("settings.theme.light"),
        i18n::tr("settings.theme.dark"),
    ];
    let theme_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.theme"))
        .model(&string_list(&theme_labels))
        .build();
    let language_labels = [
        i18n::tr("settings.language.auto"),
        i18n::tr("settings.language.ru"),
        i18n::tr("settings.language.en"),
    ];
    let language_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.language"))
        .model(&string_list(&language_labels))
        .build();
    appearance.add(&theme_row);
    appearance.add(&language_row);
    page.add(&appearance);

    // --- Группа: система ---------------------------------------------------
    let system = adw::PreferencesGroup::builder()
        .title(&i18n::tr("settings.group_system"))
        .build();
    let log_level_labels: Vec<String> = LOG_LEVELS_KEYS.iter().map(|k| i18n::tr(k)).collect();
    let log_level_refs: Vec<&str> = log_level_labels.iter().map(String::as_str).collect();
    let log_level_row = adw::ComboRow::builder()
        .title(&i18n::tr("settings.log_level"))
        .model(&gtk::StringList::new(&log_level_refs))
        .build();
    let logs_row = adw::ActionRow::builder()
        .title(&i18n::tr("settings.open_logs"))
        .activatable(true)
        .build();
    let logs_btn = gtk::Button::builder()
        .icon_name("folder-symbolic")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    logs_row.add_suffix(&logs_btn);
    let reset_row = adw::ActionRow::builder()
        .title(&i18n::tr("settings.reset"))
        .activatable(true)
        .build();
    let reset_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .css_classes(["flat", "destructive-action"])
        .valign(gtk::Align::Center)
        .build();
    reset_row.add_suffix(&reset_btn);
    system.add(&log_level_row);
    system.add(&logs_row);
    system.add(&reset_row);
    page.add(&system);

    let state = Rc::new(SettingsState {
        ctx: ctx.clone(),
        printer_row: printer_row.clone(),
        scanner_row: scanner_row.clone(),
        media_row: media_row.clone(),
        resolution_row: resolution_row.clone(),
        mode_row: mode_row.clone(),
        format_row: format_row.clone(),
        ocr_row: ocr_row.clone(),
        ocr_lang_row: ocr_lang_row.clone(),
        template_row: template_row.clone(),
        folder_row: folder_row.clone(),
        theme_row: theme_row.clone(),
        language_row: language_row.clone(),
        log_level_row: log_level_row.clone(),
    });

    // --- Инициализация значений из настроек --------------------------------
    state.load_values();

    // --- Сигналы -----------------------------------------------------------
    {
        let s = state.clone();
        printer_row.connect_selected_notify(move |row| {
            let name = combo_text(row);
            s.ctx.settings.borrow_mut().default_printer =
                name.filter(|n| n != &i18n::tr("common.none"));
            s.ctx.save_settings();
        });
    }
    {
        let s = state.clone();
        scanner_row.connect_selected_notify(move |row| {
            let id = combo_text(row);
            s.ctx.settings.borrow_mut().default_scanner =
                id.filter(|n| n != &i18n::tr("common.none"));
            s.ctx.save_settings();
        });
    }
    {
        let s = state.clone();
        media_row.connect_selected_notify(move |row| {
            if let Some(media) = combo_text(row) {
                s.ctx.settings.borrow_mut().print.media = media;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        resolution_row.connect_selected_notify(move |row| {
            let dpi = combo_text(row).and_then(|t| t.parse().ok());
            if let Some(dpi) = dpi {
                s.ctx.settings.borrow_mut().scan.resolution = dpi;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        mode_row.connect_selected_notify(move |row| {
            if let Some(mode) = combo_text(row).and_then(|m| ColorMode::from_scanimage(&m)) {
                s.ctx.settings.borrow_mut().scan.mode = mode;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        format_row.connect_selected_notify(move |row| {
            let fmt = combo_text(row).and_then(|f| match f.as_str() {
                "PNG" => Some(OutputFormat::Png),
                "JPEG" => Some(OutputFormat::Jpeg),
                "TIFF" => Some(OutputFormat::Tiff),
                "WebP" => Some(OutputFormat::Webp),
                _ => Some(OutputFormat::Pdf),
            });
            if let Some(fmt) = fmt {
                s.ctx.settings.borrow_mut().scan.format = fmt;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        ocr_row.connect_active_notify(move |row| {
            s.ctx.settings.borrow_mut().scan.ocr_enabled = row.is_active();
            s.ctx.save_settings();
        });
    }
    {
        let s = state.clone();
        ocr_lang_row.connect_selected_notify(move |row| {
            if let Some(lang) = combo_text(row) {
                s.ctx.settings.borrow_mut().scan.ocr_lang = lang;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        template_row.connect_changed(move |row| {
            let text = row.text().to_string();
            if !text.trim().is_empty() {
                s.ctx.settings.borrow_mut().scan.template = text;
                s.ctx.save_settings();
            }
        });
    }
    {
        let s = state.clone();
        folder_row.connect_activated(move |_| s.choose_folder());
    }
    {
        let s = state.clone();
        folder_btn.connect_clicked(move |_| s.choose_folder());
    }
    {
        let s = state.clone();
        theme_row.connect_selected_notify(move |row| {
            let theme = match row.selected() {
                1 => Theme::Light,
                2 => Theme::Dark,
                _ => Theme::System,
            };
            s.ctx.settings.borrow_mut().theme = theme;
            s.ctx.save_settings();
            apply_theme(theme);
        });
    }
    {
        let s = state.clone();
        language_row.connect_selected_notify(move |row| {
            let lang = match row.selected() {
                1 => LangChoice::Ru,
                2 => LangChoice::En,
                _ => LangChoice::Auto,
            };
            s.ctx.settings.borrow_mut().lang = lang;
            s.ctx.save_settings();
            i18n::set_language(lang.resolve());
            s.ctx.toast(&i18n::tr("settings.language_restart"));
        });
    }
    {
        let s = state.clone();
        log_level_row.connect_selected_notify(move |row| {
            if let Some(level) = LOG_LEVELS.get(row.selected() as usize) {
                s.ctx.settings.borrow_mut().log_level = level.to_string();
                s.ctx.save_settings();
                s.ctx.toast(&i18n::tr("settings.saved"));
            }
        });
    }
    {
        let s = state.clone();
        logs_row.connect_activated(move |_| s.open_logs());
    }
    {
        let s = state.clone();
        reset_btn.connect_clicked(move |_| {
            let window = s.ctx.parent();
            let do_reset = {
                let s = s.clone();
                move || {
                    if let Err(e) = Settings::reset() {
                        s.ctx.toast_error(&e);
                        return;
                    }
                    let defaults = Settings::default();
                    s.ctx.settings.replace(defaults);
                    i18n::set_language(s.ctx.settings.borrow().language());
                    apply_theme(s.ctx.settings.borrow().theme);
                    s.load_values();
                    s.ctx
                        .journal
                        .info(Category::Settings, i18n::tr("journal.entry.settings_reset"));
                    s.ctx.toast(&i18n::tr("journal.entry.settings_reset"));
                }
            };
            if let Some(window) = window {
                confirm(
                    &window,
                    &i18n::tr("settings.reset"),
                    &i18n::tr("settings.reset_confirm"),
                    do_reset,
                );
            } else {
                do_reset();
            }
        });
    }

    let refresh = {
        let s = state.clone();
        move || s.load_values()
    };
    let handle = make_handle(refresh);
    (page.upcast::<gtk::Widget>(), handle)
}

struct SettingsState {
    ctx: Ctx,
    printer_row: adw::ComboRow,
    scanner_row: adw::ComboRow,
    media_row: adw::ComboRow,
    resolution_row: adw::ComboRow,
    mode_row: adw::ComboRow,
    format_row: adw::ComboRow,
    ocr_row: adw::SwitchRow,
    ocr_lang_row: adw::ComboRow,
    template_row: adw::EntryRow,
    folder_row: adw::ActionRow,
    theme_row: adw::ComboRow,
    language_row: adw::ComboRow,
    log_level_row: adw::ComboRow,
}

impl SettingsState {
    /// Заполнить модели устройств и выставить текущие значения.
    fn load_values(self: &Rc<Self>) {
        let settings = self.ctx.settings.borrow().clone();

        // Принтеры.
        let mut printer_items = vec![i18n::tr("common.none")];
        let printers = self.ctx.printers.borrow().clone();
        printer_items.extend(printers.iter().map(|p| p.name.clone()));
        set_combo_items(&self.printer_row, &printer_items);
        let default_printer = self.ctx.effective_default_printer().unwrap_or_default();
        select_combo_text(&self.printer_row, &default_printer);

        // Сканеры.
        let mut scanner_items = vec![i18n::tr("common.none")];
        let scanners = self.ctx.scanners.borrow().clone();
        scanner_items.extend(scanners.iter().map(|s| s.id.clone()));
        set_combo_items(&self.scanner_row, &scanner_items);
        let default_scanner = self.ctx.effective_default_scanner().unwrap_or_default();
        select_combo_text(&self.scanner_row, &default_scanner);

        select_combo_text(&self.media_row, &settings.print.media);
        select_combo_text(&self.resolution_row, &settings.scan.resolution.to_string());
        select_combo_text(&self.mode_row, settings.scan.mode.as_scanimage());
        let fmt = match settings.scan.format {
            OutputFormat::Pdf => "PDF",
            OutputFormat::Png => "PNG",
            OutputFormat::Jpeg => "JPEG",
            OutputFormat::Tiff => "TIFF",
            OutputFormat::Webp => "WebP",
        };
        select_combo_text(&self.format_row, fmt);
        self.ocr_row.set_active(settings.scan.ocr_enabled);
        select_combo_text(&self.ocr_lang_row, &settings.scan.ocr_lang);
        self.template_row.set_text(&settings.scan.template);
        self.folder_row.set_subtitle(&settings.scan.scan_dir);
        self.theme_row.set_selected(match settings.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        self.language_row.set_selected(match settings.lang {
            LangChoice::Auto => 0,
            LangChoice::Ru => 1,
            LangChoice::En => 2,
        });
        let level_idx = LOG_LEVELS
            .iter()
            .position(|l| *l == settings.log_level)
            .unwrap_or(2) as u32;
        self.log_level_row.set_selected(level_idx);
    }

    fn choose_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(&i18n::tr("settings.scan_dir"))
            .build();
        let s = self.clone();
        dialog.select_folder(
            self.ctx.parent().as_ref(),
            gio::Cancellable::NONE,
            move |result| {
                if let Ok(folder) = result {
                    if let Some(path) = folder.path() {
                        let text = path.to_string_lossy().into_owned();
                        s.ctx.settings.borrow_mut().scan.scan_dir = text.clone();
                        s.ctx.save_settings();
                        s.folder_row.set_subtitle(&text);
                    }
                }
            },
        );
    }

    fn open_logs(self: &Rc<Self>) {
        let dir = linuxprinterg_core::storage::log_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.ctx
                .toast_error(&linuxprinterg_core::Error::io("создание каталога логов", e));
            return;
        }
        let uri = gio::File::for_path(&dir).uri();
        if let Err(e) =
            gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
        {
            self.ctx
                .toast_error(&linuxprinterg_core::Error::msg(e.to_string()));
        }
    }
}

fn combo_text(row: &adw::ComboRow) -> Option<String> {
    row.selected_item()
        .and_then(|o| o.downcast::<gtk::StringObject>().ok())
        .map(|s| s.string().to_string())
}

fn set_combo_items(row: &adw::ComboRow, items: &[String]) {
    let model = gtk::StringList::new(&[]);
    for item in items {
        model.append(item);
    }
    row.set_model(Some(&model));
}

/// `StringList` из массива строк (для моделей ComboRow).
fn string_list(labels: &[String]) -> gtk::StringList {
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&refs)
}

fn select_combo_text(row: &adw::ComboRow, value: &str) {
    let model = match row.model() {
        Some(m) => m,
        None => return,
    };
    for i in 0..model.n_items() {
        if let Some(text) = model
            .item(i)
            .and_then(|o| o.downcast::<gtk::StringObject>().ok())
            .map(|s| s.string())
        {
            if text == value {
                row.set_selected(i);
                return;
            }
        }
    }
}

/// Применить тему немедленно (системная/светлая/тёмная).
pub fn apply_theme(theme: Theme) {
    let manager = adw::StyleManager::default();
    let scheme = match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    };
    manager.set_color_scheme(scheme);
}
