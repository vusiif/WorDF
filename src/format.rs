//! WorDF format detection, deep package pre-flight verification, and defensible resource limits.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read};
use zip::ZipArchive;

use crate::{ConversionError, ConversionErrorKind, Diagnostic, InputFormat};

pub const MAX_INPUT_BYTES: usize = 128 * 1024 * 1024; // 128 MiB
pub const MAX_DOCX_ENTRIES: usize = 1_000;
pub const MAX_DOCX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB
pub const MAX_DOCX_MAIN_XML_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB
pub const MAX_DOCX_AUX_XML_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
pub const MAX_DOCX_MEDIA_BYTES: u64 = 20 * 1024 * 1024; // 20 MiB
pub const MAX_IMAGE_DIMENSION: usize = 8_192;
pub const MAX_IMAGE_PIXELS: usize = 16_777_216; // 16 Mpx

pub const MAX_DOC_STREAMS: usize = 1_000;
pub const MAX_DOC_STREAM_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

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

pub fn validate_docx_package(bytes: &[u8]) -> Result<(), ConversionError> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupted DOCX ZIP archive: {}", e),
    })?;

    let entry_count = archive.len();
    if entry_count > MAX_DOCX_ENTRIES {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "DOCX ZIP entry count {} exceeds maximum allowed limit of {}",
                entry_count, MAX_DOCX_ENTRIES
            ),
        });
    }

    let mut has_content_types = false;
    let mut has_document_xml = false;
    let mut has_rels = false;
    let mut total_uncompressed: u64 = 0;

    for i in 0..entry_count {
        let file = archive.by_index(i).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Corrupted entry header in DOCX ZIP: {}", e),
        })?;

        let name = file.name().to_string();

        // Path traversal check
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            return Err(ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: format!("Illegal path traversal attempt in ZIP entry: '{}'", name),
            });
        }

        // Encryption check
        if name.contains("EncryptedPackage") || name.contains("EncryptionInfo") {
            return Err(ConversionError {
                kind: ConversionErrorKind::Encrypted,
                message: "Document is password protected / encrypted OOXML package".to_string(),
            });
        }

        if name == "[Content_Types].xml" {
            has_content_types = true;
        } else if name == "word/document.xml" {
            has_document_xml = true;
        } else if name == "_rels/.rels" || name == "word/_rels/document.xml.rels" {
            has_rels = true;
        }

        let uncompressed_size = file.size();
        let compressed_size = file.compressed_size();

        total_uncompressed = total_uncompressed.saturating_add(uncompressed_size);
        if total_uncompressed > MAX_DOCX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(ConversionError {
                kind: ConversionErrorKind::ResourceLimit,
                message: format!(
                    "Cumulative decompressed size exceeds limit of {} bytes (64 MiB)",
                    MAX_DOCX_TOTAL_UNCOMPRESSED_BYTES
                ),
            });
        }

        // Part-specific budget checks
        if name == "word/document.xml" {
            if uncompressed_size > MAX_DOCX_MAIN_XML_BYTES {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!(
                        "Main XML part 'word/document.xml' uncompressed size {} exceeds limit of {} bytes",
                        uncompressed_size, MAX_DOCX_MAIN_XML_BYTES
                    ),
                });
            }
        } else if name.ends_with(".xml") || name.ends_with(".rels") {
            if uncompressed_size > MAX_DOCX_AUX_XML_BYTES {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!(
                        "Auxiliary XML/rels part '{}' uncompressed size {} exceeds limit of {} bytes",
                        name, uncompressed_size, MAX_DOCX_AUX_XML_BYTES
                    ),
                });
            }
        } else if name.starts_with("word/media/") {
            if uncompressed_size > MAX_DOCX_MEDIA_BYTES {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!(
                        "Media item '{}' uncompressed size {} exceeds limit of {} bytes",
                        name, uncompressed_size, MAX_DOCX_MEDIA_BYTES
                    ),
                });
            }
        }

        // Compression ratio check for zip bombs
        if uncompressed_size > 1024 * 1024 && compressed_size > 0 {
            let ratio = uncompressed_size / compressed_size;
            if ratio > 100 {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!(
                        "Abnormal compression ratio ({} : 1) for entry '{}'",
                        ratio, name
                    ),
                });
            }
        }
    }

    if !has_content_types {
        return Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "Missing '[Content_Types].xml' part; input is a generic ZIP archive, not a Word document".to_string(),
        });
    }

    if !has_document_xml {
        return Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "Missing mandatory 'word/document.xml' part in package; non-Word OOXML packages (e.g. XLSX/PPTX) are not supported".to_string(),
        });
    }

    if !has_rels {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'word/_rels/document.xml.rels' or package relationships in DOCX package".to_string(),
        });
    }

    // Inspect image dimensions for media entries
    for i in 0..entry_count {
        let mut file = match archive.by_index(i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let name = file.name().to_string();
        if name.starts_with("word/media/") {
            let mut img_bytes = Vec::new();
            if file.read_to_end(&mut img_bytes).is_ok() {
                if let Ok(size) = imagesize::blob_size(&img_bytes) {
                    if size.width > MAX_IMAGE_DIMENSION || size.height > MAX_IMAGE_DIMENSION {
                        return Err(ConversionError {
                            kind: ConversionErrorKind::ResourceLimit,
                            message: format!(
                                "Image '{}' dimensions {}x{} exceed maximum allowed single-dimension limit of {}px",
                                name, size.width, size.height, MAX_IMAGE_DIMENSION
                            ),
                        });
                    }
                    let pixels = size.width.saturating_mul(size.height);
                    if pixels > MAX_IMAGE_PIXELS {
                        return Err(ConversionError {
                            kind: ConversionErrorKind::ResourceLimit,
                            message: format!(
                                "Image '{}' total pixels {} exceed maximum allowed limit of {}px",
                                name, pixels, MAX_IMAGE_PIXELS
                            ),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

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
