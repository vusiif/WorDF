//! Unit tests for the Agile encryption parser/decryptor.
//! License: GPL-3.0-or-later

use super::*;
use std::io::Cursor;

#[test]
fn test_agile_roundtrip() {
    let docx_content = b"PK\x03\x04Dummy DOCX payload for agile test";
    let password = "SecretPassword123!";
    let cfb_bytes = build_agile_encrypted_cfb_for_test(docx_content, password);

    let mut comp = cfb::CompoundFile::open(Cursor::new(&cfb_bytes)).unwrap();
    let mut info_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut comp.open_stream("/EncryptionInfo").unwrap(),
        &mut info_bytes,
    )
    .unwrap();
    let mut pkg_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut comp.open_stream("/EncryptedPackage").unwrap(),
        &mut pkg_bytes,
    )
    .unwrap();

    // 1. Correct password
    let decrypted = decrypt_agile_package(&info_bytes, &pkg_bytes, password).unwrap();
    assert_eq!(decrypted, docx_content);

    // 2. Incorrect password
    let err = decrypt_agile_package(&info_bytes, &pkg_bytes, "WrongPassword").unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::Encrypted);
    assert!(err.message.contains("Incorrect password"));
}

#[test]
fn test_excessive_spin_count_defense() {
    let cfb_bytes = build_agile_encrypted_cfb_with_spin_for_test(b"dummy", "pass", 2_000_000);
    let mut comp = cfb::CompoundFile::open(Cursor::new(&cfb_bytes)).unwrap();
    let mut info_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut comp.open_stream("/EncryptionInfo").unwrap(),
        &mut info_bytes,
    )
    .unwrap();
    let mut pkg_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut comp.open_stream("/EncryptedPackage").unwrap(),
        &mut pkg_bytes,
    )
    .unwrap();

    let err = decrypt_agile_package(&info_bytes, &pkg_bytes, "pass").unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::ResourceLimit);
    assert!(err.message.contains("exceeds limit"));
}
