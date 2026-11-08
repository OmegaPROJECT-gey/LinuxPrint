//! Автоподгон фото под лист (аналог Canon PRINT): подбор формата и
//! ориентации по соотношению сторон снимка, режим заполнения и расчёт
//! обрезки.
//!
//! Чистая логика без IO и GTK — ядро тестируется без графического
//! окружения. На вход — реальные choices опции `PageSize` принтера
//! (`lpoptions -p … -l`), включая форматы `.Borderless`.

/// Размеры листа в мм и признак безполейного (`.Borderless`) формата.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaDims {
    pub w_mm: f64,
    pub h_mm: f64,
    pub borderless: bool,
}

/// Итог автоподгона: формат, ориентация, режим подгонки и обрезка.
#[derive(Debug, Clone, PartialEq)]
pub struct FitPlan {
    /// Значение `PageSize` (`4x6.Borderless`, `A4` …).
    pub media: String,
    /// Печать в альбомной ориентации: снимок шире листа.
    pub landscape: bool,
    /// Режим `print-scaling`: `fill` (заполнение с обрезкой) или `fit`
    /// (вписывание — когда принтер не заявляет `fill`).
    pub scaling: &'static str,
    /// Суммарная обрезка по ширине листа, мм (0 для `fit`).
    pub crop_w_mm: f64,
    /// Суммарная обрезка по высоте листа, мм (0 для `fit`).
    pub crop_h_mm: f64,
    /// Доля ширины снимка, отрезаемая по центру (0…1) — для оверлея
    /// предпросмотра: тёмные полосы закрывают именно эти края.
    pub crop_frac_x: f64,
    /// Доля высоты снимка, отрезаемая по центру (0…1).
    pub crop_frac_y: f64,
}

/// Имя файла без признака `.Borderless` — именованные форматы.
fn named_size(name: &str) -> Option<(f64, f64)> {
    // Значения в мм; фотопрофильные размеры в дюймах разбирает общий
    // парсер ниже (`4x6`, `5x7`, `3.5x5` …).
    Some(match name {
        "A3" => (297.0, 420.0),
        "A4" => (210.0, 297.0),
        "A5" => (148.0, 210.0),
        "A6" => (105.0, 148.0),
        "B4" => (250.0, 353.0),
        "B5" => (176.0, 250.0),
        "Letter" => (215.9, 279.4),
        "Legal" => (215.9, 355.6),
        "Executive" => (184.2, 266.7),
        "Oficio" => (216.0, 340.0),
        "Postcard" => (100.0, 148.0),
        // Форматы в «мм-имени»: 10×15 см/100×150 мм.
        "10x15" | "100x150" => (100.0, 150.0),
        _ => return None,
    })
}

/// Размеры листа по значению `PageSize`. `None` для нераспознанных
/// значений: `Custom.*`, конвертов, фальцованной бумаги и мусора.
pub fn media_dims(value: &str) -> Option<MediaDims> {
    let raw = value.trim();
    if raw.is_empty() || raw.starts_with("Custom.") {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    // Конверты и фальцованная бумага фото не бывают.
    if lower.starts_with("env") || lower.contains("fanfold") {
        return None;
    }
    let borderless = lower.ends_with(".borderless");
    let name = if borderless {
        &raw[..raw.len() - ".borderless".len()]
    } else {
        raw
    };
    if let Some((w_mm, h_mm)) = named_size(name) {
        return Some(MediaDims {
            w_mm,
            h_mm,
            borderless,
        });
    }
    let low = name.to_ascii_lowercase();
    // Явные миллиметры: «215x345mm», «100x150mm».
    if let Some(rest) = low.strip_suffix("mm") {
        let (w, h) = rest.split_once('x')?;
        let w_mm: f64 = w.trim().parse().ok()?;
        let h_mm: f64 = h.trim().parse().ok()?;
        if w_mm > 0.0 && h_mm > 0.0 {
            return Some(MediaDims {
                w_mm,
                h_mm,
                borderless,
            });
        }
        return None;
    }
    // Фотоформаты в дюймах: «4x6», «3.5x5», «8x10».
    let (w, h) = low.split_once('x')?;
    let w_in: f64 = w.trim().parse().ok()?;
    let h_in: f64 = h.trim().parse().ok()?;
    // Ограничиваем разумным диапазоном: листы больше 100″ не бывают,
    // а «A4x2» и подобные мусорные имена так отсекаются.
    if w_in > 0.0 && h_in > 0.0 && w_in < 100.0 && h_in < 100.0 {
        // Округляем до 0.001 мм: 6 × 25.4 = 152.39999999999998, а не 152.4.
        let round = |v: f64| (v * 25.4 * 1000.0).round() / 1000.0;
        return Some(MediaDims {
            w_mm: round(w_in),
            h_mm: round(h_in),
            borderless,
        });
    }
    None
}

/// Выбранный формат с оценкой.
struct Pick {
    media: String,
    landscape: bool,
    dims: MediaDims,
    score: f64,
}

/// Лучший формат: минимальное относительное отклонение соотношения
/// сторон (в логарифмической шкале — симметрично для «крупнее/мелче»);
/// при равном отклонении побеждает больший лист, а `prefer_borderless`
/// решает спор одинаковых форматов в пользу `.Borderless`.
fn best_pick(img_w: u32, img_h: u32, choices: &[String], prefer_borderless: bool) -> Option<Pick> {
    if img_w == 0 || img_h == 0 {
        return None;
    }
    let ratio = img_w as f64 / img_h as f64;
    let mut best: Option<Pick> = None;
    for value in choices {
        let Some(dims) = media_dims(value) else {
            continue;
        };
        let portrait = dims.w_mm / dims.h_mm;
        // Лист можно печатать в альбомной ориентации — сравниваем и её.
        let score_p = (portrait / ratio).ln().abs();
        let score_l = (1.0 / portrait / ratio).ln().abs();
        let (landscape, mut score) = if score_l < score_p {
            (true, score_l)
        } else {
            (false, score_p)
        };
        if prefer_borderless && dims.borderless {
            score -= 0.005;
        }
        let area = dims.w_mm * dims.h_mm;
        let better = match &best {
            None => true,
            Some(b) => {
                score < b.score - 1e-9
                    || ((score - b.score).abs() <= 1e-9 && area > b.dims.w_mm * b.dims.h_mm)
            }
        };
        if better {
            best = Some(Pick {
                media: value.clone(),
                landscape,
                dims,
                score,
            });
        }
    }
    best
}

/// Рассчитать автоподгон снимка под список форматов принтера.
///
/// * `fill_supported` — принтер заявляет `print-scaling` со значением
///   `fill`: лист заполняется снимком с обрезкой краёв; иначе —
///   безопасное вписывание (`fit`) без обрезки;
/// * `prefer_borderless` — при равном качестве отдавать предпочтение
///   формату `.Borderless` (печать «от края до края»).
pub fn plan(
    img_w: u32,
    img_h: u32,
    choices: &[String],
    fill_supported: bool,
    prefer_borderless: bool,
) -> Option<FitPlan> {
    let pick = best_pick(img_w, img_h, choices, prefer_borderless)?;
    // Эффективный лист с учётом ориентации печати.
    let (ew, eh) = if pick.landscape {
        (pick.dims.h_mm, pick.dims.w_mm)
    } else {
        (pick.dims.w_mm, pick.dims.h_mm)
    };
    let (scaling, crop_w_mm, crop_h_mm, crop_frac_x, crop_frac_y) = if fill_supported {
        // Коэффициент «покрытия» листа: берём больший из двух, чтобы
        // не осталось полей, — лишнее обрезается по центру.
        let k = f64::max(ew / img_w as f64, eh / img_h as f64);
        let printed_w = img_w as f64 * k;
        let printed_h = img_h as f64 * k;
        let cw = (printed_w - ew).max(0.0);
        let ch = (printed_h - eh).max(0.0);
        ("fill", cw, ch, cw / printed_w, ch / printed_h)
    } else {
        ("fit", 0.0, 0.0, 0.0, 0.0)
    };
    Some(FitPlan {
        media: pick.media,
        landscape: pick.landscape,
        scaling,
        crop_w_mm,
        crop_h_mm,
        crop_frac_x,
        crop_frac_y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices() -> Vec<String> {
        [
            "A4",
            "A4.Borderless",
            "4x6",
            "4x6.Borderless",
            "Letter",
            "Postcard",
            "EnvDL",
            "Custom.100x150mm",
            "215x345mm",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn dims_named_and_borderless() {
        assert_eq!(
            media_dims("A4"),
            Some(MediaDims {
                w_mm: 210.0,
                h_mm: 297.0,
                borderless: false
            })
        );
        assert_eq!(
            media_dims("4x6.Borderless"),
            Some(MediaDims {
                w_mm: 101.6,
                h_mm: 152.4,
                borderless: true
            })
        );
        assert_eq!(
            media_dims("Postcard"),
            Some(MediaDims {
                w_mm: 100.0,
                h_mm: 148.0,
                borderless: false
            })
        );
    }

    #[test]
    fn dims_explicit_mm_and_inches() {
        let mm = media_dims("215x345mm").expect("mm-формат");
        assert_eq!((mm.w_mm, mm.h_mm), (215.0, 345.0));
        let inch = media_dims("5x7").expect("дюймовый формат");
        assert!((inch.w_mm - 127.0).abs() < 1e-9);
        assert!((inch.h_mm - 177.8).abs() < 1e-9);
    }

    #[test]
    fn dims_skips_custom_envelopes_and_garbage() {
        assert_eq!(media_dims("Custom.100x150mm"), None);
        assert_eq!(media_dims("EnvDL"), None);
        assert_eq!(media_dims(""), None);
        assert_eq!(media_dims("Stationery"), None);
    }

    #[test]
    fn picks_borderless_4x6_for_3x2_photo() {
        // Снимок 3:2 — точное совпадение с 4×6; при равном счёте
        // побеждает .Borderless (prefer_borderless).
        let plan = plan(3000, 2000, &choices(), true, true).expect("план");
        assert_eq!(plan.media, "4x6.Borderless");
        assert!(plan.landscape, "3:2 шире вертикального 4×6 → альбомная");
        assert_eq!(plan.scaling, "fill");
        assert!((plan.crop_w_mm - 0.0).abs() < 1e-6);
        assert!((plan.crop_h_mm - 0.0).abs() < 1e-6);
    }

    #[test]
    fn fills_and_crops_4x3_photo_on_4x6() {
        // 4:3 (4032×3024) на 4×6 в альбомной: лист 152.4×101.6 мм,
        // масштаб по ширине, высота переполняется → обрезка ~12.7 мм.
        // Формат только один — проверяем именно математику обрезки.
        let only = vec!["4x6.Borderless".to_string()];
        let plan = plan(4032, 3024, &only, true, true).expect("план");
        assert_eq!(plan.media, "4x6.Borderless");
        assert!(plan.landscape);
        assert_eq!(plan.scaling, "fill");
        assert!(plan.crop_w_mm.abs() < 1e-6);
        assert!((plan.crop_h_mm - 12.7).abs() < 0.01, "{}", plan.crop_h_mm);
    }

    #[test]
    fn falls_back_to_fit_without_fill_support() {
        let plan = plan(3000, 2000, &choices(), false, true).expect("план");
        assert_eq!(plan.scaling, "fit");
        assert_eq!(plan.crop_w_mm, 0.0);
        assert_eq!(plan.crop_h_mm, 0.0);
    }

    #[test]
    fn portrait_photo_stays_portrait_on_a4() {
        // Вертикальный снимок 2:3 (2000×3000) → A4 (0.703) против
        // 4×6 (0.667): ближе A4? 2:3=0.667 → 4×6 точнее, но в
        // вертикальной ориентации; проверяем выбор по соотношению.
        let plan = plan(2000, 3000, &["A4".to_string()], true, false).expect("план");
        assert_eq!(plan.media, "A4");
        assert!(!plan.landscape);
        assert_eq!(plan.scaling, "fill");
    }

    #[test]
    fn skips_unparseable_choices() {
        // Мусорные имена не должны ронять подбор.
        let bad = vec!["Stationery".to_string(), "A4".to_string()];
        let plan = plan(2000, 3000, &bad, true, true).expect("план");
        assert_eq!(plan.media, "A4");
    }

    #[test]
    fn zero_dimensions_give_no_plan() {
        assert!(plan(0, 100, &choices(), true, true).is_none());
        assert!(plan(100, 0, &choices(), true, true).is_none());
    }
}
