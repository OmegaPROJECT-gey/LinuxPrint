//! Опциональная конвертация офисных документов в PDF через LibreOffice headless.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::services::proc;

const CONVERT_TIMEOUT: Duration = Duration::from_secs(300);

/// Найти бинарник LibreOffice (`soffice`/`libreoffice`).
pub fn libreoffice_binary() -> Option<PathBuf> {
    proc::tool_available("soffice").or_else(|| proc::tool_available("libreoffice"))
}

pub fn available() -> bool {
    libreoffice_binary().is_some()
}

/// Конвертировать документ в PDF в каталог `out_dir`.
/// Возвращает путь к полученному PDF.
pub fn to_pdf(src: &Path, out_dir: &Path) -> Result<PathBuf> {
    let binary = libreoffice_binary().ok_or_else(|| Error::ToolMissing {
        tool: "soffice".to_string(),
    })?;
    if !src.is_file() {
        return Err(Error::InvalidInput(format!(
            "файл не найден: {}",
            src.display()
        )));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| Error::io(format!("создание {}", out_dir.display()), e))?;

    let outdir_arg = out_dir.to_string_lossy().into_owned();
    let src_arg = src.to_string_lossy().into_owned();
    let out = proc::run(
        binary.to_string_lossy().as_ref(),
        [
            "--headless",
            "--norestore",
            "--convert-to",
            "pdf",
            "--outdir",
            outdir_arg.as_str(),
            src_arg.as_str(),
        ]
        .as_slice(),
        CONVERT_TIMEOUT,
    )?;

    let expected = out_dir.join(format!(
        "{}.pdf",
        src.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if out.status == 0 && expected.is_file() {
        return Ok(expected);
    }
    // LibreOffice может сообщать об успехе, но не создать файл — либо упасть.
    Err(Error::ToolFailed {
        tool: "soffice".into(),
        code: out.status,
        message: if out.stderr.trim().is_empty() {
            format!("файл {} не создан", expected.display())
        } else {
            out.stderr.trim().to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_source_is_rejected() {
        if !available() {
            // LibreOffice не установлен — ожидаем ToolMissing.
            let dir = tempfile::tempdir().unwrap();
            let err = to_pdf(Path::new("/nonexistent.docx"), dir.path()).unwrap_err();
            assert!(matches!(err, Error::ToolMissing { .. }));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(to_pdf(Path::new("/nonexistent.docx"), dir.path()).is_err());
    }
}
