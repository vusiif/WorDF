//! ECMA-376 / MS-OFFCRYPTO Agile Encryption parser and decryptor.
//! License: GPL-3.0-or-later

use std::io::Cursor;
use base64::prelude::*;

use crate::crypto::cipher::{aes_cbc_decrypt, aes_cbc_encrypt, HashAlgorithm};
use crate::{ConversionError, ConversionErrorKind};

pub const BLOCK_KEY_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
pub const BLOCK_KEY_VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
pub const BLOCK_KEY_KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
pub const MAX_SPIN_COUNT: u32 = 1_000_000;

fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let key = format!("{}=\"", attr);
    let start = tag.find(&key)? + key.len();
    let end = start + tag[start..].find('"')?;
    Some(tag[start..end].to_string())
}

fn derive_key(h: &[u8], block_key: &[u8], key_bytes: usize, algo: HashAlgorithm) -> Vec<u8> {
    let mut buf = Vec::with_capacity(h.len() + block_key.len());
    buf.extend_from_slice(h);
    buf.extend_from_slice(block_key);
    let mut digest = algo.digest(&buf);
    if digest.len() >= key_bytes { digest.truncate(key_bytes); } else { digest.resize(key_bytes, 0x36); }
    digest
}

pub fn derive_password_hash(password: &str, salt: &[u8], spin_count: u32, algo: HashAlgorithm) -> Vec<u8> {
    let pwd_utf16: Vec<u8> = password.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let mut h_buf = Vec::with_capacity(salt.len() + pwd_utf16.len());
    h_buf.extend_from_slice(salt);
    h_buf.extend_from_slice(&pwd_utf16);
    let mut h = algo.digest(&h_buf);
    for i in 0..spin_count {
        let mut iter_buf = Vec::with_capacity(4 + h.len());
        iter_buf.extend_from_slice(&i.to_le_bytes());
        iter_buf.extend_from_slice(&h);
        h = algo.digest(&iter_buf);
    }
    h
}

pub fn decrypt_agile_package(
    info_bytes: &[u8],
    pkg_bytes: &[u8],
    password: &str,
) -> Result<Vec<u8>, ConversionError> {
    if info_bytes.len() < 8 {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "EncryptionInfo stream is truncated (less than 8 bytes)".to_string(),
        });
    }

    let (v_major, v_minor) = (u16::from_le_bytes([info_bytes[0], info_bytes[1]]), u16::from_le_bytes([info_bytes[2], info_bytes[3]]));
    if v_major != 4 || v_minor != 4 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported Office encryption version: {}.{}", v_major, v_minor),
        });
    }

    let xml = String::from_utf8_lossy(&info_bytes[8..]);
    let key_data_tag = xml.lines().find(|l| l.contains("<keyData") || l.contains("<p:keyData")).unwrap_or("");
    let enc_key_tag = xml.lines().find(|l| l.contains("encryptedKey")).unwrap_or("");

    let key_salt = BASE64_STANDARD.decode(extract_attr(key_data_tag, "saltValue").unwrap_or_default()).map_err(|_| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: "Invalid base64 in keyData saltValue".to_string(),
    })?;
    let key_algo = HashAlgorithm::parse(&extract_attr(key_data_tag, "hashAlgorithm").unwrap_or_default()).unwrap_or(HashAlgorithm::Sha512);

    let spin_count: u32 = extract_attr(enc_key_tag, "spinCount").and_then(|s| s.parse().ok()).unwrap_or(100_000);
    if spin_count > MAX_SPIN_COUNT {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!("Encryption spinCount {} exceeds limit {}", spin_count, MAX_SPIN_COUNT),
        });
    }
    let enc_salt = BASE64_STANDARD.decode(extract_attr(enc_key_tag, "saltValue").unwrap_or_default()).map_err(|_| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: "Invalid base64 in encryptedKey saltValue".to_string(),
    })?;
    let enc_algo = HashAlgorithm::parse(&extract_attr(enc_key_tag, "hashAlgorithm").unwrap_or_default()).unwrap_or(HashAlgorithm::Sha512);
    let key_bytes = extract_attr(enc_key_tag, "keyBits").and_then(|s| s.parse::<usize>().ok()).unwrap_or(256) / 8;
    if key_bytes != 16 && key_bytes != 32 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported encryption key length: {} bits", key_bytes * 8),
        });
    }

    let enc_ver_input = BASE64_STANDARD.decode(extract_attr(enc_key_tag, "encryptedVerifierHashInput").unwrap_or_default()).unwrap_or_default();
    let enc_ver_val = BASE64_STANDARD.decode(extract_attr(enc_key_tag, "encryptedVerifierHashValue").unwrap_or_default()).unwrap_or_default();
    let enc_key_val = BASE64_STANDARD.decode(extract_attr(enc_key_tag, "encryptedKeyValue").unwrap_or_default()).unwrap_or_default();

    let mut iv = [0x36u8; 16];
    let copy_len = enc_salt.len().min(16);
    iv[..copy_len].copy_from_slice(&enc_salt[..copy_len]);

    let mut h = derive_password_hash(password, &enc_salt, spin_count, enc_algo);
    let mut k_input = derive_key(&h, &BLOCK_KEY_VERIFIER_INPUT, key_bytes, enc_algo);
    let mut k_val = derive_key(&h, &BLOCK_KEY_VERIFIER_VALUE, key_bytes, enc_algo);
    let mut k_key = derive_key(&h, &BLOCK_KEY_KEY_VALUE, key_bytes, enc_algo);

    let ver_input = aes_cbc_decrypt(&k_input, &iv, &enc_ver_input).map_err(|e| ConversionError {
        kind: ConversionErrorKind::Encrypted,
        message: format!("Decryption failed for verifier input: {}", e),
    })?;
    let ver_val = aes_cbc_decrypt(&k_val, &iv, &enc_ver_val).map_err(|e| ConversionError {
        kind: ConversionErrorKind::Encrypted,
        message: format!("Decryption failed for verifier value: {}", e),
    })?;

    let expected_hash = enc_algo.digest(&ver_input);
    if ver_val.len() < expected_hash.len()
        || !constant_time_compare(&ver_val[..expected_hash.len()], expected_hash.as_slice())
    {
        h.fill(0); k_input.fill(0); k_val.fill(0); k_key.fill(0);
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: "Decryption failed: Incorrect password".to_string(),
        });
    }

    let mut dec_key = aes_cbc_decrypt(&k_key, &iv, &enc_key_val).map_err(|e| ConversionError {
        kind: ConversionErrorKind::Encrypted,
        message: format!("Decryption failed for package key: {}", e),
    })?;
    h.fill(0); k_input.fill(0); k_val.fill(0); k_key.fill(0);
    if dec_key.len() < key_bytes {
        dec_key.fill(0);
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Decrypted package key buffer shorter than declared key size".to_string(),
        });
    }
    let package_key = &dec_key[..key_bytes];

    if pkg_bytes.len() < 8 {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "EncryptedPackage stream is truncated".to_string(),
        });
    }

    let orig_size = u64::from_le_bytes(pkg_bytes[..8].try_into().unwrap());
    if orig_size > 128 * 1024 * 1024 {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!("Encrypted package size {} exceeds 128 MiB resource limit", orig_size),
        });
    }

    let ciphertext = &pkg_bytes[8..];
    let mut decrypted_payload = Vec::with_capacity(ciphertext.len());
    let segment_size = 4096;
    let num_segments = ciphertext.len().div_ceil(segment_size);

    for seg_idx in 0..num_segments {
        let start = seg_idx * segment_size;
        let end = (start + segment_size).min(ciphertext.len());
        let seg_cipher = &ciphertext[start..end];
        if seg_cipher.is_empty() { break; }

        let mut seg_hash_buf = Vec::with_capacity(key_salt.len() + 4);
        seg_hash_buf.extend_from_slice(&key_salt);
        seg_hash_buf.extend_from_slice(&(seg_idx as u32).to_le_bytes());
        let seg_iv_hash = key_algo.digest(&seg_hash_buf);

        let seg_dec = aes_cbc_decrypt(package_key, &seg_iv_hash[..16], seg_cipher).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Decryption failed on package segment {}: {}", seg_idx, e),
        })?;
        decrypted_payload.extend_from_slice(&seg_dec);
    }

    dec_key.fill(0);
    let final_len = (orig_size as usize).min(decrypted_payload.len());
    decrypted_payload.truncate(final_len);
    Ok(decrypted_payload)
}

/// Helper for constructing standard MS-OFFCRYPTO Agile encrypted CFB package in tests.
pub fn build_agile_encrypted_cfb_for_test(docx_bytes: &[u8], password: &str) -> Vec<u8> {
    build_agile_encrypted_cfb_with_spin_for_test(docx_bytes, password, 1_000)
}

pub fn build_agile_encrypted_cfb_with_spin_for_test(docx_bytes: &[u8], password: &str, spin_count: u32) -> Vec<u8> {
    let (salt, enc_algo, key_bytes) = ([0x7au8; 16], HashAlgorithm::Sha512, 32usize);
    let h = derive_password_hash(password, &salt, spin_count, enc_algo);
    let k_input = derive_key(&h, &BLOCK_KEY_VERIFIER_INPUT, key_bytes, enc_algo);
    let k_val = derive_key(&h, &BLOCK_KEY_VERIFIER_VALUE, key_bytes, enc_algo);
    let k_key = derive_key(&h, &BLOCK_KEY_KEY_VALUE, key_bytes, enc_algo);

    let ver_input = [0x55u8; 16];
    let mut ver_hash_padded = enc_algo.digest(&ver_input);
    while !ver_hash_padded.len().is_multiple_of(16) { ver_hash_padded.push(0); }

    let pkg_key = [0x33u8; 32];
    let enc_ver_input = aes_cbc_encrypt(&k_input, &salt, &ver_input).unwrap();
    let enc_ver_val = aes_cbc_encrypt(&k_val, &salt, &ver_hash_padded).unwrap();
    let enc_key_val = aes_cbc_encrypt(&k_key, &salt, &pkg_key).unwrap();

    let (s_b64, v_in, v_val, k_val) = (
        BASE64_STANDARD.encode(salt), BASE64_STANDARD.encode(enc_ver_input),
        BASE64_STANDARD.encode(enc_ver_val), BASE64_STANDARD.encode(enc_key_val),
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\"><keyData saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashAlgorithm=\"SHA512\" cipherAlgorithm=\"AES\" saltValue=\"{0}\"/><keyEncryptors><keyEncryptor uri=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\"><p:encryptedKey xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\" spinCount=\"{1}\" saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashAlgorithm=\"SHA512\" saltValue=\"{0}\" encryptedVerifierHashInput=\"{2}\" encryptedVerifierHashValue=\"{3}\" encryptedKeyValue=\"{4}\"/></keyEncryptor></keyEncryptors></encryption>",
        s_b64, spin_count, v_in, v_val, k_val
    );

    let mut info_bytes = Vec::new();
    info_bytes.extend_from_slice(&4u16.to_le_bytes()); info_bytes.extend_from_slice(&4u16.to_le_bytes()); info_bytes.extend_from_slice(&0x40u32.to_le_bytes());
    info_bytes.extend_from_slice(xml.as_bytes());

    let mut pkg_stream = Vec::new();
    pkg_stream.extend_from_slice(&(docx_bytes.len() as u64).to_le_bytes());
    let (segment_size, num_segments) = (4096, docx_bytes.len().div_ceil(4096));
    for seg_idx in 0..num_segments {
        let (start, end) = (seg_idx * segment_size, (seg_idx * segment_size + segment_size).min(docx_bytes.len()));
        let mut slice = docx_bytes[start..end].to_vec();
        while !slice.len().is_multiple_of(16) { slice.push(0); }
        let mut seg_hash_buf = Vec::with_capacity(salt.len() + 4);
        seg_hash_buf.extend_from_slice(&salt);
        seg_hash_buf.extend_from_slice(&(seg_idx as u32).to_le_bytes());
        let seg_iv_hash = enc_algo.digest(&seg_hash_buf);
        pkg_stream.extend_from_slice(&aes_cbc_encrypt(&pkg_key, &seg_iv_hash[..16], &slice).unwrap());
    }

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut comp = cfb::CompoundFile::create(&mut cursor).unwrap();
        let mut s1 = comp.create_new_stream("/EncryptionInfo").unwrap();
        std::io::Write::write_all(&mut s1, &info_bytes).unwrap();
        let mut s2 = comp.create_new_stream("/EncryptedPackage").unwrap();
        std::io::Write::write_all(&mut s2, &pkg_stream).unwrap();
    }
    cursor.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agile_roundtrip() {
        let docx_content = b"PK\x03\x04Dummy DOCX payload for agile test";
        let password = "SecretPassword123!";
        let cfb_bytes = build_agile_encrypted_cfb_for_test(docx_content, password);

        let mut comp = cfb::CompoundFile::open(Cursor::new(&cfb_bytes)).unwrap();
        let mut info_bytes = Vec::new();
        std::io::Read::read_to_end(&mut comp.open_stream("/EncryptionInfo").unwrap(), &mut info_bytes).unwrap();
        let mut pkg_bytes = Vec::new();
        std::io::Read::read_to_end(&mut comp.open_stream("/EncryptedPackage").unwrap(), &mut pkg_bytes).unwrap();

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
        std::io::Read::read_to_end(&mut comp.open_stream("/EncryptionInfo").unwrap(), &mut info_bytes).unwrap();
        let mut pkg_bytes = Vec::new();
        std::io::Read::read_to_end(&mut comp.open_stream("/EncryptedPackage").unwrap(), &mut pkg_bytes).unwrap();

        let err = decrypt_agile_package(&info_bytes, &pkg_bytes, "pass").unwrap_err();
        assert_eq!(err.kind, ConversionErrorKind::ResourceLimit);
        assert!(err.message.contains("exceeds limit"));
    }
}
