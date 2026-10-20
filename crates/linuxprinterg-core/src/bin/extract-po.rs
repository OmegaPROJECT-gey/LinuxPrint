//! Извлечение `.po` из компилируемого словаря и сборка `.mo` — без
//! системного gettext (дорожная карта, п.8).
//!
//! Использование:
//!
//! ```text
//! extract-po            # перегенерировать po/linuxprinterg.pot и
//!                       # создать скелеты po/{uk,de,fr}.po, если их нет
//! extract-po --build    # собрать locale/<ll>/LC_MESSAGES/linuxprinterg.mo
//!                       # из существующих po/<ll>.po (свой писатель MO)
//! ```
//!
//! Пути считаются от корня репозитория (../../ относительно манифеста
//! crate). Скелеты никогда не перезаписываются — переводы редактируются
//! вручную или через Weblate/msgmerge; POT пересоздаётся всегда.

use std::path::{Path, PathBuf};

use linuxprinterg_core::{i18n, po};

/// Языки, для которых ведутся скелеты переводов.
const LANGS: &[&str] = &["uk", "de", "fr"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Перегенерировать POT и создать отсутствующие скелеты `.po`.
fn extract(root: &Path) -> std::io::Result<()> {
    let entries: Vec<(&str, &str)> = i18n::catalog().map(|(k, en, _)| (k, en)).collect();
    let po_dir = root.join("po");
    std::fs::create_dir_all(&po_dir)?;

    let pot_path = po_dir.join("linuxprinterg.pot");
    std::fs::write(&pot_path, po::render_pot(entries.iter().copied()))?;
    println!("записан {} ({} ключей)", pot_path.display(), entries.len());

    for lang in LANGS {
        let path = po_dir.join(format!("{lang}.po"));
        if path.exists() {
            println!("оставлен существующий {}", path.display());
        } else {
            std::fs::write(&path, po::render_po(lang, entries.iter().copied()))?;
            println!("создан скелет {} (заполните msgstr)", path.display());
        }
    }
    Ok(())
}

/// Собрать `.mo` из всех существующих `po/<ll>.po`.
fn build(root: &Path) -> std::io::Result<()> {
    let po_dir = root.join("po");
    let locale_dir = root.join("locale");
    let mut built = 0;
    for lang in LANGS {
        let path = po_dir.join(format!("{lang}.po"));
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue, // скелет ещё не создан
        };
        let mut entries = po::parse_po(&text).map_err(std::io::Error::other)?;
        let filled_count = entries.values().filter(|v| !v.is_empty()).count();
        if filled_count == 0 {
            println!("{}: переводов пока нет — .mo не собирается", path.display());
            continue;
        }
        // Заголовочная запись (charset и пр.) — как пишет msgfmt; наши
        // парсеры её отбрасывают, стандартным инструментам нужна.
        entries.insert(String::new(), po::catalog_header(lang));
        let bytes = po::write_mo(&entries);
        let out = locale_dir
            .join(lang)
            .join("LC_MESSAGES")
            .join("linuxprinterg.mo");
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, &bytes)?;
        println!(
            "собран {} ({} переведённых строк, {} байт)",
            out.display(),
            filled_count,
            bytes.len()
        );
        built += 1;
    }
    if built == 0 {
        println!(".mo не собрано: нет po/<lang>.po с непустыми msgstr");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    if std::env::args().any(|a| a == "--build") {
        build(&root)?;
    } else {
        extract(&root)?;
    }
    Ok(())
}
