//! WorDF format detection, deep package pre-flight verification, and defensible resource limits.
//! License: GPL-3.0-or-later

pub mod doc;
pub mod docx;

pub use doc::{validate_doc_package, MAX_DOC_STREAMS, MAX_DOC_STREAM_BYTES};
pub use docx::{
    validate_docx_package, MAX_DOCX_AUX_XML_BYTES, MAX_DOCX_ENTRIES, MAX_DOCX_MAIN_XML_BYTES,
    MAX_DOCX_MEDIA_BYTES, MAX_DOCX_TOTAL_UNCOMPRESSED_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS,
};

use crate::{ConversionError, ConversionErrorKind, Diagnostic, InputFormat};

pub const MAX_INPUT_BYTES: usize = 128 * 1024 * 1024; // 128 MiB

pub const DOC_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
pub const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];

/// Quick magic-byte check.
pub fn detect_format_magic(bytes: &[u8]) -> Option<InputFormat> {
    if bytes.len() >= 8 && bytes[..8] == DOC_MAGIC {
        Some(InputFormat::Doc)
    } else if bytes.len() >= 4 && bytes[..4] == ZIP_MAGIC {
        Some(InputFormat::Docx)
    } else {
        None
    }
}

/// Deep pre-flight format verification against defensible limits.
pub fn verify_and_detect_format(
    bytes: &[u8],
    declared: InputFormat,
) -> Result<(InputFormat, Vec<Diagnostic>), ConversionError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "Input size {} bytes exceeds maximum limit of {} bytes (128 MiB)",
                bytes.len(),
                MAX_INPUT_BYTES
            ),
        });
    }

    if bytes.starts_with(b"{\\rtf") {
        return Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "RTF format is explicitly rejected by WorDF P1 contract".to_string(),
        });
    }

    let trimmed = bytes.iter().take(256).copied().collect::<Vec<u8>>();
    let trimmed_str = String::from_utf8_lossy(&trimmed).to_lowercase();
    if trimmed_str.trim_start().starts_with("<!doctype html")
        || trimmed_str.trim_start().starts_with("<html")
    {
        return Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "HTML documents are explicitly rejected by WorDF P1 contract".to_string(),
        });
    }

    let detected = match detect_format_magic(bytes) {
        Some(fmt) => fmt,
        None => {
            return Err(ConversionError {
                kind: ConversionErrorKind::UnsupportedFormat,
                message: "Unrecognized file format or non-Word container magic bytes".to_string(),
            });
        }
    };

    let warnings = Vec::new();

    // Check declared vs detected
    match declared {
        InputFormat::Auto => {}
        InputFormat::Doc => {
            if detected != InputFormat::Doc {
                return Err(ConversionError {
                    kind: ConversionErrorKind::UnsupportedFormat,
                    message: format!(
                        "Explicit format 'doc' does not match detected format '{:?}'",
                        detected
                    ),
                });
            }
        }
        InputFormat::Docx => {
            if detected != InputFormat::Docx {
                return Err(ConversionError {
                    kind: ConversionErrorKind::UnsupportedFormat,
                    message: format!(
                        "Explicit format 'docx' does not match detected format '{:?}'",
                        detected
                    ),
                });
            }
        }
    }

    match detected {
        InputFormat::Docx => {
            validate_docx_package(bytes)?;
        }
        InputFormat::Doc => {
            validate_doc_package(bytes)?;
        }
        InputFormat::Auto => unreachable!(),
    }

    Ok((detected, warnings))
}
