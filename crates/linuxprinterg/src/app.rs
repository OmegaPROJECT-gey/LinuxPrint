//! Создание `AdwApplication`: идентификатор, действия, горячие клавиши.
//!
//! Действия и акселераторы (Ctrl+Q, Ctrl+R, Ctrl+1..5) устанавливаются в
//! [`crate::window`], где есть доступ к текущей странице и списку страниц.

use adw::prelude::*;

use crate::ctx::Ctx;
use crate::window;

pub const APP_ID: &str = "org.linuxprinterg.LinuxPrinterG";

pub fn build_application(ctx: Ctx) -> adw::Application {
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        // Повторное активирование (запуск второй копии) — показать окно заново.
        if let Some(win) = app.active_window() {
            win.present();
            return;
        }
        let win = window::build(app, &ctx);
        win.present();
    });

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app
}
