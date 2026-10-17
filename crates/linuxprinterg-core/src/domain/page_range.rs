//! Разбор диапазонов страниц вида `1-3,7,10-12`.

use crate::error::{Error, Result};

/// Диапазон страниц: вектор интервалов `[начало, конец]` (1-based, включительно).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRange(Vec<(u32, u32)>);

impl PageRange {
    /// Разбирает строку вида `1-3,7,10-12`. Пустая строка → `None`-диапазон
    /// (represented как пустой вектор) — обрабатывается вызывающей стороной.
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(PageRange(Vec::new()));
        }
        let mut ranges = Vec::new();
        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(Error::InvalidInput(format!("пустой элемент в «{input}»")));
            }
            let range = match part.split_once('-') {
                Some((a, b)) => {
                    let a = parse_num(a)?;
                    let b = parse_num(b)?;
                    if a == 0 || b == 0 {
                        return Err(Error::InvalidInput("страницы нумеруются с 1".to_string()));
                    }
                    if a > b {
                        return Err(Error::InvalidInput(format!(
                            "интервал «{part}»: начало больше конца"
                        )));
                    }
                    (a, b)
                }
                None => {
                    let n = parse_num(part)?;
                    if n == 0 {
                        return Err(Error::InvalidInput("страницы нумеруются с 1".to_string()));
                    }
                    (n, n)
                }
            };
            ranges.push(range);
        }
        // Сохраняем порядок ввода, но отклоняем пересекающиеся интервалы.
        for (i, &(a, b)) in ranges.iter().enumerate() {
            for &(c, d) in &ranges[i + 1..] {
                if a <= d && c <= b {
                    return Err(Error::InvalidInput(format!(
                        "интервалы «{a}-{b}» и «{c}-{d}» пересекаются"
                    )));
                }
            }
        }
        Ok(PageRange(ranges))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn ranges(&self) -> &[(u32, u32)] {
        &self.0
    }

    /// Общее число страниц, охваченных диапазоном.
    pub fn total_pages(&self) -> u64 {
        self.0.iter().map(|&(a, b)| (b - a + 1) as u64).sum()
    }

    /// Строка для CUPS-опции `page_ranges` (`lp -o page-ranges=...`).
    pub fn to_lp_string(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .iter()
                .map(|&(a, b)| {
                    if a == b {
                        a.to_string()
                    } else {
                        format!("{a}-{b}")
                    }
                })
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

fn parse_num(s: &str) -> Result<u32> {
    let s = s.trim();
    s.parse::<u32>()
        .map_err(|_| Error::InvalidInput(format!("не число: «{s}»")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_complex() {
        let r = PageRange::parse("1-3,7,10-12").unwrap();
        assert_eq!(r.ranges(), &[(1, 3), (7, 7), (10, 12)]);
        assert_eq!(r.total_pages(), 7);
        assert_eq!(r.to_lp_string().as_deref(), Some("1-3,7,10-12"));
    }

    #[test]
    fn parses_single_page() {
        let r = PageRange::parse("5").unwrap();
        assert_eq!(r.ranges(), &[(5, 5)]);
        assert_eq!(r.to_lp_string().as_deref(), Some("5"));
    }

    #[test]
    fn empty_means_all_pages() {
        let r = PageRange::parse("  ").unwrap();
        assert!(r.is_empty());
        assert_eq!(r.to_lp_string(), None);
    }

    #[test]
    fn rejects_zero_and_inverted_and_overlap() {
        assert!(PageRange::parse("0-3").is_err());
        assert!(PageRange::parse("7-2").is_err());
        assert!(PageRange::parse("1-3,2-5").is_err());
        assert!(PageRange::parse("abc").is_err());
        assert!(PageRange::parse("1,,2").is_err());
    }
}
