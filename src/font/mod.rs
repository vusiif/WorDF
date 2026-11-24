//! WorDF Font handling, CMap parsing, font family extraction and CJK metrics.
//! License: GPL-3.0-or-later

pub mod chain;
pub mod cmap;
pub mod family;

use std::ops::Deref;
#[cfg(feature = "embedded-cjk-font")]
use std::sync::OnceLock;

pub use chain::{FallbackFontItem, FontFallbackChain};
pub use cmap::{FontCmap, Format12Group, Format4Subtable};
pub use family::extract_font_family_names;

/// Built-in fallback font primary family name.
pub const EMBEDDED_CJK_FONT_NAME: &str = "LXGW WenKai Lite";

/// Built-in fallback font localized / Chinese family name.
pub const EMBEDDED_CJK_FONT_NAME_ALT: &str = "霞鹜文楷 轻便版";

/// Check if the embedded CJK fallback font is compiled into this build.
pub fn has_embedded_cjk_font() -> bool {
    cfg!(feature = "embedded-cjk-font")
}

#[cfg(feature = "embedded-cjk-font")]
const COMPRESSED_FONT_PAYLOAD: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/lxgw_wenkai_lite.zst"));

#[cfg(feature = "embedded-cjk-font")]
static DECOMPRESSED_FONT_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

/// Return decompressed bytes of the built-in CJK font.
/// Decompression happens lazily on first call, saving startup time and memory
/// when the embedded font is not needed.
pub fn embedded_cjk_font_bytes() -> &'static [u8] {
    #[cfg(feature = "embedded-cjk-font")]
    {
        DECOMPRESSED_FONT_CACHE.get_or_init(|| {
            zstd::decode_all(COMPRESSED_FONT_PAYLOAD)
                .expect("Failed to decompress compile-time compressed embedded font payload")
        })
    }
    #[cfg(not(feature = "embedded-cjk-font"))]
    {
        &[]
    }
}

/// Transparent proxy wrapper to maintain backward-compatibility with `EMBEDDED_CJK_FONT_BYTES`.
#[derive(Copy, Clone, Debug)]
pub struct EmbeddedFontProxy;

impl Deref for EmbeddedFontProxy {
    type Target = [u8];
    fn deref(&self) -> &'static [u8] {
        embedded_cjk_font_bytes()
    }
}

impl AsRef<[u8]> for EmbeddedFontProxy {
    fn as_ref(&self) -> &'static [u8] {
        embedded_cjk_font_bytes()
    }
}

/// Built-in fallback font bytes proxy (LXGW WenKai Lite Regular v1.530).
pub const EMBEDDED_CJK_FONT_BYTES: EmbeddedFontProxy = EmbeddedFontProxy;

/// Return list of recognized names for the embedded CJK font.
pub fn embedded_cjk_font_names() -> Vec<String> {
    if has_embedded_cjk_font() {
        vec![
            EMBEDDED_CJK_FONT_NAME.to_string(),
            EMBEDDED_CJK_FONT_NAME_ALT.to_string(),
        ]
    } else {
        vec![]
    }
}

/// Check if a character belongs to East Asian / CJK Unicode ranges (Chinese, Japanese, Korean, CJK Punctuation).
pub fn is_cjk_char(ch: char) -> bool {
    let cp = ch as u32;
    // Fast path: ASCII, Latin, Greek, Cyrillic, Hebrew, Arabic (< 0x1100) are never CJK
    if cp < 0x1100 {
        return false;
    }
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'       // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}'     // CJK Unified Ideographs Extension A
        | '\u{20000}'..='\u{2A6DF}'   // CJK Unified Ideographs Extension B
        | '\u{2A700}'..='\u{2B739}'   // CJK Unified Ideographs Extension C
        | '\u{2B740}'..='\u{2B81D}'   // CJK Unified Ideographs Extension D
        | '\u{2B820}'..='\u{2CEA1}'   // CJK Unified Ideographs Extension E
        | '\u{2CEB0}'..='\u{2EBE0}'   // CJK Unified Ideographs Extension F
        | '\u{30000}'..='\u{3134A}'   // CJK Unified Ideographs Extension G
        | '\u{31350}'..='\u{323AF}'   // CJK Unified Ideographs Extension H
        | '\u{2EBF0}'..='\u{2EE5D}'   // CJK Unified Ideographs Extension I
        | '\u{F900}'..='\u{FAFF}'     // CJK Compatibility Ideographs
        | '\u{2F800}'..='\u{2FA1F}'   // CJK Compatibility Ideographs Supplement
        | '\u{3040}'..='\u{309F}'     // Hiragana
        | '\u{30A0}'..='\u{30FF}'     // Katakana
        | '\u{31F0}'..='\u{31FF}'     // Katakana Phonetic Extensions
        | '\u{1100}'..='\u{11FF}'     // Hangul Jamo
        | '\u{3130}'..='\u{318F}'     // Hangul Compatibility Jamo
        | '\u{AC00}'..='\u{D7AF}'     // Hangul Syllables
        | '\u{3000}'..='\u{303F}'     // CJK Symbols and Punctuation (e.g. 、。)
        | '\u{FF00}'..='\u{FFEF}'     // Halfwidth and Fullwidth Forms (e.g. ！，)
    )
}

/// Count CJK characters in a string.
pub fn count_cjk_chars(text: &str) -> usize {
    // If the slice is entirely ASCII, count is guaranteed to be 0
    if text.is_ascii() {
        return 0;
    }
    text.chars().filter(|&c| is_cjk_char(c)).count()
}

static EMBEDDED_FONT_CMAP: std::sync::OnceLock<Option<FontCmap>> = std::sync::OnceLock::new();

/// Returns pre-parsed, cached FontCmap for the embedded font bytes,
/// ensuring Format 4 and Format 12 tables are parsed at most once across the process lifetime.
pub fn get_or_init_embedded_font_cmap(embedded_bytes: &[u8]) -> Option<&'static FontCmap> {
    EMBEDDED_FONT_CMAP
        .get_or_init(|| FontCmap::from_bytes(embedded_bytes))
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cjk_char_detection() {
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('文'));
        assert!(is_cjk_char('。'));
        assert!(is_cjk_char('！'));
        // Japanese Hiragana & Katakana
        assert!(is_cjk_char('あ'));
        assert!(is_cjk_char('ア'));
        // Korean Hangul Syllable & Jamo
        assert!(is_cjk_char('한'));
        assert!(is_cjk_char('글'));
        assert!(is_cjk_char('ᄀ'));
        assert!(!is_cjk_char('A'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char(' '));
    }

    #[test]
    fn test_cjk_char_count() {
        assert_eq!(count_cjk_chars("Hello, 世界！"), 3);
        assert_eq!(count_cjk_chars("Plain ASCII 123"), 0);
    }
}
