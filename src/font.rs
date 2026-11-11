//! WorDF Font handling, CMap parsing, font family extraction and CJK metrics.
//! License: GPL-3.0-or-later

use std::path::{Path, PathBuf};

/// Built-in fallback font primary family name.
pub const EMBEDDED_CJK_FONT_NAME: &str = "LXGW WenKai Lite";

/// Built-in fallback font localized / Chinese family name.
pub const EMBEDDED_CJK_FONT_NAME_ALT: &str = "霞鹜文楷 轻便版";

/// Built-in fallback font file bytes (LXGW WenKai Lite Regular v1.530).
pub const EMBEDDED_CJK_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/LXGWWenKaiLite-Regular.ttf");

/// Return list of recognized names for the embedded CJK font.
pub fn embedded_cjk_font_names() -> Vec<String> {
    vec![
        EMBEDDED_CJK_FONT_NAME.to_string(),
        EMBEDDED_CJK_FONT_NAME_ALT.to_string(),
    ]
}

/// Check if a character belongs to East Asian / CJK Unicode ranges.
pub fn is_cjk_char(ch: char) -> bool {
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
    text.chars().filter(|&c| is_cjk_char(c)).count()
}

#[derive(Debug, Clone)]
pub struct Format4Subtable {
    pub seg_count: usize,
    pub end_codes: Vec<u16>,
    pub start_codes: Vec<u16>,
    pub id_deltas: Vec<i16>,
    pub id_range_offsets: Vec<u16>,
    pub subtable_offset: usize,
}

#[derive(Debug, Clone)]
pub struct Format12Group {
    pub start_char_code: u32,
    pub end_char_code: u32,
    pub start_glyph_id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct FontCmap {
    pub format4_subtables: Vec<Format4Subtable>,
    pub format12_groups: Vec<Vec<Format12Group>>,
}

impl FontCmap {
    /// Parse Unicode cmap subtables (Format 4 and Format 12) from raw font bytes.
    /// Supports TTF, OTF, and TTC fonts.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let font_offset = if bytes.starts_with(b"ttcf") {
            if bytes.len() < 16 {
                return None;
            }
            u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize
        } else {
            0
        };

        if bytes.len() < font_offset + 12 {
            return None;
        }

        let num_tables = u16::from_be_bytes([bytes[font_offset + 4], bytes[font_offset + 5]]) as usize;
        let mut cmap_offset = None;
        for i in 0..num_tables {
            let entry = font_offset + 12 + i * 16;
            if entry + 16 > bytes.len() {
                break;
            }
            if &bytes[entry..entry + 4] == b"cmap" {
                let off = u32::from_be_bytes([
                    bytes[entry + 8],
                    bytes[entry + 9],
                    bytes[entry + 10],
                    bytes[entry + 11],
                ]) as usize;
                cmap_offset = Some(off);
                break;
            }
        }

        let cmap_offset = cmap_offset?;
        if cmap_offset + 4 > bytes.len() {
            return None;
        }

        let num_subtables = u16::from_be_bytes([bytes[cmap_offset + 2], bytes[cmap_offset + 3]]) as usize;
        let mut format4_subtables = Vec::new();
        let mut format12_groups = Vec::new();

        for i in 0..num_subtables {
            let sub_entry = cmap_offset + 4 + i * 8;
            if sub_entry + 8 > bytes.len() {
                break;
            }
            let platform_id = u16::from_be_bytes([bytes[sub_entry], bytes[sub_entry + 1]]);
            let encoding_id = u16::from_be_bytes([bytes[sub_entry + 2], bytes[sub_entry + 3]]);
            let sub_offset = cmap_offset
                + u32::from_be_bytes([
                    bytes[sub_entry + 4],
                    bytes[sub_entry + 5],
                    bytes[sub_entry + 6],
                    bytes[sub_entry + 7],
                ]) as usize;

            let is_unicode = (platform_id == 0)
                || (platform_id == 3 && (encoding_id == 1 || encoding_id == 10));

            if !is_unicode || sub_offset + 2 > bytes.len() {
                continue;
            }

            let format = u16::from_be_bytes([bytes[sub_offset], bytes[sub_offset + 1]]);
            if format == 4 {
                if let Some(sub) = FontCmap::parse_format_4(bytes, sub_offset) {
                    format4_subtables.push(sub);
                }
            } else if format == 12 {
                if let Some(groups) = FontCmap::parse_format_12(bytes, sub_offset) {
                    format12_groups.push(groups);
                }
            }
        }

        if format4_subtables.is_empty() && format12_groups.is_empty() {
            None
        } else {
            Some(Self {
                format4_subtables,
                format12_groups,
            })
        }
    }

    fn parse_format_4(bytes: &[u8], offset: usize) -> Option<Format4Subtable> {
        if offset + 14 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        if offset + length > bytes.len() || length < 14 {
            return None;
        }

        let seg_count_x2 = u16::from_be_bytes([bytes[offset + 6], bytes[offset + 7]]) as usize;
        let seg_count = seg_count_x2 / 2;
        if seg_count == 0 {
            return None;
        }

        let end_code_offset = offset + 14;
        let start_code_offset = end_code_offset + seg_count * 2 + 2; // skip reservedPad
        let id_delta_offset = start_code_offset + seg_count * 2;
        let id_range_offset = id_delta_offset + seg_count * 2;

        if id_range_offset + seg_count * 2 > offset + length {
            return None;
        }

        let mut end_codes = Vec::with_capacity(seg_count);
        for i in 0..seg_count {
            let p = end_code_offset + i * 2;
            end_codes.push(u16::from_be_bytes([bytes[p], bytes[p + 1]]));
        }

        let mut start_codes = Vec::with_capacity(seg_count);
        for i in 0..seg_count {
            let p = start_code_offset + i * 2;
            start_codes.push(u16::from_be_bytes([bytes[p], bytes[p + 1]]));
        }

        let mut id_deltas = Vec::with_capacity(seg_count);
        for i in 0..seg_count {
            let p = id_delta_offset + i * 2;
            id_deltas.push(i16::from_be_bytes([bytes[p], bytes[p + 1]]));
        }

        let mut id_range_offsets = Vec::with_capacity(seg_count);
        for i in 0..seg_count {
            let p = id_range_offset + i * 2;
            id_range_offsets.push(u16::from_be_bytes([bytes[p], bytes[p + 1]]));
        }

        Some(Format4Subtable {
            seg_count,
            end_codes,
            start_codes,
            id_deltas,
            id_range_offsets,
            subtable_offset: offset,
        })
    }

    fn parse_format_12(bytes: &[u8], offset: usize) -> Option<Vec<Format12Group>> {
        if offset + 16 > bytes.len() {
            return None;
        }
        let length = u32::from_be_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        if offset + length > bytes.len() || length < 16 {
            return None;
        }

        let num_groups = u32::from_be_bytes([
            bytes[offset + 12],
            bytes[offset + 13],
            bytes[offset + 14],
            bytes[offset + 15],
        ]) as usize;

        if offset + 16 + num_groups * 12 > offset + length {
            return None;
        }

        let mut groups = Vec::with_capacity(num_groups);
        for i in 0..num_groups {
            let p = offset + 16 + i * 12;
            let start_char_code = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
            let end_char_code = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]);
            let start_glyph_id = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]);
            groups.push(Format12Group {
                start_char_code,
                end_char_code,
                start_glyph_id,
            });
        }

        Some(groups)
    }

    /// Check if character has a non-zero glyph ID in this font's cmaps.
    pub fn has_glyph(&self, ch: char, font_bytes: &[u8]) -> bool {
        let code = ch as u32;

        // Try Format 12 first (covers full Unicode 32-bit)
        for groups in &self.format12_groups {
            for g in groups {
                if code >= g.start_char_code && code <= g.end_char_code {
                    let gid = g.start_glyph_id + (code - g.start_char_code);
                    if gid > 0 {
                        return true;
                    }
                }
            }
        }

        // Try Format 4 for BMP (<= 0xFFFF)
        if code <= 0xFFFF {
            let c = code as u16;
            for sub in &self.format4_subtables {
                if let Some(gid) = FontCmap::lookup_format_4(sub, c, font_bytes) {
                    if gid > 0 {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn lookup_format_4(sub: &Format4Subtable, c: u16, font_bytes: &[u8]) -> Option<u16> {
        let seg_idx = sub.end_codes.iter().position(|&end| end >= c)?;
        if sub.start_codes[seg_idx] > c {
            return None;
        }

        let id_range_offset = sub.id_range_offsets[seg_idx] as usize;
        if id_range_offset == 0 {
            Some(((c as i32 + sub.id_deltas[seg_idx] as i32) & 0xFFFF) as u16)
        } else {
            let id_range_offset_addr = sub.subtable_offset + 14 + sub.seg_count * 2 + 2 + sub.seg_count * 2 + sub.seg_count * 2 + seg_idx * 2;
            let glyph_addr = id_range_offset_addr + id_range_offset + (c - sub.start_codes[seg_idx]) as usize * 2;
            if glyph_addr + 2 <= font_bytes.len() {
                let raw_id = u16::from_be_bytes([font_bytes[glyph_addr], font_bytes[glyph_addr + 1]]);
                if raw_id == 0 {
                    Some(0)
                } else {
                    Some(((raw_id as i32 + sub.id_deltas[seg_idx] as i32) & 0xFFFF) as u16)
                }
            } else {
                None
            }
        }
    }
}

/// Extract font family names from raw font bytes (TTF / OTF / TTC).
pub fn extract_font_family_names(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    if bytes.len() < 12 {
        return names;
    }

    let font_offsets: Vec<usize> = if bytes.starts_with(b"ttcf") {
        let num_fonts = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let mut offsets = Vec::with_capacity(num_fonts);
        for i in 0..num_fonts {
            let entry = 12 + i * 4;
            if entry + 4 > bytes.len() {
                break;
            }
            let off = u32::from_be_bytes([
                bytes[entry],
                bytes[entry + 1],
                bytes[entry + 2],
                bytes[entry + 3],
            ]) as usize;
            offsets.push(off);
        }
        offsets
    } else {
        vec![0]
    };

    for font_offset in font_offsets {
        if font_offset + 12 > bytes.len() {
            continue;
        }
        let num_tables = u16::from_be_bytes([bytes[font_offset + 4], bytes[font_offset + 5]]) as usize;
        let mut name_offset = None;
        for i in 0..num_tables {
            let entry_start = font_offset + 12 + i * 16;
            if entry_start + 16 > bytes.len() {
                break;
            }
            if &bytes[entry_start..entry_start + 4] == b"name" {
                let off = u32::from_be_bytes([
                    bytes[entry_start + 8],
                    bytes[entry_start + 9],
                    bytes[entry_start + 10],
                    bytes[entry_start + 11],
                ]) as usize;
                name_offset = Some(off);
                break;
            }
        }

        let Some(no) = name_offset else {
            continue;
        };
        if no + 6 > bytes.len() {
            continue;
        }

        let count = u16::from_be_bytes([bytes[no + 2], bytes[no + 3]]) as usize;
        let str_offset = u16::from_be_bytes([bytes[no + 4], bytes[no + 5]]) as usize;

        for i in 0..count {
            let rec = no + 6 + i * 12;
            if rec + 12 > bytes.len() {
                break;
            }
            let pid = u16::from_be_bytes([bytes[rec], bytes[rec + 1]]);
            let nid = u16::from_be_bytes([bytes[rec + 6], bytes[rec + 7]]);
            let length = u16::from_be_bytes([bytes[rec + 8], bytes[rec + 9]]) as usize;
            let offset = u16::from_be_bytes([bytes[rec + 10], bytes[rec + 11]]) as usize;

            if nid == 1 {
                let str_pos = no + str_offset + offset;
                if str_pos + length <= bytes.len() {
                    let str_bytes = &bytes[str_pos..str_pos + length];
                    if pid == 3 || pid == 0 {
                        // UTF-16BE
                        let utf16: Vec<u16> = str_bytes
                            .chunks_exact(2)
                            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                            .collect();
                        if let Ok(s) = String::from_utf16(&utf16) {
                            if !s.trim().is_empty() && !names.contains(&s) {
                                names.push(s);
                            }
                        }
                    } else if pid == 1 {
                        // Mac Roman / ASCII
                        if let Ok(s) = std::str::from_utf8(str_bytes) {
                            let s = s.to_string();
                            if !s.trim().is_empty() && !names.contains(&s) {
                                names.push(s);
                            }
                        }
                    }
                }
            }
        }
    }

    names
}

pub fn normalize_parent_dir(parent_opt: Option<&Path>) -> PathBuf {
    match parent_opt {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}
