//! Шаблоны имён файлов и обеспечение уникальности.

use std::path::{Path, PathBuf};

use crate::domain::OutputFormat;

/// Контекст подстановки в шаблон имени файла.
///
/// Поддерживаемые поля: `{base}` — имя исходного файла, `{device}` —
/// имя принтера/сканера, `{date}`, `{time}`, `{page}`, `{n}`, `{ext}`.
#[derive(Debug, Clone)]
pub struct NameCtx {
    pub base: String,
    pub device: String,
    pub date: String,
    pub time: String,
    /// Номер страницы (для многопостраничных файлов).
    pub page: Option<u32>,
    /// Порядковый номер в серии сканов.
    pub counter: u32,
    pub ext: String,
}

impl NameCtx {
    /// Контекст «сейчас» для файла `base` с расширением `ext`.
    pub fn now(base: &str, device: &str, ext: &str) -> Self {
        let now = chrono::Local::now();
        NameCtx {
            base: base.to_string(),
            device: device.to_string(),
            date: now.format("%Y-%m-%d").to_string(),
            time: now.format("%H-%M-%S").to_string(),
            page: None,
            counter: 1,
            ext: ext.to_string(),
        }
    }
}

/// Отрендерить шаблон и очистить результат от опасных символов.
pub fn render(template: &str, ctx: &NameCtx) -> String {
    let mut out = template.to_string();
    let page = ctx
        .page
        .map(|p| p.to_string())
        .unwrap_or_else(|| "1".into());
    for (key, value) in [
        ("{base}", ctx.base.as_str()),
        ("{device}", ctx.device.as_str()),
        ("{date}", ctx.date.as_str()),
        ("{time}", ctx.time.as_str()),
        ("{page}", page.as_str()),
        ("{n}", &ctx.counter.to_string()),
        ("{ext}", ctx.ext.as_str()),
    ] {
        out = out.replace(key, value);
    }
    sanitize(&out)
}

/// Убрать path-разделители, управляющие символы, «специальные» имена; ограничить длину.
pub fn sanitize(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Схлопываем множественные подчёркивания и пробелы по краям.
    while cleaned.contains("__") {
        cleaned = cleaned.replace("__", "_");
    }
    cleaned = cleaned.trim_matches(|c| c == '_' || c == ' ').to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "file".to_string();
    }
    if cleaned.chars().count() > 120 {
        let truncated: String = cleaned.chars().take(120).collect();
        cleaned = truncated;
    }
    cleaned
}

/// Путь к файлу, гарантированно внутри `dir` (защита от `..` в шаблоне).
pub fn safe_join(dir: &Path, name: &str) -> PathBuf {
    let name = sanitize(name);
    let base = dir.to_path_buf();
    let candidate = base.join(&name);
    // После sanitize компонент один, проверка избыточна, но оставляем страховку.
    match candidate.strip_prefix(&base) {
        Ok(_) => candidate,
        Err(_) => base.join(sanitize(&name)),
    }
}

/// Уникальное имя: если файл есть — добавляем « (2)», « (3)» …
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let stem = sanitize(name);
    let first = safe_join(dir, &format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for i in 2..10_000u32 {
        let candidate = safe_join(dir, &format!("{stem} ({i}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Крайний случай: таймстемп вместо бесконечного перебора.
    let ctx = NameCtx::now(&stem, "", ext);
    safe_join(dir, &format!("{}-{}", ctx.date, ctx.time))
}

/// Расширение файла (в нижнем регистре) или пустая строка.
pub fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Имя файла без расширения.
pub fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string())
}

/// Формат по расширению пути (для определения, PDF это или изображение).
pub fn format_of(path: &Path) -> Option<OutputFormat> {
    OutputFormat::from_extension(&extension_of(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_template() {
        let ctx = NameCtx {
            base: "скан".into(),
            device: "HP".into(),
            date: "2026-10-17".into(),
            time: "12-00-00".into(),
            page: Some(3),
            counter: 2,
            ext: "pdf".into(),
        };
        let name = render("{date}_{time}_{base}_p{page}_{n}.{ext}", &ctx);
        assert_eq!(name, "2026-10-17_12-00-00_скан_p3_2.pdf");
    }

    #[test]
    fn sanitizes_dangerous_names() {
        assert_eq!(sanitize("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize(".."), "file");
        assert_eq!(sanitize("  "), "file");
        assert_eq!(sanitize("a/b\\c:d"), "a_b_c_d");
        let long = "ю".repeat(200);
        assert!(sanitize(&long).chars().count() <= 120);
    }

    #[test]
    fn unique_paths_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = unique_path(dir.path(), "scan", "pdf");
        std::fs::write(&p1, b"x").unwrap();
        let p2 = unique_path(dir.path(), "scan", "pdf");
        assert_ne!(p1, p2);
        assert!(p2.to_string_lossy().contains("(2)"));
    }

    #[test]
    fn extension_and_format_detection() {
        assert_eq!(extension_of(Path::new("a/B.JPG")), "jpg");
        assert_eq!(format_of(Path::new("x.webp")), Some(OutputFormat::Webp));
        assert_eq!(format_of(Path::new("x.docx")), None);
        assert_eq!(stem_of(Path::new("archive.tar.gz")), "archive.tar");
    }
}
