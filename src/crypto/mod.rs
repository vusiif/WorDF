//! Office document decryption and security module.
//! License: GPL-3.0-or-later

pub mod agile;
pub mod cipher;

use crate::{ConversionError, ConversionErrorKind};
use std::io::{Cursor, Read};

/// CFB / OLE2 compound document magic header bytes.
pub const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Inspect document bytes. If encrypted, attempt decryption with password if provided.
/// Returns `Ok(Some(decrypted_bytes))` if successfully decrypted,
/// `Ok(None)` if the document is not encrypted (plain DOCX/DOC),
/// or `Err(ConversionError)` if password is required or incorrect.
pub fn decrypt_office_document(
    bytes: &[u8],
    password: Option<&str>,
) -> Result<Option<Vec<u8>>, ConversionError> {
    if bytes.len() >= 8 && bytes[..8] == CFB_MAGIC {
        let cursor = Cursor::new(bytes);
        if let Ok(mut comp) = cfb::CompoundFile::open(cursor) {
            let mut has_encrypted_stream = false;
            let mut has_word_document = false;

            for entry in comp.walk() {
                if entry.is_stream() {
                    let name = entry.name();
                    if name == "EncryptedPackage" || name == "EncryptionInfo" {
                        has_encrypted_stream = true;
                    } else if name == "WordDocument" {
                        has_word_document = true;
                    }
                }
            }

            if has_encrypted_stream {
                let pwd = match password {
                    Some(p) if !p.is_empty() => p,
                    _ => {
                        return Err(ConversionError {
                            kind: ConversionErrorKind::Encrypted,
                            message: "Document is password-protected. Please provide a decryption password via --password <PASSWORD>.".to_string(),
                        });
                    }
                };

                let mut info_bytes = Vec::new();
                if let Ok(s) = comp.open_stream("/EncryptionInfo") {
                    let _ = s.take(1024 * 1024 + 1).read_to_end(&mut info_bytes);
                }
                let mut pkg_bytes = Vec::new();
                if let Ok(s) = comp.open_stream("/EncryptedPackage") {
                    let _ = s
                        .take(crate::format::MAX_INPUT_BYTES as u64 + 1)
                        .read_to_end(&mut pkg_bytes);
                }

                let decrypted = agile::decrypt_agile_package(&info_bytes, &pkg_bytes, pwd)?;
                return Ok(Some(decrypted));
            }

            if has_word_document {
                if let Ok(mut s) = comp.open_stream("/WordDocument") {
                    let mut fib_header = [0u8; 32];
                    if s.read(&mut fib_header).unwrap_or(0) >= 16 {
                        let flags = u16::from_le_bytes([fib_header[10], fib_header[11]]);
                        let encrypted = (flags & 0x0100) != 0 || (flags & 0x8000) != 0;
                        if encrypted {
                            return Err(ConversionError {
                                kind: ConversionErrorKind::Encrypted,
                                message: "Word 97-2003 binary DOC is password-protected / obfuscated. Decryption of legacy binary DOC is not supported.".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}
