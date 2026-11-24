//! Font family name extraction from OpenType / TrueType / TTC tables.
//! License: GPL-3.0-or-later

/// Extract font family names from raw font bytes (TTF / OTF / TTC).
pub fn extract_font_family_names(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    if bytes.len() < 12 {
        return names;
    }

    let font_offsets: Vec<usize> = if bytes.starts_with(b"ttcf") {
        let num_fonts = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let max_possible = bytes.len().saturating_sub(12) / 4;
        let mut offsets = Vec::with_capacity(num_fonts.min(max_possible));
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
        let num_tables =
            u16::from_be_bytes([bytes[font_offset + 4], bytes[font_offset + 5]]) as usize;
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
                let Some(str_pos) = no
                    .checked_add(str_offset)
                    .and_then(|p| p.checked_add(offset))
                else {
                    continue;
                };
                let Some(str_end) = str_pos.checked_add(length) else {
                    continue;
                };
                if str_end <= bytes.len() {
                    let str_bytes = &bytes[str_pos..str_end];
                    if pid == 3 || pid == 0 {
                        // UTF-16BE
                        let (chunks, _) = str_bytes.as_chunks::<2>();
                        let utf16: Vec<u16> = chunks
                            .iter()
                            .map(|&[b0, b1]| u16::from_be_bytes([b0, b1]))
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
