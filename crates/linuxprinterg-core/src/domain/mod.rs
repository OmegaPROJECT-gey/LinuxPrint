//! Доменные типы приложения.

pub mod page_range;

pub use page_range::PageRange;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Состояние принтера, как его сообщает `lpstat -p`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrinterState {
    #[default]
    Unknown,
    Idle,
    Printing,
    Disabled,
}

impl PrinterState {
    pub fn as_str(self) -> &'static str {
        match self {
            PrinterState::Unknown => "unknown",
            PrinterState::Idle => "idle",
            PrinterState::Printing => "printing",
            PrinterState::Disabled => "disabled",
        }
    }
}

/// Принтер, найденный системой CUPS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Printer {
    /// Имя принтера в CUPS (например, `HP_LaserJet_M15w`).
    pub name: String,
    pub state: PrinterState,
    pub is_default: bool,
    /// URI устройства из `lpstat -v`, например `ipp://...` или `usb://...`.
    pub uri: String,
    /// Описание/локация, если CUPS её сообщает.
    pub info: String,
}

/// Задание печати в очереди CUPS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Полное имя задания вида `PRINTER-23`.
    pub token: String,
    /// Числовой идентификатор задания.
    pub id: u32,
    pub printer: String,
    pub owner: String,
    /// Размер в байтах (0, если неизвестен).
    pub size: u64,
}

/// Сканер, найденный SANE (`scanimage -L`). Поддерживает и eSCL/AirScan
/// устройства, у которых в SANE установлен backend `escl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanner {
    /// Идентификатор SANE, например `escl:https://192.168.1.5:443` или
    /// `genesys:libusb:001:004`.
    pub id: String,
    /// Человекочитаемое имя устройства.
    pub name: String,
}

/// Прямоугольник в миллиметрах (область сканирования/кадрирования).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectMm {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Возможности сканера, извлечённые из `scanimage --help -d <id>`.
#[derive(Debug, Clone, Default)]
pub struct ScannerOptions {
    pub resolutions: Vec<u32>,
    pub modes: Vec<String>,
    pub sources: Vec<String>,
    /// Источники, поддерживающие автоматическую подачу двусторонних листов.
    pub duplex_sources: Vec<String>,
    /// Границы сканирования устройства, мм.
    pub bounds: Option<RectMm>,
    pub supports_batch: bool,
}

/// Поворот результата сканирования.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn degrees(self) -> u32 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }
}

/// Формат сохранения скана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Pdf,
    Png,
    Jpeg,
    Tiff,
    Webp,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Pdf => "pdf",
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Tiff => "tiff",
            OutputFormat::Webp => "webp",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "pdf" => Some(OutputFormat::Pdf),
            "png" => Some(OutputFormat::Png),
            "jpg" | "jpeg" => Some(OutputFormat::Jpeg),
            "tif" | "tiff" => Some(OutputFormat::Tiff),
            "webp" => Some(OutputFormat::Webp),
            _ => None,
        }
    }

    /// Многопостраничность поддерживает пока только PDF.
    pub fn is_multi_page(self) -> bool {
        matches!(self, OutputFormat::Pdf)
    }
}

/// Режим цвета сканирования.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    #[default]
    Color,
    Grayscale,
    Lineart,
}

impl ColorMode {
    /// Значение `--mode` для `scanimage`.
    pub fn as_scanimage(self) -> &'static str {
        match self {
            ColorMode::Color => "Color",
            ColorMode::Grayscale => "Grayscale",
            ColorMode::Lineart => "Lineart",
        }
    }

    pub fn from_scanimage(s: &str) -> Option<Self> {
        let l = s.to_ascii_lowercase();
        if l.contains("color") {
            Some(ColorMode::Color)
        } else if l.contains("gray") || l.contains("grey") {
            Some(ColorMode::Grayscale)
        } else if l.contains("lineart") || l.contains("line art") || l.contains("black and white") {
            Some(ColorMode::Lineart)
        } else {
            None
        }
    }
}

/// Параметры одного запроса сканирования.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub scanner_id: String,
    pub resolution: u32,
    pub mode: ColorMode,
    /// Точное имя режима из `scanimage --help` (некоторые backend'ы используют
    /// собственные имена, например «Gray» вместо «Grayscale»).
    pub mode_name: Option<String>,
    /// Источник: `Flatbed`, `ADF`, `ADF Duplex` и т. п.
    pub source: String,
    /// Область сканирования в мм (если None — весь лист/по умолчанию).
    pub crop: Option<RectMm>,
    pub rotation: Rotation,
    /// Число страниц; 0 — «все страницы» (работает только с ADF).
    pub pages: u32,
}

/// Режим цвета печати.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrintColorMode {
    #[default]
    Color,
    Grayscale,
    Auto,
}

impl PrintColorMode {
    /// Значение CUPS-опции `print-color-mode`.
    pub fn as_cups(self) -> &'static str {
        match self {
            PrintColorMode::Color => "color",
            PrintColorMode::Grayscale => "gray",
            PrintColorMode::Auto => "auto",
        }
    }
}

/// Качество печати (CUPS `print-quality`, 1..=5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrintQuality {
    Draft,
    #[default]
    Normal,
    High,
    Photo,
}

impl PrintQuality {
    pub fn as_cups(self) -> u8 {
        match self {
            PrintQuality::Draft => 1,
            PrintQuality::Normal => 3,
            PrintQuality::High => 5,
            PrintQuality::Photo => 5,
        }
    }
}

/// Ориентация страницы (CUPS `orientation-requested`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
    ReversePortrait,
    ReverseLandscape,
}

impl Orientation {
    pub fn as_cups(self) -> u8 {
        match self {
            Orientation::Portrait => 3,
            Orientation::Landscape => 4,
            Orientation::ReversePortrait => 6,
            Orientation::ReverseLandscape => 5,
        }
    }
}

/// Двусторонняя печать (CUPS `sides`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Sides {
    #[default]
    OneSided,
    TwoSidedLongEdge,
    TwoSidedShortEdge,
}

impl Sides {
    pub fn as_cups(self) -> &'static str {
        match self {
            Sides::OneSided => "one-sided",
            Sides::TwoSidedLongEdge => "two-sided-long-edge",
            Sides::TwoSidedShortEdge => "two-sided-short-edge",
        }
    }

    pub fn is_duplex(self) -> bool {
        !matches!(self, Sides::OneSided)
    }
}

/// Полный запрос на печать.
#[derive(Debug, Clone)]
pub struct PrintRequest {
    pub printer: String,
    pub file: PathBuf,
    pub title: String,
    pub copies: u32,
    /// Диапазон страниц в нотации CUPS, например `1-3,7`.
    pub page_range: Option<String>,
    /// Размер бумаги, например `A4` или `iso_a4_210x297mm`.
    pub media: Option<String>,
    pub sides: Sides,
    pub color_mode: PrintColorMode,
    pub quality: PrintQuality,
    pub orientation: Orientation,
    /// Кассета (значение опции `InputSlot`).
    pub tray: Option<String>,
    /// Тип носителя (значение опции `MediaType`).
    pub media_type: Option<String>,
}
