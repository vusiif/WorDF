//! Office Open XML (DOCX) ZIP container pre-flight validation.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read};
use zip::ZipArchive;

use crate::{ConversionError, ConversionErrorKind};

pub const MAX_DOCX_ENTRIES: usize = 1_000;
pub const MAX_DOCX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB
pub const MAX_DOCX_MAIN_XML_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB
pub const MAX_DOCX_AUX_XML_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB
pub const MAX_DOCX_MEDIA_BYTES: u64 = 20 * 1024 * 1024; // 20 MiB
pub const MAX_IMAGE_DIMENSION: usize = 8_192;
pub const MAX_IMAGE_PIXELS: usize = 16_777_216; // 16 Mpx

/// Validate DOCX ZIP container, check mandatory parts, limits, and potential bombs.
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
        let mut file = archive.by_index(i).map_err(|e| ConversionError {
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
            // Buffer at most 64 KiB to determine dimensions, preventing large image memory spikes
            let mut img_header = Vec::new();
            if file
                .by_ref()
                .take(65536)
                .read_to_end(&mut img_header)
                .is_ok()
            {
                let size_opt = imagesize::blob_size(&img_header).or_else(|_| {
                    let _ = file.read_to_end(&mut img_header);
                    imagesize::blob_size(&img_header)
                });

                if let Ok(size) = size_opt {
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
            message:
                "Missing 'word/_rels/document.xml.rels' or package relationships in DOCX package"
                    .to_string(),
        });
    }

    Ok(())
}
