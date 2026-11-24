//! Page range parser and selector for slice export.
//! License: GPL-3.0-or-later

use std::collections::BTreeSet;

pub const MAX_PAGE_NUMBER: usize = 100_000;
pub const MAX_PAGE_COUNT: usize = 10_000;

/// Represents a parsed, sorted, deduplicated set of 0-based page indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRange {
    /// 0-based sorted, deduplicated page indices.
    pub pages: Vec<usize>,
}

impl PageRange {
    /// Parse 1-based page range string (e.g. "1-3,5,8-10", "2", "1,3,5").
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("Page range specification cannot be empty".to_string());
        }

        let mut page_set = BTreeSet::new();

        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err("Empty segment in page range specification".to_string());
            }

            if let Some((start_s, end_s)) = part.split_once('-') {
                let start_s = start_s.trim();
                let end_s = end_s.trim();
                let start: usize = start_s
                    .parse()
                    .map_err(|_| format!("Invalid start page number '{}'", start_s))?;
                let end: usize = end_s
                    .parse()
                    .map_err(|_| format!("Invalid end page number '{}'", end_s))?;

                if start == 0 || end == 0 {
                    return Err("Page numbers must be 1-based (>= 1, got 0)".to_string());
                }
                if start > end {
                    return Err(format!(
                        "Invalid page range: start ({}) cannot be greater than end ({})",
                        start, end
                    ));
                }
                if start > MAX_PAGE_NUMBER || end > MAX_PAGE_NUMBER {
                    return Err(format!(
                        "Page number exceeds maximum allowed limit ({})",
                        MAX_PAGE_NUMBER
                    ));
                }
                for p in start..=end {
                    page_set.insert(p - 1);
                    if page_set.len() > MAX_PAGE_COUNT {
                        return Err(format!(
                            "Total selected pages exceed maximum allowed limit ({})",
                            MAX_PAGE_COUNT
                        ));
                    }
                }
            } else {
                let p: usize = part
                    .parse()
                    .map_err(|_| format!("Invalid page number '{}'", part))?;
                if p == 0 {
                    return Err("Page numbers must be 1-based (>= 1, got 0)".to_string());
                }
                if p > MAX_PAGE_NUMBER {
                    return Err(format!(
                        "Page number exceeds maximum allowed limit ({})",
                        MAX_PAGE_NUMBER
                    ));
                }
                page_set.insert(p - 1);
                if page_set.len() > MAX_PAGE_COUNT {
                    return Err(format!(
                        "Total selected pages exceed maximum allowed limit ({})",
                        MAX_PAGE_COUNT
                    ));
                }
            }
        }

        if page_set.is_empty() {
            return Err("No valid pages specified in range".to_string());
        }

        Ok(Self {
            pages: page_set.into_iter().collect(),
        })
    }

    /// Validate against total rendered pages and return the 0-based page indices.
    pub fn validate_against_total(&self, total_pages: usize) -> Result<Vec<usize>, String> {
        if total_pages == 0 {
            return Err("Document contains 0 pages".to_string());
        }
        for &p in &self.pages {
            if p >= total_pages {
                return Err(format!(
                    "Requested page {} exceeds document total page count ({})",
                    p + 1,
                    total_pages
                ));
            }
        }
        Ok(self.pages.clone())
    }
}

/// Validate raw page indices passed directly via ConversionOptions.pages.
pub fn validate_page_indices(indices: &[usize]) -> Result<(), crate::ConversionError> {
    if indices.len() > MAX_PAGE_COUNT {
        return Err(crate::ConversionError {
            kind: crate::ConversionErrorKind::PageOutOfRange,
            message: format!(
                "Total selected pages ({}) exceeds maximum allowed limit ({})",
                indices.len(),
                MAX_PAGE_COUNT
            ),
        });
    }
    for &idx in indices {
        if idx >= MAX_PAGE_NUMBER {
            return Err(crate::ConversionError {
                kind: crate::ConversionErrorKind::PageOutOfRange,
                message: format!(
                    "Page index {} exceeds maximum allowed page number limit ({})",
                    idx, MAX_PAGE_NUMBER
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_page() {
        let range = PageRange::parse("1").unwrap();
        assert_eq!(range.pages, vec![0]);

        let range = PageRange::parse(" 5 ").unwrap();
        assert_eq!(range.pages, vec![4]);
    }

    #[test]
    fn test_parse_range_and_deduplication() {
        let range = PageRange::parse("1-3").unwrap();
        assert_eq!(range.pages, vec![0, 1, 2]);

        let range = PageRange::parse("3,1-2,2").unwrap();
        assert_eq!(range.pages, vec![0, 1, 2]);

        let range = PageRange::parse(" 1 - 3 , 5 , 8 - 10 ").unwrap();
        assert_eq!(range.pages, vec![0, 1, 2, 4, 7, 8, 9]);
    }

    #[test]
    fn test_parse_invalid_inputs() {
        assert!(PageRange::parse("").is_err());
        assert!(PageRange::parse("   ").is_err());
        assert!(PageRange::parse("0").is_err());
        assert!(PageRange::parse("0-5").is_err());
        assert!(PageRange::parse("5-3").is_err());
        assert!(PageRange::parse("abc").is_err());
        assert!(PageRange::parse("1,,").is_err());
        assert!(PageRange::parse("1-").is_err());
        assert!(PageRange::parse("-5").is_err());
        assert!(PageRange::parse("100001").is_err());
        assert!(PageRange::parse("1-20000").is_err());
    }

    #[test]
    fn test_validate_against_total() {
        let range = PageRange::parse("1-3,5").unwrap();
        assert!(range.validate_against_total(5).is_ok());
        assert_eq!(range.validate_against_total(5).unwrap(), vec![0, 1, 2, 4]);

        let err = range.validate_against_total(4).unwrap_err();
        assert!(err.contains("exceeds document total page count"));

        let err = range.validate_against_total(0).unwrap_err();
        assert!(err.contains("0 pages"));
    }

    #[test]
    fn test_deduplication_at_max_limit() {
        let range = PageRange::parse("1-10000,1,100").unwrap();
        assert_eq!(range.pages.len(), 10000);
        assert_eq!(range.pages[0], 0);
        assert_eq!(range.pages[9999], 9999);

        assert!(validate_page_indices(&[0, 1, 2]).is_ok());
        assert!(validate_page_indices(&[MAX_PAGE_NUMBER]).is_err());
    }
}
