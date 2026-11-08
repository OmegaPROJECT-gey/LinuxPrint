//! Создание `AdwApplication`: идентификатор, действия, горячие клавиши.
//!
//! Действия и акселераторы (Ctrl+Q, Ctrl+R, Ctrl+1..5) устанавливаются в
//! [`crate::window`], где есть доступ к текущей странице и списку страниц.

use adw::prelude::*;

use crate::ctx::Ctx;
use crate::window;

pub const APP_ID: &str = "org.linuxprinterg.LinuxPrinterG";

pub fn build_application(ctx: Ctx) -> adw::Application {
    // HANDLES_OPEN: запуск `linuxprinterg photo.jpg` (и «Открыть с помощью»
    // из файлового менеджера) передаёт файлы в primary-инстанс приложения.
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    {
        let ctx = ctx.clone();
        app.connect_activate(move |app| {
            // Повторное активирование (запуск второй копии) — показать окно заново.
            if let Some(win) = app.active_window() {
                win.present();
                return;
            }
            let win = window::build(app, &ctx);
            win.present();
        });
    }

    {
        let ctx = ctx.clone();
        app.connect_open(move |app, files, _hint| {
            // Окно могло ещё не построиться (первый запуск с файлом).
            if app.active_window().is_none() {
                let win = window::build(app, &ctx);
                win.present();
            }
            // Страница «Печать» создаётся лениво — показать и построить её.
            if let Some(action) = app.lookup_action("page1") {
                action.activate(None::<&glib::Variant>);
            }
            let Some(path) = files.first().and_then(|f| f.path()) else {
                return;
            };
            if let Some(handler) = ctx.open_handler.borrow().as_ref() {
                handler(path);
            } else {
                // Страница ещё строится — запомнить и отдадим при построении.
                *ctx.pending_file.borrow_mut() = Some(path);
            }
        });
    }

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app
}
