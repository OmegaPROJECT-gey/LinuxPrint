//! Настройки приложения (JSON, атомарная запись).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{ColorMode, Orientation, OutputFormat, PrintColorMode, PrintQuality, Sides};
use crate::error::{Error, Result};
use crate::i18n::Language;
use crate::storage;

/// Выбор темы оформления.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Выбор языка интерфейса.
///
/// `Uk`/`De`/`Fr` идут через gettext: рантайм подхватывает `.mo`
/// (см. [`crate::i18n::set_language`]); пока перевода нет — английский.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LangChoice {
    #[default]
    Auto,
    Ru,
    En,
    Uk,
    De,
    Fr,
}

impl LangChoice {
    /// Определить язык по окружению (LC_ALL → LC_MESSAGES → LANG).
    pub fn resolve(self) -> Language {
        match self {
            LangChoice::Ru => Language::Ru,
            LangChoice::En => Language::En,
            LangChoice::Uk => Language::Uk,
            LangChoice::De => Language::De,
            LangChoice::Fr => Language::Fr,
            LangChoice::Auto => {
                let raw = std::env::var("LC_ALL")
                    .or_else(|_| std::env::var("LC_MESSAGES"))
                    .or_else(|_| std::env::var("LANG"))
                    .unwrap_or_default();
                detect_language(&raw)
            }
        }
    }
}

/// Определить язык по строке окружения (LC_ALL / LC_MESSAGES / LANG).
fn detect_language(raw: &str) -> Language {
    let raw = raw.to_ascii_lowercase();
    for (prefix, lang) in [
        ("ru", Language::Ru),
        ("uk", Language::Uk),
        ("de", Language::De),
        ("fr", Language::Fr),
    ] {
        if raw.starts_with(prefix) {
            return lang;
        }
    }
    Language::En
}

/// Параметры печати по умолчанию.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintDefaults {
    pub copies: u32,
    /// Размер бумаги: значение опции `PageSize` (например, `A4`).
    pub media: String,
    pub sides: Sides,
    pub color_mode: PrintColorMode,
    pub quality: PrintQuality,
    pub orientation: Orientation,
    /// Автоподгон фото: подбор формата бумаги под снимок, заполнение
    /// листа с обрезкой и альбомная ориентация (аналог Canon PRINT).
    pub auto_fit: bool,
}

impl Default for PrintDefaults {
    fn default() -> Self {
        PrintDefaults {
            copies: 1,
            media: "A4".to_string(),
            sides: Sides::OneSided,
            color_mode: PrintColorMode::Color,
            quality: PrintQuality::Normal,
            orientation: Orientation::Portrait,
            auto_fit: true,
        }
    }
}

/// Параметры сканирования по умолчанию.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanDefaults {
    pub resolution: u32,
    pub mode: ColorMode,
    /// Источник: `Flatbed`, `ADF`, `ADF Duplex` …
    pub source: String,
    pub format: OutputFormat,
    /// Шаблон имени файла, см. [`crate::services::naming`].
    pub template: String,
    /// Каталог сохранения сканов (строка, путь должен быть абсолютным).
    pub scan_dir: String,
    pub ocr_enabled: bool,
    /// Язык(и) OCR для Tesseract, например `rus+eng`.
    pub ocr_lang: String,
    /// Разрешение предпросмотра (быстрое сканирование).
    pub preview_dpi: u32,
}

impl Default for ScanDefaults {
    fn default() -> Self {
        ScanDefaults {
            resolution: 300,
            mode: ColorMode::Color,
            source: "Flatbed".to_string(),
            format: OutputFormat::Pdf,
            template: "{date}_{time}_{base}".to_string(),
            scan_dir: storage::default_scan_dir().to_string_lossy().into_owned(),
            ocr_enabled: false,
            ocr_lang: "rus+eng".to_string(),
            preview_dpi: 96,
        }
    }
}

/// Все настройки приложения.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub lang: LangChoice,
    pub default_printer: Option<String>,
    pub default_scanner: Option<String>,
    pub print: PrintDefaults,
    pub scan: ScanDefaults,
    /// Уровень логирования: error | warn | info | debug.
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            lang: LangChoice::Auto,
            default_printer: None,
            default_scanner: None,
            print: PrintDefaults::default(),
            scan: ScanDefaults::default(),
            log_level: "info".to_string(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        storage::config_dir().join("settings.json")
    }

    /// Загрузить настройки; при любой ошибке — значения по умолчанию.
    pub fn load() -> Self {
        let path = Self::path();
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default();
        let mut settings = settings;
        settings.validate();
        settings
    }

    /// Сохранить настройки (атомарно).
    pub fn save(&self) -> Result<()> {
        let mut sanitized = self.clone();
        sanitized.validate();
        let data = serde_json::to_string_pretty(&sanitized)
            .map_err(|e| Error::msg(format!("сериализация настроек: {e}")))?;
        storage::atomic_write(&Self::path(), data.as_bytes())
            .map_err(|e| Error::io("запись настроек", e))
    }

    /// Удалить файл настроек (сброс к заводским).
    pub fn reset() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| Error::io("сброс настроек", e))?;
        }
        Ok(())
    }

    /// Привести значения к допустимым (защита от правки файла руками).
    pub fn validate(&mut self) {
        self.print.copies = self.print.copies.clamp(1, 999);
        self.scan.resolution = self.scan.resolution.clamp(50, 2400);
        self.scan.preview_dpi = self.scan.preview_dpi.clamp(50, 300);
        if self.scan.template.trim().is_empty() {
            self.scan.template = ScanDefaults::default().template;
        }
        if self.scan.ocr_lang.trim().is_empty() {
            self.scan.ocr_lang = "eng".to_string();
        }
        if !matches!(
            self.log_level.as_str(),
            "error" | "warn" | "info" | "debug" | "trace"
        ) {
            self.log_level = "info".to_string();
        }
        // Путь каталога сканов должен быть абсолютным.
        let dir = PathBuf::from(&self.scan.scan_dir);
        if !dir.is_absolute() || self.scan.scan_dir.trim().is_empty() {
            self.scan.scan_dir = storage::default_scan_dir().to_string_lossy().into_owned();
        }
        if self.print.media.trim().is_empty() {
            self.print.media = "A4".to_string();
        }
    }

    pub fn scan_dir(&self) -> PathBuf {
        PathBuf::from(&self.scan.scan_dir)
    }

    pub fn log_dir(&self) -> PathBuf {
        storage::log_dir()
    }

    /// Язык для i18n.
    pub fn language(&self) -> Language {
        self.lang.resolve()
    }

    /// Создать каталог сканов при необходимости.
    pub fn ensure_scan_dir(&self) -> Result<PathBuf> {
        let dir = self.scan_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::io(format!("создание {}", dir.display()), e))?;
        Ok(dir)
    }

    /// Проверить, что каталог существует и в него можно писать.
    pub fn check_writable_dir(dir: &Path) -> Result<()> {
        if !dir.is_dir() {
            return Err(Error::InvalidInput(format!(
                "каталог не существует: {}",
                dir.display()
            )));
        }
        let probe = dir.join(".linuxprinterg-write-test");
        std::fs::write(&probe, b"ok")
            .map_err(|_| Error::Permission(format!("нет прав на запись в {}", dir.display())))?;
        let _ = std::fs::remove_file(&probe);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_sane() {
        let s = Settings::default();
        assert_eq!(s.print.copies, 1);
        assert_eq!(s.scan.resolution, 300);
        assert_eq!(s.scan.template, "{date}_{time}_{base}");
        assert!(
            Path::new(&s.scan.scan_dir).is_absolute(),
            "{}",
            s.scan.scan_dir
        );
    }

    #[test]
    fn roundtrip_json() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings {
            theme: Theme::Dark,
            lang: LangChoice::Ru,
            default_printer: Some("HP".into()),
            scan: ScanDefaults {
                resolution: 600,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // Запись и чтение через файл (без правки реального профиля).
        let path = dir.path().join("settings.json");
        std::fs::write(&path, &json).unwrap();
        let loaded: Settings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.theme, Theme::Dark);
    }

    #[test]
    fn validation_fixes_bad_values() {
        let mut s = Settings {
            print: PrintDefaults {
                copies: 0,
                media: "  ".into(),
                ..Default::default()
            },
            scan: ScanDefaults {
                resolution: 100000,
                template: "   ".into(),
                scan_dir: "относительный/путь".into(),
                ocr_lang: "".into(),
                ..Default::default()
            },
            log_level: "взлом".into(),
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.print.copies, 1);
        assert_eq!(s.print.media, "A4");
        assert_eq!(s.scan.resolution, 2400);
        assert_eq!(s.scan.template, "{date}_{time}_{base}");
        assert_eq!(s.log_level, "info");
        assert_eq!(s.scan.ocr_lang, "eng");
        assert!(Path::new(&s.scan.scan_dir).is_absolute());
    }

    #[test]
    fn unknown_fields_ignored_and_missing_get_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark","будущее":42}"#).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.scan.resolution, 300);
    }
}
