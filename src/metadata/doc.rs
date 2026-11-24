//! Microsoft Word 97-2003 binary DOC (CFB) OLEPS SummaryInformation metadata extraction.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read};

use super::DocumentMetadata;

pub const PID_TITLE: u32 = 2;
pub const PID_SUBJECT: u32 = 3;
pub const PID_AUTHOR: u32 = 4;
pub const PID_KEYWORDS: u32 = 5;

pub const VT_LPSTR: u32 = 30;
pub const VT_LPWSTR: u32 = 31;

fn parse_property_string(data: &[u8], prop_offset: usize) -> Option<String> {
    if prop_offset + 8 > data.len() {
        return None;
    }
    let vt = u32::from_le_bytes(data[prop_offset..prop_offset + 4].try_into().ok()?);
    let str_offset = prop_offset + 4;

    if vt == VT_LPSTR {
        let len = u32::from_le_bytes(data[str_offset..str_offset + 4].try_into().ok()?) as usize;
        let val_start = str_offset + 4;
        let val_end = (val_start + len).min(data.len());
        if val_start < val_end {
            let mut slice = &data[val_start..val_end];
            if slice.ends_with(b"\0") {
                slice = &slice[..slice.len() - 1];
            }
            return Some(
                String::from_utf8(slice.to_vec())
                    .unwrap_or_else(|_| String::from_utf8_lossy(slice).to_string()),
            );
        }
    } else if vt == VT_LPWSTR {
        let cch = u32::from_le_bytes(data[str_offset..str_offset + 4].try_into().ok()?) as usize;
        let val_start = str_offset + 4;
        let val_end = (val_start + cch * 2).min(data.len());
        if val_start < val_end {
            let u16_slice: Vec<u16> = data[val_start..val_end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let mut s = String::from_utf16_lossy(&u16_slice);
            if s.ends_with('\0') {
                s.pop();
            }
            return Some(s);
        }
    }

    None
}

/// Extract metadata from CFB SummaryInformation stream.
pub fn extract_doc_metadata(bytes: &[u8]) -> DocumentMetadata {
    let cursor = Cursor::new(bytes);
    let mut comp = match cfb::CompoundFile::open(cursor) {
        Ok(c) => c,
        Err(_) => return DocumentMetadata::default(),
    };

    let mut stream_path = None;
    for entry in comp.walk() {
        if entry.is_stream() {
            let name = entry.name();
            if name.ends_with("SummaryInformation") && !name.contains("DocumentSummaryInformation")
            {
                stream_path = Some(entry.path().to_path_buf());
                break;
            }
        }
    }

    let path = match stream_path {
        Some(p) => p,
        None => return DocumentMetadata::default(),
    };

    let mut stream_data = Vec::new();
    if let Ok(stream) = comp.open_stream(path) {
        let _ = stream
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut stream_data);
    }

    if stream_data.len() < 48 {
        return DocumentMetadata::default();
    }

    let byte_order = u16::from_le_bytes([stream_data[0], stream_data[1]]);
    if byte_order != 0xFFFE {
        return DocumentMetadata::default();
    }

    let sec_offset = u32::from_le_bytes(stream_data[44..48].try_into().unwrap_or([0; 4])) as usize;
    if sec_offset + 8 > stream_data.len() {
        return DocumentMetadata::default();
    }

    let sec_data = &stream_data[sec_offset..];
    let prop_count = u32::from_le_bytes(sec_data[4..8].try_into().unwrap_or([0; 4])) as usize;

    let mut metadata = DocumentMetadata::default();
    for i in 0..prop_count {
        let entry_offset = 8 + i * 8;
        if entry_offset + 8 > sec_data.len() {
            break;
        }
        let prop_id = u32::from_le_bytes(
            sec_data[entry_offset..entry_offset + 4]
                .try_into()
                .unwrap_or([0; 4]),
        );
        let prop_rel_offset = u32::from_le_bytes(
            sec_data[entry_offset + 4..entry_offset + 8]
                .try_into()
                .unwrap_or([0; 4]),
        ) as usize;

        let val_opt = parse_property_string(sec_data, prop_rel_offset);
        match prop_id {
            PID_TITLE => metadata.title = val_opt,
            PID_SUBJECT => metadata.subject = val_opt,
            PID_AUTHOR => metadata.author = val_opt,
            PID_KEYWORDS => metadata.keywords = val_opt,
            _ => {}
        }
    }

    metadata
}
