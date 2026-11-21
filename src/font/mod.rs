//! WorDF Font handling, CMap parsing, font family extraction and CJK metrics.
//! License: GPL-3.0-or-later

pub mod cmap;
pub mod family;

use std::ops::Deref;
#[cfg(feature = "embedded-cjk-font")]
use std::sync::OnceLock;

pub use cmap::{get_or_init_embedded_font_cmap, FontCmap, Format12Group, Format4Subtable};
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
const COMPRESSED_FONT_PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lxgw_wenkai_lite.deflate"));

#[cfg(feature = "embedded-cjk-font")]
static DECOMPRESSED_FONT_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

/// Return decompressed bytes of the built-in CJK font.
/// Decompression happens lazily on first call, saving startup time and memory
/// when the embedded font is not needed.
pub fn embedded_cjk_font_bytes() -> &'static [u8] {
    #[cfg(feature = "embedded-cjk-font")]
    {
        DECOMPRESSED_FONT_CACHE.get_or_init(|| {
            miniz_oxide::inflate::decompress_to_vec_zlib(COMPRESSED_FONT_PAYLOAD)
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

/// Check if a character belongs to East Asian / CJK Unicode ranges.
pub fn is_cjk_char(ch: char) -> bool {
    let cp = ch as u32;
    // Fast path: ASCII, Latin, Greek, and Cyrillic (< 0x3000) are never CJK
    if cp < 0x3000 {
        return false;
    }
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'       // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}'     // CJK Unified Ideographs Extension A
        | '\u{20000}'..='\u{2A6DF}'   // CJK Unified Ideographs Extension B
        | '\u{F900}'..='\u{FAFF}'     // CJK Compatibility Ideographs
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cjk_char_detection() {
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('文'));
        assert!(is_cjk_char('。'));
        assert!(is_cjk_char('！'));
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
