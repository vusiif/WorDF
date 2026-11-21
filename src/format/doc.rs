//! Microsoft Word 97-2003 Binary DOC (Compound File Binary) pre-flight validation.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read};

use crate::{ConversionError, ConversionErrorKind};

pub const MAX_DOC_STREAMS: usize = 1_000;
pub const MAX_DOC_STREAM_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

/// Validate Compound File Binary (CFB/OLE) structure and WordDocument FIB.
pub fn validate_doc_package(bytes: &[u8]) -> Result<(), ConversionError> {
    let cursor = Cursor::new(bytes);
    let mut comp = cfb::CompoundFile::open(cursor).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupted Compound File Binary (CFB/OLE): {}", e),
    })?;

    let mut stream_count = 0;
    let mut has_word_document = false;

    for entry in comp.walk() {
        stream_count += 1;
        if stream_count > MAX_DOC_STREAMS {
            return Err(ConversionError {
                kind: ConversionErrorKind::ResourceLimit,
                message: format!(
                    "DOC CFB stream count exceeds maximum limit of {}",
                    MAX_DOC_STREAMS
                ),
            });
        }
        if entry.is_stream() && entry.name() == "WordDocument" {
            has_word_document = true;
        }
    }

    if !has_word_document {
        return Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "CFB container does not contain 'WordDocument' stream (not a Word DOC, possibly Excel XLS or PowerPoint PPT)".to_string(),
        });
    }

    // Open and check WordDocument stream & FIB
    let mut stream = comp
        .open_stream("/WordDocument")
        .map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Failed to open WordDocument stream: {}", e),
        })?;

    let stream_len = stream.len();
    if stream_len > MAX_DOC_STREAM_BYTES {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "WordDocument stream length {} bytes exceeds limit of {} bytes (64 MiB)",
                stream_len, MAX_DOC_STREAM_BYTES
            ),
        });
    }

    let mut fib_header = [0u8; 32];
    let read_len = stream.read(&mut fib_header).unwrap_or(0);
    if read_len < 16 {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "WordDocument stream is truncated (less than 16 bytes)".to_string(),
        });
    }

    let w_ident = u16::from_le_bytes([fib_header[0], fib_header[1]]);
    if w_ident != 0xA5EC && w_ident != 0xA5DC && w_ident != 0xA5C6 {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Invalid FIB wIdent in WordDocument stream: 0x{:04X}", w_ident),
        });
    }

    let flags = u16::from_le_bytes([fib_header[10], fib_header[11]]);
    let encrypted = (flags & 0x0100) != 0;
    let obfuscated = (flags & 0x8000) != 0;

    if encrypted || obfuscated {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: "Document is password protected / encrypted Word binary DOC".to_string(),
        });
    }

    Ok(())
}
