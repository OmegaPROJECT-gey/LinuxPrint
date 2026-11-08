//! Общий контекст приложения: настройки, журнал, тосты, кэш устройств.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use linuxprinterg_core::domain::{Printer, Scanner};
use linuxprinterg_core::storage::journal::{Category, JournalHandle, Level};
use linuxprinterg_core::storage::settings::Settings;
use linuxprinterg_core::{i18n, services};

/// Обработчик открытия файла извне (`linuxprinterg photo.jpg`).
pub type OpenHandler = Box<dyn Fn(std::path::PathBuf)>;

/// Разделяемый контекст (один на всё приложение, главный поток).
pub struct AppCtx {
    pub settings: RefCell<Settings>,
    pub journal: JournalHandle,
    pub toasts: adw::ToastOverlay,
    /// Кэш принтеров (обновляется асинхронно).
    pub printers: RefCell<Vec<Printer>>,
    /// Кэш сканеров.
    pub scanners: RefCell<Vec<Scanner>>,
    /// Была ли первая загрузка устройств (ленивая инициализация).
    pub devices_loaded: Cell<bool>,
    /// Родительское окно для диалогов (устанавливается окном).
    pub window: RefCell<Option<adw::ApplicationWindow>>,
    /// Метка-счётчик активных заданий в сайдбаре (строка «Печать»).
    pub sidebar_badge: RefCell<Option<gtk::Label>>,
    /// Файл, переданный приложению при запуске (`linuxprinterg photo.jpg`,
    /// «Открыть с помощью»), пока страница «Печать» ещё не построена.
    pub pending_file: RefCell<Option<std::path::PathBuf>>,
    /// Обработчик открытия файла: страница «Печать» регистрирует его при
    /// построении (Weak-ссылкой на состояние — без циклов Rc).
    pub open_handler: RefCell<Option<OpenHandler>>,
}

pub type Ctx = Rc<AppCtx>;

impl AppCtx {
    pub fn new(settings: Settings) -> Ctx {
        Rc::new(AppCtx {
            settings: RefCell::new(settings),
            journal: JournalHandle::open_default(),
            toasts: adw::ToastOverlay::new(),
            printers: RefCell::new(Vec::new()),
            scanners: RefCell::new(Vec::new()),
            devices_loaded: Cell::new(false),
            window: RefCell::new(None),
            sidebar_badge: RefCell::new(None),
            pending_file: RefCell::new(None),
            open_handler: RefCell::new(None),
        })
    }

    /// Показать простое уведомление.
    pub fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// Показать ошибку: короткое сообщение + кнопка «Подробнее» с инструкцией.
    pub fn toast_error(&self, err: &linuxprinterg_core::Error) {
        tracing::warn!("ошибка UI: {err}");
        self.journal
            .log(Level::Error, Category::System, err.to_string(), None);
        let toast = adw::Toast::new(&err.ui_message());
        if let Some(help) = err.help_text() {
            toast.set_button_label(Some(&i18n::tr("common.details")));
            let help = help.clone();
            toast.connect_button_clicked(move |_| {
                show_message(None::<&gtk::Widget>, &i18n::tr("common.details"), &help);
            });
        }
        self.toasts.add_toast(toast);
    }

    /// Сохранить настройки на диск и отразить в журнале при ошибке.
    pub fn save_settings(&self) {
        let settings = self.settings.borrow().clone();
        if let Err(e) = settings.save() {
            self.toast_error(&e);
        }
    }

    /// Асинхронно обновить кэш принтеров и сканеров.
    ///
    /// `then` — замыкание, вызываемое в главном потоке после завершения
    /// (например, «обновить списки в UI»).
    pub fn refresh_devices(&self, then: Option<Box<dyn FnOnce()>>) {
        let ctx_self = Rc::clone(&self_rc(self));
        crate::tasks::run_async(
            || {
                let printers = match services::cups::list_printers() {
                    Ok(p) => Ok(p),
                    Err(e) if e.is_cancelled() => Err(e),
                    Err(e) => {
                        // CUPS может отсутствовать — не блокируем сканеры.
                        tracing::warn!("CUPS недоступен: {e}");
                        Ok(Vec::new())
                    }
                }?;
                let scanners = match services::sane::list_scanners() {
                    Ok(s) => Ok(s),
                    Err(e) => {
                        tracing::warn!("SANE недоступен: {e}");
                        Ok(Vec::new())
                    }
                }?;
                Ok((printers, scanners))
            },
            move |result| {
                ctx_self.devices_loaded.set(true);
                match result {
                    Ok((printers, scanners)) => {
                        *ctx_self.printers.borrow_mut() = printers;
                        *ctx_self.scanners.borrow_mut() = scanners;
                    }
                    Err(e) => ctx_self.toast_error(&e),
                }
                if let Some(then) = then {
                    then();
                }
            },
        );
    }

    /// Принтер по умолчанию: пользовательский выбор, затем CUPS.
    pub fn effective_default_printer(&self) -> Option<String> {
        if let Some(p) = &self.settings.borrow().default_printer {
            return Some(p.clone());
        }
        self.printers
            .borrow()
            .iter()
            .find(|p| p.is_default)
            .map(|p| p.name.clone())
    }

    /// Сканер по умолчанию: выбор пользователя, затем первый найденный.
    pub fn effective_default_scanner(&self) -> Option<String> {
        if let Some(s) = &self.settings.borrow().default_scanner {
            if self.scanners.borrow().iter().any(|x| &x.id == s) {
                return Some(s.clone());
            }
        }
        self.scanners.borrow().first().map(|s| s.id.clone())
    }

    /// Родитель для модальных диалогов (или None).
    pub fn parent(&self) -> Option<adw::ApplicationWindow> {
        self.window.borrow().clone()
    }

    /// Системное уведомление: freedesktop Notifications (D-Bus) через
    /// `gio::Notification`. `id` стабилен — повтор с тем же id заменяет
    /// старое уведомление вместо дублирования.
    pub fn notify(&self, id: &str, title: &str, body: Option<&str>) {
        let Some(window) = self.window.borrow().clone() else {
            return;
        };
        let Some(app) = window.application() else {
            return;
        };
        let notification = gio::Notification::new(title);
        notification.set_body(body);
        app.send_notification(Some(id), &notification);
    }

    /// Счётчик активных заданий в сайдбаре (0 — метка скрыта).
    pub fn set_queue_badge(&self, count: u32) {
        if let Some(badge) = self.sidebar_badge.borrow().as_ref() {
            badge.set_visible(count > 0);
            badge.set_label(&count.to_string());
        }
    }
}

/// Хак для получения `Rc<AppCtx>` внутри `impl AppCtx`: вызывается только из
/// методов, получивших `&self` через `Ctx` (см. `refresh_devices`).
fn self_rc(this: &AppCtx) -> Ctx {
    // SAFETY-замечание: метод вызывается исключительно из `Ctx` (Rc), и
    // `Rc::increment_strong_count`/`from_raw` восстанавливают тот же Rc.
    let ptr = this as *const AppCtx;
    unsafe {
        Rc::increment_strong_count(ptr);
        Rc::from_raw(ptr)
    }
}

/// Показать модальное сообщение (используется из тостов и страниц).
pub fn show_message(parent: Option<&impl IsA<gtk::Widget>>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("close")
        .build();
    dialog.add_response("close", &i18n::tr("common.close"));
    // MessageDialog — подкласс gtk::Window: родитель задаётся transient_for.
    if let Some(parent_widget) = parent {
        if let Some(parent_window) = parent_widget
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
        {
            dialog.set_transient_for(Some(&parent_window));
        }
    }
    dialog.present();
}
