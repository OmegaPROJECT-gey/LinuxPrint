//! Предпросмотр PDF: первая страница → PNG через `pdftoppm` (poppler-utils).
//!
//! Изображения (PNG/JPEG/WebP/TIFF) показываются GTK напрямую, без рендера.
//! Для PDF вызывается `pdftoppm` — он есть в poppler-utils на всех целевых
//! дистрибутивах; если утилиты нет, показывается понятная подсказка.
//! (Альтернатива `poppler-rs` отклонена: требует системного poppler-glib и
//! тянет GObject-цикл в core, который иначе полностью чистый.)

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::services::proc;

const TIMEOUT: Duration = Duration::from_secs(60);

pub fn available() -> bool {
    proc::tool_available("pdftoppm").is_some()
}

/// Отрендерить первую страницу PDF в PNG. `out_dir` — рабочий каталог кэша.
pub fn pdf_first_page(pdf: &Path, out_dir: &Path, dpi: u32) -> Result<PathBuf> {
    if !available() {
        return Err(Error::ToolMissing {
            tool: "pdftoppm".to_string(),
        });
    }
    if !pdf.is_file() {
        return Err(Error::InvalidInput(format!(
            "файл не найден: {}",
            pdf.display()
        )));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| Error::io(format!("создание {}", out_dir.display()), e))?;
    let prefix = out_dir.join("page");
    let dpi = dpi.clamp(36, 300).to_string();
    let pdf_arg = pdf.to_string_lossy().into_owned();
    let prefix_arg = prefix.to_string_lossy().into_owned();
    proc::run(
        "pdftoppm",
        &[
            "-png",
            "-f",
            "1",
            "-l",
            "1",
            "-r",
            dpi.as_str(),
            pdf_arg.as_str(),
            prefix_arg.as_str(),
        ],
        TIMEOUT,
    )?
    .ensure_ok("pdftoppm")?;

    // pdftoppm создаёт <prefix>-1.png (или -01.png при больших номерах).
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(out_dir)
        .map_err(|e| Error::io("чтение каталога кэша", e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("page") && n.ends_with(".png"))
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| Error::msg("pdftoppm не создал изображение — возможно, повреждённый PDF"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_rejected() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(pdf_first_page(Path::new("/nonexistent.pdf"), dir.path(), 96).is_err());
    }

    #[test]
    fn renders_generated_pdf() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("test.pdf");
        let img = image::DynamicImage::new_rgb8(100, 140);
        crate::services::pdf::PdfWriter::new(96)
            .with_image(&img)
            .save(&pdf_path)
            .unwrap();
        let png = pdf_first_page(&pdf_path, &dir.path().join("out"), 72).unwrap();
        assert!(png.is_file());
        let rendered = image::open(&png).unwrap();
        assert!(rendered.width() > 0 && rendered.height() > 0);
    }
}
