//! OCR через Tesseract (опциональная функция).

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::services::proc;

const OCR_TIMEOUT: Duration = Duration::from_secs(300);

/// Доступен ли tesseract.
pub fn available() -> bool {
    proc::tool_available("tesseract").is_some()
}

/// Языки, установленные в системе (`tesseract --list-langs`).
/// Возвращает список кодов вида `rus`, `eng`; при ошибке — пусто.
pub fn languages() -> Vec<String> {
    let Ok(out) = proc::run("tesseract", &["--list-langs"], Duration::from_secs(15)) else {
        return Vec::new();
    };
    out.stdout_str()
        .lines()
        .skip(1) // "List of available languages in ... (N):"
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of"))
        .map(str::to_string)
        .collect()
}

/// Распознать текст на изображении. `lang` — например `rus+eng`.
pub fn ocr_file(image: &Path, lang: &str) -> Result<String> {
    if !available() {
        return Err(Error::ToolMissing {
            tool: "tesseract".to_string(),
        });
    }
    if !image.is_file() {
        return Err(Error::InvalidInput(format!(
            "файл не найден: {}",
            image.display()
        )));
    }
    let lang = sanitize_lang(lang);
    let out = proc::run(
        "tesseract",
        &[
            image.to_string_lossy().as_ref(),
            "stdout",
            "-l",
            &lang,
            "--psm",
            "1",
        ],
        OCR_TIMEOUT,
    )?;
    if out.status != 0 {
        return Err(Error::ToolFailed {
            tool: "tesseract".into(),
            code: out.status,
            message: out.stderr.trim().to_string(),
        });
    }
    Ok(out.stdout_str())
}

/// Язык — только `[a-zA-Z0-9_+ -]` (защита от подстановки опций).
fn sanitize_lang(lang: &str) -> String {
    let cleaned: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-' | ' '))
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "eng".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_language() {
        assert_eq!(sanitize_lang("rus+eng"), "rus+eng");
        // Символы вроде ';' '/' '.' вырезаются полностью.
        assert_eq!(sanitize_lang("rus; rm -rf /"), "rus rm -rf");
        assert_eq!(sanitize_lang("--help"), "--help");
        assert_eq!(sanitize_lang(""), "eng");
        assert_eq!(sanitize_lang("../../../etc"), "etc");
    }

    #[test]
    fn languages_query_is_safe_without_tesseract() {
        // Без tesseract возвращается пустой список, а не паника.
        let langs = languages();
        if !available() {
            assert!(langs.is_empty());
        }
    }
}
