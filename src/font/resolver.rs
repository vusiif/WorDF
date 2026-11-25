//! Unified font resolution and cascade fallback engine.
//! License: GPL-3.0-or-later

use std::collections::HashSet;
use std::sync::Arc;

use crate::font::chain::FontFallbackChain;
use crate::font::cmap::FontCmap;
use crate::font::is_cjk_char;

#[derive(Clone, Debug)]
pub struct FontResolver {
    pub fallback_chain: FontFallbackChain,
    pub available_fonts: Vec<String>,
    pub primary_fallback_name: Option<String>,
    pub primary_fallback_bytes: Option<Arc<Vec<u8>>>,
    pub primary_cmap: Option<FontCmap>,
}

impl FontResolver {
    pub fn new(
        fallback_chain: FontFallbackChain,
        available_fonts: Vec<String>,
        primary_fallback_name: Option<String>,
        primary_fallback_bytes: Option<Vec<u8>>,
    ) -> Self {
        let primary_fallback_bytes = primary_fallback_bytes.map(Arc::new);
        let primary_cmap = if let Some(ref bytes) = primary_fallback_bytes {
            let embedded = crate::font::embedded_cjk_font_bytes();
            if !embedded.is_empty() && bytes.as_slice() == embedded {
                crate::font::get_or_init_embedded_font_cmap(bytes).cloned()
            } else {
                FontCmap::from_bytes(bytes)
            }
        } else {
            None
        };

        Self {
            fallback_chain,
            available_fonts,
            primary_fallback_name,
            primary_fallback_bytes,
            primary_cmap,
        }
    }

    /// True if either an explicit fallback chain or a primary fallback font is configured.
    pub fn has_fallback(&self) -> bool {
        !self.fallback_chain.is_empty() || self.primary_cmap.is_some()
    }

    /// Queries the fallback chain and primary fallback font to find the first font covering `ch`.
    pub fn resolve_glyph(&self, ch: char) -> Option<&str> {
        // 1. Try explicit fallback chain in priority order
        if let Some(name) = self.fallback_chain.resolve_glyph(ch) {
            return Some(name);
        }

        // 2. Try primary fallback font
        if let (Some(cmap), Some(ref bytes)) = (&self.primary_cmap, &self.primary_fallback_bytes) {
            if cmap.has_glyph(ch, bytes) {
                return self.primary_fallback_name.as_deref();
            }
        }

        None
    }

    /// Checks if a character has a resolved glyph in the available font ecosystem.
    /// If no fallback font or chain is active, returns true (coverage checking disabled).
    pub fn is_char_covered(&self, ch: char) -> bool {
        if !self.has_fallback() {
            return true;
        }
        self.resolve_glyph(ch).is_some()
    }

    /// Returns list of unique CJK characters in `text` that lack glyph coverage.
    pub fn find_missing_glyphs(&self, text: &str) -> Vec<char> {
        if !self.has_fallback() {
            return Vec::new();
        }
        let mut missing = Vec::new();
        let mut checked = HashSet::new();
        for c in text.chars() {
            if is_cjk_char(c) && checked.insert(c) && !self.is_char_covered(c) {
                missing.push(c);
            }
        }
        missing
    }

    /// Resolves the optimal fallback font family for an XML text run.
    pub fn resolve_font_for_run(&self, text: &str, default_font: Option<&str>) -> Option<String> {
        for c in text.chars() {
            if is_cjk_char(c) {
                if let Some(font) = self.resolve_glyph(c) {
                    return Some(font.to_string());
                }
            }
        }
        default_font.map(|s| s.to_string())
    }

    /// Returns true if the font family name is recognized as an available system/configured font.
    pub fn is_font_available(&self, name: &str) -> bool {
        self.available_fonts
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
    }
}
