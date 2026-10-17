//! Инициализация логирования: stderr + ротируемый файл (раз в сутки).
//!
//! Уровень управляется переменной `RUST_LOG` или настройкой `log_level`.

use std::path::Path;

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Инициализировать глобальный подписчик. Возвращает guard фонового писателя:
/// его нужно держать живым всё время работы приложения.
pub fn init(level: &str, log_dir: &Path) -> tracing_appender::non_blocking::WorkerGuard {
    if let Err(e) = std::fs::create_dir_all(log_dir) {
        eprintln!(
            "не удалось создать каталог логов {}: {e}",
            log_dir.display()
        );
    }
    let file_appender = tracing_appender::rolling::daily(log_dir, "linuxprinterg.log");
    let (nb_file, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "linuxprinterg_core={level},linuxprinterg={level},warn"
        ))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_writer(nb_file))
        .init();

    guard
}
