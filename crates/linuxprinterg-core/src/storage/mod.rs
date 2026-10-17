//! Хранилище: настройки и журнал в JSON (XDG-каталоги).
//!
//! Выбран JSON вместо GSettings по обоснованным причинам:
//! * не требуется компиляция схемы (`glib-compile-schemas`) при установке из
//!   исходников и в AppImage;
//! * настройки переносятся копированием одного файла;
//! * нет жёсткой привязки к GObject для core-слоя, который тестируется без GTK.
//!
//! Формат совместим с GSettings по смыслу полей; миграция возможна позже.

pub mod journal;
pub mod settings;

use std::path::PathBuf;

use directories::ProjectDirs;

/// Идентификаторы проекта для XDG-каталогов.
pub fn project_dirs() -> ProjectDirs {
    // Fallback на текущий каталог, если XDG определён некорректно.
    ProjectDirs::from("org", "linuxprinterg", "LinuxPrinterG").unwrap_or_else(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        ProjectDirs::from_path(cwd).expect("рабочий каталог доступен")
    })
}

/// Каталог настроек (`~/.config/linuxprinterg`).
pub fn config_dir() -> PathBuf {
    project_dirs().config_dir().to_path_buf()
}

/// Каталог состояния (`~/.local/state/linuxprinterg`): журнал, логи.
pub fn state_dir() -> PathBuf {
    project_dirs()
        .state_dir()
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().join("state"))
}

/// Каталог логов.
pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

/// Каталог журнала операций.
pub fn journal_path() -> PathBuf {
    state_dir().join("journal.json")
}

/// Временный рабочий каталог приложения (предпросмотр, конвертация).
pub fn cache_dir() -> PathBuf {
    project_dirs().cache_dir().to_path_buf()
}

/// Каталог для сканов по умолчанию (`~/Documents/Scans` или локализованный).
pub fn default_scan_dir() -> PathBuf {
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(docs) = user_dirs.document_dir() {
            return docs.join("Scans");
        }
        return user_dirs.home_dir().join("Scans");
    }
    PathBuf::from(".").join("Scans")
}

/// Атомарная запись файла: временный файл + rename.
pub fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
