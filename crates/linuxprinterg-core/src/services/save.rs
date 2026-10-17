//! Сохранение изображений в выбранные форматы.

use std::path::Path;

use image::DynamicImage;

use crate::domain::OutputFormat;
use crate::error::{Error, Result};

/// Сохранить изображение в файл нужного формата (с перезаписью).
pub fn save_image(img: &DynamicImage, path: &Path, format: OutputFormat) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(format!("создание каталога {}", parent.display()), e))?;
    }
    let result = match format {
        OutputFormat::Png => img.save_with_format(path, image::ImageFormat::Png),
        OutputFormat::Jpeg => {
            // JPEG не поддерживает альфа-канал: конвертируем в RGB.
            match img {
                DynamicImage::ImageRgb8(_) => img.save_with_format(path, image::ImageFormat::Jpeg),
                other => other
                    .to_rgb8()
                    .save_with_format(path, image::ImageFormat::Jpeg),
            }
        }
        OutputFormat::Tiff => img.save_with_format(path, image::ImageFormat::Tiff),
        OutputFormat::Webp => img.save_with_format(path, image::ImageFormat::WebP),
        OutputFormat::Pdf => {
            return Err(Error::msg(
                "PDF сохраняется через pdf::PdfWriter, а не save_image",
            ));
        }
    };
    result.map_err(|e| Error::io(format!("сохранение {}", path.display()), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::OutputFormat;

    #[test]
    fn saves_png_jpeg_tiff_webp() {
        let dir = tempfile::tempdir().unwrap();
        let img = DynamicImage::new_rgb8(8, 8);
        for (fmt, name) in [
            (OutputFormat::Png, "a.png"),
            (OutputFormat::Jpeg, "a.jpg"),
            (OutputFormat::Tiff, "a.tiff"),
            (OutputFormat::Webp, "a.webp"),
        ] {
            let path = dir.path().join(name);
            save_image(&img, &path, fmt).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(path.metadata().unwrap().len() > 0, "{name} пуст");
        }
    }

    #[test]
    fn pdf_is_rejected_here() {
        let dir = tempfile::tempdir().unwrap();
        let img = DynamicImage::new_rgb8(4, 4);
        assert!(save_image(&img, &dir.path().join("x.pdf"), OutputFormat::Pdf).is_err());
    }
}
