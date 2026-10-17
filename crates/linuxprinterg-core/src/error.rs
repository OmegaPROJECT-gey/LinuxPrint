//! Тип ошибки приложения и его локализованное представление.

use crate::i18n;

/// Единый тип ошибки core-слоя.
///
/// `Display` намеренно английский (стабильные логи, CI), а для показа в UI
/// используйте [`Error::ui_message`] и [`Error::help_text`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("external tool '{tool}' was not found in PATH")]
    ToolMissing { tool: String },

    #[error("command '{tool}' failed with code {code}: {message}")]
    ToolFailed {
        tool: String,
        code: i32,
        message: String,
    },

    #[error("timeout ({secs}s) waiting for '{tool}'")]
    Timeout { tool: String, secs: u64 },

    #[error("operation cancelled by the user")]
    Cancelled,

    #[error("{context}: {text}")]
    Io { context: String, text: String },

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("failed to parse output of '{tool}': {details}")]
    Parse { tool: String, details: String },

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Error::Cancelled)
    }

    pub fn io(context: impl Into<String>, source: impl std::fmt::Display) -> Self {
        Error::Io {
            context: context.into(),
            text: source.to_string(),
        }
    }

    pub fn msg(text: impl Into<String>) -> Self {
        Error::Msg(text.into())
    }

    /// Короткое локализованное сообщение для тостов/диалогов.
    pub fn ui_message(&self) -> String {
        match self {
            Error::ToolMissing { tool } => i18n::trf("err-tool-missing", &[("tool", tool)]),
            Error::ToolFailed {
                tool,
                code,
                message,
            } => i18n::trf(
                "err-tool-failed",
                &[
                    ("tool", tool),
                    ("code", &code.to_string()),
                    ("message", message),
                ],
            ),
            Error::Timeout { tool, secs } => i18n::trf(
                "err-timeout",
                &[("tool", tool), ("secs", &secs.to_string())],
            ),
            Error::Cancelled => i18n::tr("err-cancelled"),
            Error::Io { context, text } => {
                i18n::trf("err-io", &[("context", context), ("source", text)])
            }
            Error::InvalidInput(s) => i18n::trf("err-invalid-input", &[("msg", s)]),
            Error::Parse { tool, details } => {
                i18n::trf("err-parse", &[("tool", tool), ("details", details)])
            }
            Error::Permission(s) => i18n::trf("err-permission", &[("msg", s)]),
            Error::Msg(s) => s.clone(),
        }
    }

    /// Подробная инструкция «что делать» (может отсутствовать).
    pub fn help_text(&self) -> Option<String> {
        match self {
            Error::ToolMissing { tool } => match tool.as_str() {
                "lpstat" | "lp" | "cancel" | "lpoptions" | "lpadmin" => {
                    Some(i18n::tr("help-install-cups"))
                }
                "scanimage" => Some(i18n::tr("help-install-sane")),
                "tesseract" => Some(i18n::tr("help-install-tesseract")),
                "pdftoppm" => Some(i18n::tr("help-install-poppler")),
                "soffice" | "libreoffice" => Some(i18n::tr("help-install-libreoffice")),
                _ => None,
            },
            Error::Permission(_) => Some(i18n::tr("help-permission")),
            _ => None,
        }
    }
}
