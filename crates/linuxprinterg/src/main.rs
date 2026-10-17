//! Точка входа LinuxPrinterG.

mod app;
mod ctx;
mod pages;
mod tasks;
mod window;

use gio::prelude::ApplicationExtManual;
use linuxprinterg_core::storage::settings::Settings;
use linuxprinterg_core::{i18n, logging, storage};

fn main() {
    // Настройки загружаются до инициализации логов (уровень логирования — из них).
    let settings = Settings::load();
    let _log_guard = logging::init(&settings.log_level, &storage::log_dir());
    i18n::set_language(settings.language());

    tracing::info!("LinuxPrinterG запускается");

    // Виджеты libadwaita создаются в AppCtx до запуска приложения,
    // поэтому GTK/libadwaita инициализируется здесь (повторный вызов безопасен).
    if let Err(e) = adw::init() {
        tracing::error!("не удалось инициализировать GTK/libadwaita: {e}");
        std::process::exit(1);
    }

    let ctx = ctx::AppCtx::new(settings);
    let app = app::build_application(ctx);
    app.run_with_args::<&str>(&[]);

    tracing::info!("LinuxPrinterG завершает работу");
    drop(_log_guard);
}
