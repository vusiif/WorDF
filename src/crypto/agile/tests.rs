//! Unit tests for the Agile encryption parser/decryptor and integrity verification.
//! License: GPL-3.0-or-later

use super::*;
use base64::prelude::*;
use std::io::Cursor;

#[test]
fn test_agile_roundtrip_with_integrity() {
    let docx_content = b"PK\x03\x04Dummy DOCX payload for agile test with data integrity";
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

    // 3. Tampered ciphertext triggers HMAC integrity check failure
    let mut tampered_pkg = pkg_bytes.clone();
    let last = tampered_pkg.len() - 1;
    tampered_pkg[last] ^= 0x01;
    let err_tamper = decrypt_agile_package(&info_bytes, &tampered_pkg, password).unwrap_err();
    assert_eq!(err_tamper.kind, ConversionErrorKind::CorruptInput);
    assert!(err_tamper.message.contains("HMAC mismatch"));
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

#[test]
fn test_xml_parser_single_line_and_namespaces() {
    let s_b64 = base64::prelude::BASE64_STANDARD.encode([0x42u8; 16]);
    let dummy_b64 = base64::prelude::BASE64_STANDARD.encode([0x11u8; 16]);
    let xml = format!(
        r#"<encryption xmlns="http://schemas.microsoft.com/office/2006/encryption"><keyData saltSize='16' blockSize='16' keyBits='256' hashAlgorithm='SHA512' cipherAlgorithm='AES' cipherChaining='ChainingModeCBC' saltValue='{0}'/><keyEncryptors><keyEncryptor uri="http://schemas.microsoft.com/office/2006/keyEncryptor/password"><p:encryptedKey xmlns:p="http://schemas.microsoft.com/office/2006/keyEncryptor/password" spinCount="50000" saltSize="16" blockSize="16" keyBits="256" hashAlgorithm="SHA512" saltValue="{0}" encryptedVerifierHashInput="{1}" encryptedVerifierHashValue="{1}" encryptedKeyValue="{1}"/></keyEncryptor></keyEncryptors></encryption>"#,
        s_b64, dummy_b64
    );
    let info = parser::parse_agile_xml(&xml).unwrap();
    assert_eq!(info.key_data.key_bytes, 32);
    assert_eq!(info.key_data.block_size, 16);
    assert_eq!(info.password_encryptor.spin_count, 50000);
    assert_eq!(info.password_encryptor.key_bytes, 32);
    assert!(info.data_integrity.is_none());
}

#[test]
fn test_xml_parser_unsupported_cipher_rejection() {
    let xml = r#"<encryption xmlns="http://schemas.microsoft.com/office/2006/encryption"><keyData saltSize="16" blockSize="16" keyBits="256" hashAlgorithm="SHA512" cipherAlgorithm="DES" saltValue="AAAAAAAAAAAAAA=="/><keyEncryptors><keyEncryptor uri="http://schemas.microsoft.com/office/2006/keyEncryptor/password"><encryptedKey spinCount="1000" saltSize="16" blockSize="16" keyBits="256" hashAlgorithm="SHA512" saltValue="BBBBBBBBBBBBBB==" encryptedVerifierHashInput="CCCC" encryptedVerifierHashValue="DDDD" encryptedKeyValue="EEEE"/></keyEncryptor></keyEncryptors></encryption>"#;
    let err = parser::parse_agile_xml(xml).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::Encrypted);
    assert!(err
        .message
        .contains("Unsupported encryption cipherAlgorithm"));
}
