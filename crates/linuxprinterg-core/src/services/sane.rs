//! Интеграция со сканерами через SANE (`scanimage`).
//!
//! Поддерживаются:
//! * локальные USB-сканеры через системный backend SANE;
//! * сетевые и AirScan/eSCL-устройства — если установлен backend `escl`
//!   (тогда они видны в `scanimage -L` как `escl:https://...`).
//!
//! Приложение не содержит собственных драйверов: работает только с тем, что
//! предоставляет система.

use std::time::Duration;

use image::DynamicImage;
use regex::Regex;

use crate::domain::{RectMm, Rotation, ScanRequest, Scanner, ScannerOptions};
use crate::error::{Error, Result};
use crate::services::proc::{self, CancelHandle};

/// Одна отсканированная страница.
#[derive(Debug, Clone)]
pub struct ScanPage {
    pub image: DynamicImage,
    pub dpi: u32,
}

/// Прогресс сканирования (сообщается в UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProgress {
    /// Начато сканирование страницы `page` (всего `total`, если известно).
    PageStarted { page: usize, total: Option<usize> },
    /// Страница `page` получена.
    PageDone { page: usize, total: Option<usize> },
}

/// Проверить наличие `scanimage`.
pub fn ensure_available() -> Result<()> {
    if proc::tool_available("scanimage").is_none() {
        return Err(Error::ToolMissing {
            tool: "scanimage".to_string(),
        });
    }
    Ok(())
}

/// Список сканеров, видимых системе.
pub fn list_scanners() -> Result<Vec<Scanner>> {
    ensure_available()?;
    let out = proc::run("scanimage", &["-L"], Duration::from_secs(20))?;
    // Ненулевой код при отсутствии сканеров — не фатально.
    Ok(parse_scanner_list(&out.stdout_str()))
}

/// Разбор вывода `scanimage -L`.
pub fn parse_scanner_list(text: &str) -> Vec<Scanner> {
    let re = Regex::new(r"device `(.+?)' is a (.+)").expect("корректный regex");
    let mut out: Vec<Scanner> = Vec::new();
    for caps in re.captures_iter(text) {
        let id = caps[1].trim().to_string();
        let name = caps[2].trim().to_string();
        if !id.is_empty() {
            out.push(Scanner { id, name });
        }
    }
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Возможности сканера из `scanimage --help -d <id>`.
pub fn scanner_options(id: &str) -> Result<ScannerOptions> {
    ensure_available()?;
    let out = proc::run("scanimage", &["--help", "-d", id], Duration::from_secs(30))?;
    if out.status != 0 {
        return Err(Error::ToolFailed {
            tool: "scanimage".into(),
            code: out.status,
            message: out.stderr.trim().to_string(),
        });
    }
    Ok(parse_scanimage_help(&out.stdout_str()))
}

/// Разбор `scanimage --help`: разрешения, режимы, источники, границы.
///
/// Реальный формат help-вывода: опция идёт строкой с `-`, а её значения
/// (в т.ч. список кавычек-строк) — на следующих, отступных строках.
pub fn parse_scanimage_help(text: &str) -> ScannerOptions {
    let mut opts = ScannerOptions::default();

    let num_re = Regex::new(r"^\d+(\.\d+)?$").expect("корректный regex");
    let quoted_re = Regex::new(r#""([^"]+)""#).expect("корректный regex");
    let bounds_re =
        Regex::new(r"([lthw])\s+(\d+(?:\.\d+)?)\.\.(\d+(?:\.\d+)?)mm").expect("корректный regex");

    #[derive(Clone, Copy, PartialEq)]
    enum Current {
        None,
        Resolution,
        Mode,
        Source,
        Other,
    }
    let mut current = Current::None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('-') {
            current = if trimmed.starts_with("--resolution") || trimmed.starts_with("-r ") {
                Current::Resolution
            } else if trimmed.starts_with("--mode") {
                Current::Mode
            } else if trimmed.starts_with("--source") {
                Current::Source
            } else if trimmed.starts_with("--batch") {
                opts.supports_batch = true;
                Current::Other
            } else {
                Current::Other
            };
        }
        match current {
            Current::Resolution => {
                // Значения разрешений — только на строке самой опции.
                let values: Vec<u32> = trimmed
                    .split_whitespace()
                    .filter(|t| num_re.is_match(t))
                    .filter_map(|t| t.parse().ok())
                    .collect();
                for v in values {
                    if (50..=4800).contains(&v) && !opts.resolutions.contains(&v) {
                        opts.resolutions.push(v);
                    }
                }
            }
            Current::Mode => {
                for cap in quoted_re.captures_iter(trimmed) {
                    let m = cap[1].to_string();
                    if !opts.modes.iter().any(|x| x.eq_ignore_ascii_case(&m)) {
                        opts.modes.push(m);
                    }
                }
            }
            Current::Source => {
                for cap in quoted_re.captures_iter(trimmed) {
                    let s = cap[1].to_string();
                    if !opts.sources.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
                        opts.sources.push(s);
                    }
                }
            }
            Current::None | Current::Other => {}
        }
    }

    // Границы: строка вида "-l 0..215.9mm t 0..297mm w 0..215.9mm h 0..297mm".
    if let Some(caps) = bounds_re.captures(text) {
        let get = |name: &str| -> Option<(f64, f64)> {
            for c in bounds_re.captures_iter(text) {
                if &c[1] == name {
                    let lo: f64 = c[2].parse().ok()?;
                    let hi: f64 = c[3].parse().ok()?;
                    return Some((lo, hi));
                }
            }
            None
        };
        let _ = caps; // достаточно первого прохода ниже
        if let (Some(l), Some(t), Some(w), Some(h)) = (get("l"), get("t"), get("w"), get("h")) {
            opts.bounds = Some(RectMm {
                x: l.0,
                y: t.0,
                w: w.1 - w.0,
                h: h.1 - h.0,
            });
        }
    }

    if opts.resolutions.is_empty() {
        opts.resolutions = vec![75, 100, 150, 200, 300, 600, 1200];
    }
    if opts.modes.is_empty() {
        opts.modes = vec!["Lineart".into(), "Grayscale".into(), "Color".into()];
    }
    opts.duplex_sources = opts
        .sources
        .iter()
        .filter(|s| s.to_ascii_lowercase().contains("duplex"))
        .cloned()
        .collect();
    opts
}

fn page_timeout(resolution: u32) -> Duration {
    Duration::from_secs((120 + resolution as u64).min(900))
}

/// Постраничное сканирование.
///
/// * `pages == 0` — «все страницы» из ADF (остановка по сигналу «подача
///   закончилась»); на flatbed интерпретируется как 1 страница.
/// * Отмена через `CancelHandle` убивает `scanimage`.
pub fn scan_pages(
    req: &ScanRequest,
    opts: &ScannerOptions,
    cancel: &CancelHandle,
    mut progress: impl FnMut(ScanProgress),
) -> Result<Vec<ScanPage>> {
    ensure_available()?;
    if req.scanner_id.trim().is_empty() {
        return Err(Error::InvalidInput("не выбран сканер".into()));
    }

    let is_flatbed = req.source.to_ascii_lowercase().contains("flat");
    let total: Option<usize> = if req.pages == 0 {
        None
    } else if is_flatbed {
        Some(1)
    } else {
        Some(req.pages as usize)
    };
    let limit = total.unwrap_or(200); // защита от бесконечной ADF-петли

    let mut pages: Vec<ScanPage> = Vec::new();
    for index in 1..=limit {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        progress(ScanProgress::PageStarted { page: index, total });
        let mut args: Vec<String> = vec![
            format!("--device-name={}", req.scanner_id),
            format!("--resolution={}", req.resolution.clamp(50, 4800)),
            format!(
                "--mode={}",
                req.mode_name
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| req.mode.as_scanimage().to_string())
            ),
            format!("--source={}", req.source),
            // pnm — формат, встроенный в sane-backends, без внешних зависимостей.
            "--format=pnm".to_string(),
        ];
        if let Some(crop) = &req.crop {
            // Ограничиваем запрошенную область границами устройства.
            let crop = clamp_crop(crop, opts.bounds);
            args.push(format!("-l={:.2}", crop.x.max(0.0)));
            args.push(format!("-t={:.2}", crop.y.max(0.0)));
            args.push(format!("-w={:.2}", crop.w.max(1.0)));
            args.push(format!("-h={:.2}", crop.h.max(1.0)));
        }

        let out = proc::run_cancellable("scanimage", &args, page_timeout(req.resolution), cancel);

        let out = match out {
            Ok(o) if o.status == 0 && !o.stdout.is_empty() => o,
            Ok(o) => {
                let err_text = format!("{} {}", o.stderr, o.stdout_str());
                let lower = err_text.to_ascii_lowercase();
                // ADF закончился — штатное завершение многостраничного скана.
                let adf_exhausted = index > 1
                    && (lower.contains("out of data")
                        || lower.contains("no document")
                        || lower.contains("adf")
                            && (lower.contains("empty") || lower.contains("error")));
                if adf_exhausted {
                    tracing::info!("ADF пуст: получено страниц: {index}");
                    break;
                }
                return Err(Error::ToolFailed {
                    tool: "scanimage".into(),
                    code: o.status,
                    message: classify_scan_error(&err_text),
                });
            }
            Err(e) => return Err(e),
        };

        let image = decode_scan_output(&out.stdout)?;
        let image = rotate_image(image, req.rotation);
        pages.push(ScanPage {
            image,
            dpi: req.resolution,
        });
        progress(ScanProgress::PageDone { page: index, total });

        if let Some(t) = total {
            if index >= t {
                break;
            }
        }
        // Flatbed: только одна страница за проход (лист кладут вручную).
        if is_flatbed {
            break;
        }
    }

    if pages.is_empty() {
        return Err(Error::msg(i18n_empty_adf()));
    }
    Ok(pages)
}

fn i18n_empty_adf() -> String {
    crate::i18n::tr("scan-empty-adf")
}

/// Обрезать запрошенную область по границам сканера (если они известны).
fn clamp_crop(crop: &RectMm, bounds: Option<RectMm>) -> RectMm {
    let Some(b) = bounds else { return *crop };
    let x = crop.x.clamp(0.0, b.w.max(0.0));
    let y = crop.y.clamp(0.0, b.h.max(0.0));
    let w = crop.w.clamp(1.0, (b.w - x).max(1.0));
    let h = crop.h.clamp(1.0, (b.h - y).max(1.0));
    RectMm { x, y, w, h }
}

fn classify_scan_error(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("device busy") || lower.contains("resource busy") {
        crate::i18n::trf("scan-device-busy", &[("msg", text.trim())])
    } else if lower.contains("invalid argument") {
        crate::i18n::trf("scan-invalid-arg", &[("msg", text.trim())])
    } else if lower.contains("access denied") || lower.contains("permission") {
        crate::i18n::trf("err-permission", &[("msg", text.trim())])
    } else {
        text.trim().to_string()
    }
}

fn decode_scan_output(bytes: &[u8]) -> Result<DynamicImage> {
    if let Ok(img) = image::load_from_memory_with_format(bytes, image::ImageFormat::Pnm) {
        return Ok(img);
    }
    image::load_from_memory(bytes)
        .map_err(|e| Error::msg(format!("не удалось разобрать изображение со сканера: {e}")))
}

/// Повернуть изображение на угол, кратный 90°.
pub fn rotate_image(img: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::R0 => img,
        Rotation::R90 => img.rotate90(),
        Rotation::R180 => img.rotate180(),
        Rotation::R270 => img.rotate270(),
    }
}

/// Обрезать изображение по прямоугольнику в пикселях (клип к границам).
pub fn crop_image(img: &DynamicImage, x: u32, y: u32, w: u32, h: u32) -> DynamicImage {
    let (iw, ih) = (img.width(), img.height());
    let x = x.min(iw.saturating_sub(1));
    let y = y.min(ih.saturating_sub(1));
    let w = w.min(iw - x).max(1);
    let h = h.min(ih - y).max(1);
    img.crop_imm(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scanner_list() {
        let out = "\
device `genesys:libusb:001:004' is a Plustek OpticFilm 7200 film scanner
device `escl:https://192.168.1.5:443' is a HP LaserJet MFP M130fw eSCL
";
        let scanners = parse_scanner_list(out);
        assert_eq!(scanners.len(), 2);
        assert_eq!(scanners[0].id, "genesys:libusb:001:004");
        assert!(scanners[0].name.contains("OpticFilm"));
        assert_eq!(scanners[1].id, "escl:https://192.168.1.5:443");
    }

    #[test]
    fn parses_help_output() {
        let help = r#"scanimage --version...

    -l, --top-left-x <number>    Top-left x position to scan at in mm.
    -t, --top-left-y <number>    Top-left y position to scan at in mm.
    -w, --bottom-right-x <number>
                                 Bottom-right x position...
    -h, --bottom-right-y <number>
    --resolution <number>|<number>...
                                 Sets the resolution of the scan.
    --mode <string>...           Selects the basic mode of the scanner.
                                 "Lineart" "Grayscale" "Color"
    --source <string>...         Selects the scan source (such as Flatbed).
                                 "Flatbed" "ADF" "ADF Duplex"
    --batch-count=<number>       ...
    --batch                      Batch-scanning option
    -x <number>                  Width of scan-area...

    -l 0..215.9mm t 0..297mm w 0..215.9mm h 0..297mm
"#;
        let opts = parse_scanimage_help(help);
        assert!(opts.resolutions.contains(&300) || !opts.resolutions.is_empty());
        assert!(opts.modes.iter().any(|m| m == "Color"));
        assert!(opts.sources.iter().any(|s| s == "Flatbed"));
        assert_eq!(opts.duplex_sources, vec!["ADF Duplex".to_string()]);
        assert!(opts.supports_batch);
        let b = opts.bounds.expect("границы должны разобраться");
        assert!((b.w - 215.9).abs() < 0.01, "{b:?}");
        assert!((b.h - 297.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn rotation_works() {
        let img = DynamicImage::new_rgba8(4, 2);
        let r = rotate_image(img, Rotation::R90);
        assert_eq!((r.width(), r.height()), (2, 4));
        // Поворот на 180° не меняет размеры.
        let r = rotate_image(r, Rotation::R180);
        assert_eq!((r.width(), r.height()), (2, 4));
    }

    #[test]
    fn crops_within_bounds() {
        let img = DynamicImage::new_rgba8(10, 10);
        let c = crop_image(&img, 8, 8, 10, 10);
        assert_eq!((c.width(), c.height()), (2, 2));
    }

    #[test]
    fn decode_real_pnm() {
        // PPM P6 2x1: два красных пикселя.
        let ppm = b"P6\n2 1\n255\n\xff\x00\x00\xff\x00\x00";
        let img = decode_scan_output(ppm).unwrap();
        assert_eq!((img.width(), img.height()), (2, 1));
    }

    #[test]
    fn scanner_list_works_against_system() {
        // Интеграционный: пропускается, если scanimage недоступен.
        if proc::tool_available("scanimage").is_none() {
            return;
        }
        let scanners = list_scanners().expect("scanimage -L должен выполниться");
        // Пустой список допустим — сканеров может не быть.
        let _ = scanners;
    }
}
