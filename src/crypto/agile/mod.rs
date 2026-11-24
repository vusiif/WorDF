//! ECMA-376 / MS-OFFCRYPTO Agile Encryption parser and decryptor.
//! License: GPL-3.0-or-later
mod builder;
#[cfg(test)]
mod tests;

pub use builder::{
    build_agile_encrypted_cfb_for_test, build_agile_encrypted_cfb_with_spin_for_test,
};

use base64::prelude::*;

use crate::crypto::cipher::{aes_cbc_decrypt, HashAlgorithm};
use crate::{ConversionError, ConversionErrorKind};

pub const BLOCK_KEY_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
pub const BLOCK_KEY_VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
pub const BLOCK_KEY_KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
pub const MAX_SPIN_COUNT: u32 = 1_000_000;

fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
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
    if digest.len() >= key_bytes {
        digest.truncate(key_bytes);
    } else {
        digest.resize(key_bytes, 0x36);
    }
    digest
}

pub fn derive_password_hash(
    password: &str,
    salt: &[u8],
    spin_count: u32,
    algo: HashAlgorithm,
) -> Vec<u8> {
    let pwd_utf16: Vec<u8> = password
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
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

    let (v_major, v_minor) = (
        u16::from_le_bytes([info_bytes[0], info_bytes[1]]),
        u16::from_le_bytes([info_bytes[2], info_bytes[3]]),
    );
    if v_major != 4 || v_minor != 4 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!(
                "Unsupported Office encryption version: {}.{}",
                v_major, v_minor
            ),
        });
    }

    let xml = String::from_utf8_lossy(&info_bytes[8..]);
    let key_data_tag = xml
        .lines()
        .find(|l| l.contains("<keyData") || l.contains("<p:keyData"))
        .unwrap_or("");
    let enc_key_tag = xml
        .lines()
        .find(|l| l.contains("encryptedKey"))
        .unwrap_or("");

    let key_salt = BASE64_STANDARD
        .decode(extract_attr(key_data_tag, "saltValue").unwrap_or_default())
        .map_err(|_| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Invalid base64 in keyData saltValue".to_string(),
        })?;
    let key_algo =
        HashAlgorithm::parse(&extract_attr(key_data_tag, "hashAlgorithm").unwrap_or_default())
            .unwrap_or(HashAlgorithm::Sha512);

    let spin_count: u32 = extract_attr(enc_key_tag, "spinCount")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    if spin_count > MAX_SPIN_COUNT {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "Encryption spinCount {} exceeds limit {}",
                spin_count, MAX_SPIN_COUNT
            ),
        });
    }
    let enc_salt = BASE64_STANDARD
        .decode(extract_attr(enc_key_tag, "saltValue").unwrap_or_default())
        .map_err(|_| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Invalid base64 in encryptedKey saltValue".to_string(),
        })?;
    let enc_algo =
        HashAlgorithm::parse(&extract_attr(enc_key_tag, "hashAlgorithm").unwrap_or_default())
            .unwrap_or(HashAlgorithm::Sha512);
    let key_bytes = extract_attr(enc_key_tag, "keyBits")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(256)
        / 8;
    if key_bytes != 16 && key_bytes != 32 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported encryption key length: {} bits", key_bytes * 8),
        });
    }

    let enc_ver_input = BASE64_STANDARD
        .decode(extract_attr(enc_key_tag, "encryptedVerifierHashInput").unwrap_or_default())
        .unwrap_or_default();
    let enc_ver_val = BASE64_STANDARD
        .decode(extract_attr(enc_key_tag, "encryptedVerifierHashValue").unwrap_or_default())
        .unwrap_or_default();
    let enc_key_val = BASE64_STANDARD
        .decode(extract_attr(enc_key_tag, "encryptedKeyValue").unwrap_or_default())
        .unwrap_or_default();

    let mut iv = [0x36u8; 16];
    let copy_len = enc_salt.len().min(16);
    iv[..copy_len].copy_from_slice(&enc_salt[..copy_len]);

    let mut h = derive_password_hash(password, &enc_salt, spin_count, enc_algo);
    let mut k_input = derive_key(&h, &BLOCK_KEY_VERIFIER_INPUT, key_bytes, enc_algo);
    let mut k_val = derive_key(&h, &BLOCK_KEY_VERIFIER_VALUE, key_bytes, enc_algo);
    let mut k_key = derive_key(&h, &BLOCK_KEY_KEY_VALUE, key_bytes, enc_algo);

    let ver_input =
        aes_cbc_decrypt(&k_input, &iv, &enc_ver_input).map_err(|e| ConversionError {
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
        h.fill(0);
        k_input.fill(0);
        k_val.fill(0);
        k_key.fill(0);
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: "Decryption failed: Incorrect password".to_string(),
        });
    }

    let mut dec_key = aes_cbc_decrypt(&k_key, &iv, &enc_key_val).map_err(|e| ConversionError {
        kind: ConversionErrorKind::Encrypted,
        message: format!("Decryption failed for package key: {}", e),
    })?;
    h.fill(0);
    k_input.fill(0);
    k_val.fill(0);
    k_key.fill(0);
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
            message: format!(
                "Encrypted package size {} exceeds 128 MiB resource limit",
                orig_size
            ),
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
        if seg_cipher.is_empty() {
            break;
        }

        let mut seg_hash_buf = Vec::with_capacity(key_salt.len() + 4);
        seg_hash_buf.extend_from_slice(&key_salt);
        seg_hash_buf.extend_from_slice(&(seg_idx as u32).to_le_bytes());
        let seg_iv_hash = key_algo.digest(&seg_hash_buf);

        let seg_dec =
            aes_cbc_decrypt(package_key, &seg_iv_hash[..16], seg_cipher).map_err(|e| {
                ConversionError {
                    kind: ConversionErrorKind::Encrypted,
                    message: format!("Decryption failed on package segment {}: {}", seg_idx, e),
                }
            })?;
        decrypted_payload.extend_from_slice(&seg_dec);
    }

    dec_key.fill(0);
    let final_len = (orig_size as usize).min(decrypted_payload.len());
    decrypted_payload.truncate(final_len);
    Ok(decrypted_payload)
}
