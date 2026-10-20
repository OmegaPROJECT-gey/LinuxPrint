//! Ядро LinuxPrinterG.
//!
//! Модули:
//! * [`domain`] — доменные типы (принтеры, сканеры, задания, запросы);
//! * [`services`] — интеграция с системными службами: CUPS (`lp`/`lpstat`),
//!   SANE/eSCL (`scanimage`), LibreOffice headless, Tesseract, `pdftoppm`,
//!   безопасный запуск процессов, шаблоны имён, запись PDF;
//! * [`storage] — настройки и журнал (JSON в XDG-каталогах);
//! * [`i18n`] — локализация RU/EN;
//! * [`logging] — инициализация `tracing` (stderr + ротируемый файл).
//!
//! Crate не зависит от GTK: вся логика тестируется без графического окружения.
pub mod domain;
pub mod error;
pub mod i18n;
pub mod logging;
pub mod po;
pub mod services;
pub mod storage;

pub use error::{Error, Result};
