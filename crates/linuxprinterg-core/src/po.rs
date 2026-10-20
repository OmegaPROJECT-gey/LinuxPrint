//! Форматы gettext — POT/PO-текст и бинарные MO — без системного gettext.
//!
//! Дорожная карта, п.8: извлечение `.po` из компилируемого словаря и
//! переводы (uk, de, fr) в стандартном формате gettext. Ни сборка, ни
//! рантайм не связаны с gettext-CLI/FFI:
//!
//! * **POT/PO** генерируем и читаем сами. Соглашение: `msgid` —
//!   pseudo-ключ словаря (`print.scaling`), исходный английский текст —
//!   в комментарии `# EN: …`, перевод — `msgstr`. Так поиск на рантайме
//!   идёт по ключу без неоднозначностей (разные ключи с одинаковым EN
//!   получают независимые переводы; `msgctxt` не нужен).
//! * **MO** (стандартный результат `msgfmt`) читает рантайм — переводы
//!   загружаются как overlay поверх словаря (см. [`crate::i18n`]).
//! * писатель MO нужен, чтобы собирать `.mo` из `.po` без `msgfmt`
//!   (бинарник `extract-po`); порядок строк детерминированный —
//!   воспроизводимые сборки.
//!
//! Ограничения (честно): без множественных форм (`msgid_plural`) и
//! контекстов — словарь их не использует; заголовок PO пересоздаётся
//! при генерации, а парсер пропускает незнакомые строки и записи
//! с флагом `fuzzy`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use crate::error::{Error, Result};

/// Сигнатура MO-файла в little-endian (байты, которые пишет `msgfmt`
/// на LE-системах).
const MO_MAGIC_LE: [u8; 4] = [0xde, 0x12, 0x04, 0x95];
/// Сигнатура MO-файла в big-endian.
const MO_MAGIC_BE: [u8; 4] = [0x95, 0x04, 0x12, 0xde];

/// Метаданные каталога — значение заголовочной записи MO/PO
/// (ключ `msgid ""`; наши парсеры её отбрасывают, стандартные
/// gettext-инструменты используют для кодировки).
pub fn catalog_header(lang: &str) -> String {
    format!(
        "Project-Id-Version: linuxprinterg\n\
         MIME-Version: 1.0\n\
         Content-Type: text/plain; charset=UTF-8\n\
         Content-Transfer-Encoding: 8bit\n\
         Language: {lang}\n"
    )
}

/// Заголовок PO-файла (метаданные gettext).
fn po_header(lang: &str) -> String {
    let mut out = String::from("msgid \"\"\nmsgstr \"\"\n");
    for line in catalog_header(lang).lines() {
        let _ = writeln!(out, "\"{line}\\n\"");
    }
    out.push('\n');
    out
}

/// Экранирование строки для кавычек PO.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Снять строку с кавычек (с раскодированием экранирования).
fn unquote(raw: &str, line: usize) -> Result<String> {
    let s = raw.trim();
    if !s.starts_with('"') {
        return Err(Error::msg(format!(
            "po:{line}: ожидается строка в кавычках"
        )));
    }
    let mut out = String::new();
    let mut chars = s.chars();
    chars.next(); // opening quote
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                // хвост до конца строки не должен содержать ничего
                // значимого (кроме пробелов)
                if chars.clone().any(|c| !c.is_whitespace()) {
                    return Err(Error::msg(format!("po:{line}: мусор после кавычки")));
                }
                return Ok(out);
            }
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err(Error::msg(format!("po:{line}: незакрытый escape"))),
            },
            other => out.push(other),
        }
    }
    Err(Error::msg(format!("po:{line}: незакрытая кавычка")))
}

fn render_entries<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>, out: &mut String) {
    for (key, en) in entries {
        // Исходный английский текст — однострочным комментарием
        // (с тем же экранированием, что и в строках PO: \n, \t, \").
        let _ = writeln!(out, "# EN: {}", escape(en));
        let _ = writeln!(out, "msgid \"{}\"", escape(key));
        out.push_str("msgstr \"\"\n\n");
    }
}

/// POT-шаблон из словаря: `msgid` = ключ, EN — в комментарии.
pub fn render_pot<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::from(
        "# Извлечено из компилируемого словаря (i18n.rs).\n\
         # Редактируются переводы в po/<lang>.po, этот файл пересоздаётся.\n",
    );
    out.push_str(&po_header(""));
    render_entries(entries, &mut out);
    out
}

/// Скелет PO-файла для языка: все ключи с пустым `msgstr`
/// (заполняется переводчиком; при пустом переводе рантайм откатится
/// на английский из словаря).
pub fn render_po<'a>(lang: &str, entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::from("# Скелет перевода (заполняется msgstr).\n");
    out.push_str(&po_header(lang));
    render_entries(entries, &mut out);
    out
}

/// Разобрать PO-текст в пары ключ → перевод.
///
/// Поддержано: комментарии, `#, fuzzy` (запись пропускается), заголовок
/// (`msgid ""`), многострочные строки, escape-последовательности,
/// `msgctxt`/`msgid_plural` (игнорируются), `msgstr[0]` (берётся как
/// единственная форма). Пустые `msgstr` сохраняются — отфильтровывает
/// их загрузчик (пустой перевод не должен затирать словарь).
pub fn parse_po(text: &str) -> Result<HashMap<String, String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        None,
        Id,
        Str,
        Ignored,
    }
    let mut map = HashMap::new();
    let mut key: Option<String> = None;
    let mut value: Option<String> = None;
    let mut phase = Phase::None;
    // Флаг fuzzy относится к записи, начинающейся СЛЕДУЮЩЕЙ после
    // комментария `#, fuzzy`; поэтому «текущий» и «ожидающий» разделены.
    let mut fuzzy = false;
    let mut pending_fuzzy = false;

    // Вставляет накопленную запись; сбрасывает только «текущий» fuzzy.
    let flush = |key: &mut Option<String>,
                 value: &mut Option<String>,
                 fuzzy: &mut bool,
                 map: &mut HashMap<String, String>| {
        if let Some(k) = key.take() {
            if !k.is_empty() && !*fuzzy {
                map.insert(k, value.take().unwrap_or_default());
            } else {
                let _ = value.take();
            }
        }
        *fuzzy = false;
    };

    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            pending_fuzzy = false;
            flush(&mut key, &mut value, &mut fuzzy, &mut map);
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(flags) = rest.strip_prefix(',') {
                if flags.split(',').any(|f| f.trim() == "fuzzy") {
                    pending_fuzzy = true;
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid_plural") {
            let _ = rest;
            phase = Phase::Ignored;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgctxt") {
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgstr[0]") {
            flush(&mut key, &mut value, &mut fuzzy, &mut map);
            phase = Phase::Str;
            value = Some(unquote(rest, line_no)?);
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgstr") {
            // и «msgstr[1]» и т. п. — остальные формы plural не используем
            let rest = rest.trim_start();
            if rest.starts_with('[') {
                phase = Phase::Ignored;
                continue;
            }
            if key.is_none() {
                return Err(Error::msg(format!("po:{line_no}: msgstr без msgid")));
            }
            phase = Phase::Str;
            value = Some(unquote(rest, line_no)?);
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid") {
            // fuzzy из комментария относится к ЭТОЙ записи — забираем
            // до сброса в flush (там сбрасывается предыдущая).
            let was_fuzzy = std::mem::take(&mut pending_fuzzy);
            flush(&mut key, &mut value, &mut fuzzy, &mut map);
            fuzzy = was_fuzzy;
            phase = Phase::Id;
            key = Some(unquote(rest, line_no)?);
            continue;
        }
        if line.starts_with('"') {
            let part = unquote(line, line_no)?;
            match phase {
                Phase::Id => key.get_or_insert_with(String::new).push_str(&part),
                Phase::Str => value.get_or_insert_with(String::new).push_str(&part),
                Phase::None | Phase::Ignored => {
                    return Err(Error::msg(format!(
                        "po:{line_no}: строка вне контекста msgid/msgstr"
                    )));
                }
            }
            continue;
        }
        // Незнакомая директива — игнорируем (формат не расширяем).
    }
    flush(&mut key, &mut value, &mut fuzzy, &mut map);
    Ok(map)
}

/// Собрать бинарный MO (little-endian, без hash-таблицы) из пар
/// ключ → перевод. Порядок строк сортируется — воспроизводимая сборка.
pub fn write_mo(entries: &HashMap<String, String>) -> Vec<u8> {
    let sorted: BTreeMap<&str, &str> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let n = sorted.len() as u32;
    let orig_table_off: u32 = 28;
    let trans_table_off: u32 = orig_table_off + n * 8;
    let orig_pool_off: u32 = trans_table_off + n * 8;

    let mut orig_pool: Vec<u8> = Vec::new();
    let mut trans_pool: Vec<u8> = Vec::new();
    let mut orig_index: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    // Смещения переводов зависят от размера пула оригиналов — заполняем
    // после сбора обоих пулов.
    let mut trans_lens: Vec<u32> = Vec::with_capacity(sorted.len());
    for (k, v) in &sorted {
        orig_index.push((k.len() as u32, orig_pool_off + orig_pool.len() as u32));
        orig_pool.extend_from_slice(k.as_bytes());
        orig_pool.push(0);
        trans_lens.push(v.len() as u32);
        trans_pool.extend_from_slice(v.as_bytes());
        trans_pool.push(0);
    }
    let trans_pool_off = orig_pool_off + orig_pool.len() as u32;
    let mut trans_index: Vec<(u32, u32)> = Vec::with_capacity(trans_lens.len());
    let mut rel = 0u32;
    for len in &trans_lens {
        trans_index.push((*len, trans_pool_off + rel));
        rel += len + 1; // + NUL
    }

    let mut out = Vec::with_capacity(orig_pool_off as usize + orig_pool.len() + trans_pool.len());
    out.extend_from_slice(&MO_MAGIC_LE);
    out.extend_from_slice(&0u32.to_le_bytes()); // ревизия major.minor =0.0
    out.extend_from_slice(&n.to_le_bytes()); // N
    out.extend_from_slice(&orig_table_off.to_le_bytes()); // O
    out.extend_from_slice(&trans_table_off.to_le_bytes()); // T
    out.extend_from_slice(&0u32.to_le_bytes()); // размер hash-таблицы
    out.extend_from_slice(&0u32.to_le_bytes()); // смещение hash-таблицы
    for (len, off) in &orig_index {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for (len, off) in &trans_index {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    out.extend_from_slice(&orig_pool);
    out.extend_from_slice(&trans_pool);
    out
}

/// Разобрать бинарный MO (LE и BE) в пары ключ → перевод.
///
/// Формат: заголовок28 байт (магия, ревизия, N, O, T, hash), затем две
/// таблицы `{length, offset}` (смещения от начала файла), затем сами
/// строки с NUL-терминатором. Записи plural (`\0` в оригинале) сводятся
/// к единственной форме — словарь plural не использует; заголовок
/// (`msgid ""`) отбрасывается.
pub fn parse_mo(data: &[u8]) -> Result<HashMap<String, String>> {
    if data.len() < 28 {
        return Err(Error::msg("mo: файл короче заголовка"));
    }
    let le = if data[..4] == MO_MAGIC_LE[..] {
        true
    } else if data[..4] == MO_MAGIC_BE[..] {
        false
    } else {
        return Err(Error::msg("mo: неизвестная сигнатура"));
    };
    let word = |off: usize| -> Result<u32> {
        let bytes: [u8; 4] = data[off..off + 4]
            .try_into()
            .map_err(|_| Error::msg("mo: оборванный заголовок"))?;
        Ok(if le {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let rev = word(4)?;
    if rev >> 16 != 0 {
        return Err(Error::msg(format!("mo: неподдерживаемая ревизия {rev}")));
    }
    let n = word(8)? as usize;
    let orig_off = word(12)? as usize;
    let trans_off = word(16)? as usize;
    let read_table = |base: usize| -> Result<Vec<&[u8]>> {
        base.checked_add(n * 8)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| Error::msg("mo: таблица строк выходит за файл"))?;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let at = base + i * 8;
            let len = word(at)? as usize;
            let off = word(at + 4)? as usize;
            out.push(
                data.get(off..off + len)
                    .ok_or_else(|| Error::msg("mo: строка выходит за файл"))?,
            );
        }
        Ok(out)
    };
    let orig = read_table(orig_off)?;
    let trans = read_table(trans_off)?;
    let mut map = HashMap::with_capacity(orig.len());
    for (o, t) in orig.iter().zip(trans.iter()) {
        let (o, t) = (*o, *t);
        let key = std::str::from_utf8(o).map_err(|_| Error::msg("mo: ключ не UTF-8"))?;
        let key = key.split('\0').next().unwrap_or(key);
        if key.is_empty() {
            continue; // заголовок
        }
        let text = std::str::from_utf8(t).map_err(|_| Error::msg("mo: перевод не UTF-8"))?;
        let text = text.split('\0').next().unwrap_or(text);
        map.insert(key.to_string(), text.to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<(&'static str, &'static str)> {
        vec![
            ("nav.print", "Print"),
            ("nav.scan", "Scan"),
            ("tricky", "Quote \" and \\ backslash\nand tab\t"),
        ]
    }

    #[test]
    fn pot_contains_header_refs_and_escapes() {
        let pot = render_pot(entries());
        assert!(pot.contains("Project-Id-Version: linuxprinterg"));
        assert!(pot.contains("Content-Type: text/plain; charset=UTF-8"));
        assert!(pot.contains("# EN: Print"));
        assert!(pot.contains("msgid \"nav.print\""));
        assert!(pot.contains("msgstr \"\""));
        // экранирование EN-комментария и содержимого
        assert!(pot.contains("Quote \\\" and \\\\ backslash\\nand tab\\t"));
    }

    #[test]
    fn po_skeleton_has_language_and_empty_msgstr() {
        let po = render_po("uk", entries());
        assert!(po.contains("\"Language: uk\\n\""));
        let parsed = parse_po(&po).unwrap();
        assert_eq!(parsed.len(), entries().len());
        assert!(parsed.values().all(|v| v.is_empty()));
        assert_eq!(parsed["nav.print"], "");
    }

    #[test]
    fn parse_multiline_escapes_and_fuzzy() {
        let text = r#"
# комментарий
msgid ""
msgstr ""
"Project-Id-Version: x\n"
"Language: uk\n"

#, fuzzy
msgid "fuzzy-key"
msgstr "пропускается"

msgid "long-key"
msgstr ""
"довольно "
"длинный"
" перевод"
"#;
        let map = parse_po(text).unwrap();
        assert!(!map.contains_key("fuzzy-key"), "fuzzy не должен попадать");
        assert!(!map.contains_key(""), "заголовок не должен попадать");
        assert_eq!(map.len(), 1);
        assert_eq!(map["long-key"], "довольно длинный перевод");
    }

    #[test]
    fn parse_unescapes_sequences() {
        let text = "msgid \"k\"\nmsgstr \"a\\nb\\\"c\\\\d\\t\"\n";
        let map = parse_po(text).unwrap();
        assert_eq!(map["k"], "a\nb\"c\\d\t");
    }

    #[test]
    fn parse_rejects_malformed_quotes() {
        assert!(parse_po("msgid nope\n").is_err());
        assert!(parse_po("msgid \"unterminated\n").is_err());
    }

    #[test]
    fn mo_header_entry_is_written_but_skipped_by_parser() {
        let mut src = HashMap::new();
        src.insert(String::new(), catalog_header("uk"));
        src.insert("app.title".to_string(), "LinuxPrinterG".to_string());
        let bytes = write_mo(&src);
        // Заголовок — первая запись (сортировка по ключу, "" < всё).
        let back = parse_mo(&bytes).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back["app.title"], "LinuxPrinterG");
        assert!(!back.contains_key(""));
        // Внешние инструменты должны видеть charset=UTF-8 без ошибок.
        let needle = b"Content-Type: text/plain; charset=UTF-8";
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "в .mo нет заголовочной записи с charset"
        );
    }

    #[test]
    fn mo_roundtrip_with_utf8_and_layout() {
        let mut src = HashMap::new();
        src.insert("print.scaling".to_string(), "Подгонка под лист".to_string());
        src.insert(
            "uk.only".to_string(),
            "Привіт, світе! \"кави\"\nрядок".to_string(),
        );
        src.insert("empty".to_string(), String::new());
        let bytes = write_mo(&src);
        // Заголовок: магия, ревизия0, N =3.
        assert_eq!(bytes[..4], MO_MAGIC_LE);
        assert_eq!(&bytes[4..8], &0u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 3);
        let back = parse_mo(&bytes).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn parse_mo_handles_big_endian_and_header_skip() {
        // Вручную собранный BE-MO c одной записью и заголовком.
        let mut b = Vec::new();
        b.extend_from_slice(&MO_MAGIC_BE);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&2u32.to_be_bytes()); // N (заголовок + запись)
        b.extend_from_slice(&28u32.to_be_bytes()); // O
        b.extend_from_slice(&44u32.to_be_bytes()); // T =28 +2*8
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        // Таблица оригиналов: "", "k"
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&60u32.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&61u32.to_be_bytes());
        // Таблица переводов: заголовок, "v"
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&62u32.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&63u32.to_be_bytes());
        // Пул: "\0" "k\0" "\0" "v\0" — позиции60..64 от начала файла
        assert_eq!(b.len(), 60);
        b.extend_from_slice(b"\0k\0v\0".as_slice());
        let map = parse_mo(&b).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["k"], "v");
    }

    #[test]
    fn parse_mo_rejects_garbage() {
        assert!(parse_mo(b"short").is_err());
        let mut bad = write_mo(&HashMap::new());
        bad[0] = 0x11;
        assert!(parse_mo(&bad).is_err());
    }

    #[test]
    fn write_mo_is_deterministic() {
        let mut a = HashMap::new();
        a.insert("b".to_string(), "2".to_string());
        a.insert("a".to_string(), "1".to_string());
        let mut b = HashMap::new();
        b.insert("a".to_string(), "1".to_string());
        b.insert("b".to_string(), "2".to_string());
        assert_eq!(write_mo(&a), write_mo(&b));
    }
}
