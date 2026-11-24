//! Multi-tiered font fallback chain for sequential glyph coverage resolution.
//! License: GPL-3.0-or-later

use crate::font::cmap::FontCmap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct FallbackFontItem {
    pub family_name: String,
    pub font_bytes: Arc<Vec<u8>>,
    pub cmap: FontCmap,
}

/// Ordered collection of fonts queried sequentially when resolving missing glyphs.
#[derive(Default, Clone, Debug)]
pub struct FontFallbackChain {
    pub items: Vec<FallbackFontItem>,
}

impl FontFallbackChain {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Appends a font to the fallback priority chain if its CMap can be parsed.
    pub fn push(&mut self, family_name: String, font_bytes: Vec<u8>) -> bool {
        if let Some(cmap) = FontCmap::from_bytes(&font_bytes) {
            self.items.push(FallbackFontItem {
                family_name,
                font_bytes: Arc::new(font_bytes),
                cmap,
            });
            true
        } else {
            false
        }
    }

    /// Appends font bytes using a pre-parsed FontCmap.
    pub fn push_with_cmap(&mut self, family_name: String, font_bytes: Vec<u8>, cmap: FontCmap) {
        self.items.push(FallbackFontItem {
            family_name,
            font_bytes: Arc::new(font_bytes),
            cmap,
        });
    }

    /// Queries the fallback chain in priority order for the first font providing a glyph for `ch`.
    /// Returns the family name of the matching font, or `None` if no font in the chain covers it.
    pub fn resolve_glyph(&self, ch: char) -> Option<&str> {
        for item in &self.items {
            if item.cmap.has_glyph(ch, &item.font_bytes) {
                return Some(&item.family_name);
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn primary_font_name(&self) -> Option<&str> {
        self.items.first().map(|i| i.family_name.as_str())
    }

    pub fn all_font_names(&self) -> Vec<String> {
        self.items.iter().map(|i| i.family_name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_ttf_format12(start_char: u32, end_char: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        buf.extend_from_slice(&1u16.to_be_bytes()); // 1 table
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

        let cmap_tag_offset = buf.len();
        buf.extend_from_slice(b"cmap");
        buf.extend_from_slice(&[0, 0, 0, 0]); // checksum
        let table_offset = (buf.len() + 8) as u32;
        buf.extend_from_slice(&table_offset.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]); // len placeholder

        // cmap header
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes()); // 1 subtable

        // subtable 0: Unicode Format 12
        buf.extend_from_slice(&3u16.to_be_bytes()); // Windows
        buf.extend_from_slice(&10u16.to_be_bytes()); // UCS-4
        let sub_offset = 4 + 8;
        buf.extend_from_slice(&(sub_offset as u32).to_be_bytes());

        // Format 12 subtable
        let f12_start = buf.len();
        buf.extend_from_slice(&12u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        let f12_len = 16 + 12;
        buf.extend_from_slice(&(f12_len as u32).to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // language
        buf.extend_from_slice(&1u32.to_be_bytes()); // 1 group

        buf.extend_from_slice(&start_char.to_be_bytes());
        buf.extend_from_slice(&end_char.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes()); // glyph id 1

        let total_table_len = buf.len() - (table_offset as usize);
        buf[cmap_tag_offset + 12..cmap_tag_offset + 16]
            .copy_from_slice(&(total_table_len as u32).to_be_bytes());
        let _ = f12_start;

        buf
    }

    #[test]
    fn test_font_fallback_chain_priority_resolution() {
        let font_a_bytes = build_test_ttf_format12(0x4E00, 0x4E00); // Only covers '一' (U+4E00)
        let font_b_bytes = build_test_ttf_format12(0x4E8C, 0x4E8C); // Only covers '二' (U+4E8C)

        let mut chain = FontFallbackChain::new();
        assert!(chain.push("FontA".to_string(), font_a_bytes));
        assert!(chain.push("FontB".to_string(), font_b_bytes));

        assert_eq!(chain.len(), 2);
        assert_eq!(chain.primary_font_name(), Some("FontA"));

        // '一' resolved by FontA
        assert_eq!(chain.resolve_glyph('一'), Some("FontA"));
        // '二' resolved by FontB (cascade fallback)
        assert_eq!(chain.resolve_glyph('二'), Some("FontB"));
        // '三' covered by neither
        assert_eq!(chain.resolve_glyph('三'), None);
    }
}
