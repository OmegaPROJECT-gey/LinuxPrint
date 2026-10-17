//! Журнал операций: история печати, сканирования, системных событий.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage;

/// Уровень записи журнала.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// Категория события.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Print,
    Scan,
    System,
    Settings,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Print => "print",
            Category::Scan => "scan",
            Category::System => "system",
            Category::Settings => "settings",
        }
    }
}

/// Одна запись журнала.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// RFC3339, UTC.
    pub ts: String,
    pub level: Level,
    pub category: Category,
    pub message: String,
    /// Путь к сохранённому файлу (если есть).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Entry {
    /// Человекочитаемая метка времени: `2026-10-17 12:00:00`.
    pub fn ts_display(&self) -> String {
        self.ts
            .replace('T', " ")
            .chars()
            .take(19)
            .collect::<String>()
            .trim()
            .to_string()
    }
}

struct JournalInner {
    path: PathBuf,
    max: usize,
    entries: Vec<Entry>,
}

/// Потокобезопасный дескриптор журнала: клонируется, разделяется между UI и
/// рабочими потоками.
#[derive(Clone)]
pub struct JournalHandle {
    inner: Arc<Mutex<JournalInner>>,
    seq: Arc<AtomicU64>,
}

impl JournalHandle {
    /// Открыть журнал: загрузить из файла (если есть) и готовить к записи.
    pub fn open(path: PathBuf, max: usize) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Entry>>(&text).ok())
            .unwrap_or_default();
        let mut entries = entries;
        if entries.len() > max {
            let cut = entries.len() - max;
            entries.drain(0..cut);
        }
        JournalHandle {
            inner: Arc::new(Mutex::new(JournalInner { path, max, entries })),
            seq: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Журнал в каталоге состояния приложения.
    pub fn open_default() -> Self {
        Self::open(storage::journal_path(), 1000)
    }

    pub fn log(
        &self,
        level: Level,
        category: Category,
        message: impl Into<String>,
        path: Option<&Path>,
    ) {
        let entry = Entry {
            ts: chrono::Utc::now().to_rfc3339(),
            level,
            category,
            message: message.into(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
        };
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries.push(entry);
            if inner.entries.len() > inner.max {
                let cut = inner.entries.len() - inner.max;
                inner.entries.drain(0..cut);
            }
            let snapshot: Vec<Entry> = inner.entries.clone();
            let file = inner.path.clone();
            drop(inner);
            // Запись файла — за пределами блокировки.
            if let Ok(data) = serde_json::to_string_pretty(&snapshot) {
                if let Err(e) = storage::atomic_write(&file, data.as_bytes()) {
                    tracing::warn!("не удалось записать журнал {}: {e}", file.display());
                }
            }
        }
        self.seq.fetch_add(1, Ordering::SeqCst);
    }

    pub fn info(&self, category: Category, message: impl Into<String>) {
        self.log(Level::Info, category, message, None);
    }

    pub fn warn(&self, category: Category, message: impl Into<String>) {
        self.log(Level::Warn, category, message, None);
    }

    pub fn error(&self, category: Category, message: impl Into<String>) {
        self.log(Level::Error, category, message, None);
    }

    /// Копия записей (новые в конце).
    pub fn entries(&self) -> Vec<Entry> {
        self.inner
            .lock()
            .map(|i| i.entries.clone())
            .unwrap_or_default()
    }

    /// Число изменений — для оптимистичного обновления UI.
    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// Очистить журнал.
    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries.clear();
            let _ = storage::atomic_write(&inner.path, b"[]");
        }
        self.seq.fetch_add(1, Ordering::SeqCst);
    }

    /// Экспорт журнала в текстовый файл.
    pub fn export_to(&self, path: &Path) -> Result<()> {
        let entries = self.entries();
        let mut text = String::new();
        text.push_str("# LinuxPrinterG journal\n");
        for e in entries {
            text.push_str(&format!(
                "[{}] {:<5} {:<8} {}",
                e.ts_display(),
                e.level.as_str(),
                e.category.as_str(),
                e.message
            ));
            if let Some(p) = &e.path {
                text.push_str(&format!(" -> {p}"));
            }
            text.push('\n');
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("создание {}", parent.display()), e))?;
        }
        std::fs::write(path, text).map_err(|e| Error::io(format!("запись {}", path.display()), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.json");
        let j = JournalHandle::open(path.clone(), 10);
        j.info(Category::Print, "отправлено задание 42");
        j.error(Category::Scan, "сканер занят");
        assert_eq!(j.entries().len(), 2);
        assert!(j.seq() >= 2);

        // Перезагрузка из файла.
        let j2 = JournalHandle::open(path, 10);
        let entries = j2.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].category, Category::Print);
        assert_eq!(entries[1].level, Level::Error);
        assert!(entries[0].ts_display().contains(' '));
    }

    #[test]
    fn cap_limits_entries() {
        let dir = tempfile::tempdir().unwrap();
        let j = JournalHandle::open(dir.path().join("j.json"), 5);
        for i in 0..20 {
            j.info(Category::System, format!("событие {i}"));
        }
        assert_eq!(j.entries().len(), 5);
        assert!(j.entries().last().unwrap().message.contains("19"));
    }

    #[test]
    fn export_writes_text() {
        let dir = tempfile::tempdir().unwrap();
        let j = JournalHandle::open(dir.path().join("j.json"), 10);
        j.info(Category::Scan, "сохранено: /tmp/x.pdf");
        let out = dir.path().join("export.txt");
        j.export_to(&out).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("LinuxPrinterG journal"));
        assert!(text.contains("scan"));
        assert!(text.contains("/tmp/x.pdf"));
    }

    #[test]
    fn clear_empties_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("j.json");
        let j = JournalHandle::open(path.clone(), 10);
        j.info(Category::System, "a");
        j.clear();
        assert!(j.entries().is_empty());
        let j2 = JournalHandle::open(path, 10);
        assert!(j2.entries().is_empty());
    }
}
