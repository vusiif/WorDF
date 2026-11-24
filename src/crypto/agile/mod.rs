//! ECMA-376 / MS-OFFCRYPTO Agile Encryption parser and decryptor.
//! License: GPL-3.0-or-later

mod builder;
pub mod kdf;
pub mod parser;
#[cfg(test)]
mod tests;

pub use builder::{
    build_agile_encrypted_cfb_for_test, build_agile_encrypted_cfb_with_spin_for_test,
};
pub use kdf::*;
pub use parser::MAX_SPIN_COUNT;

use crate::crypto::cipher::aes_cbc_decrypt;
use crate::{ConversionError, ConversionErrorKind};

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

    let xml = std::str::from_utf8(&info_bytes[8..]).map_err(|_| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: "EncryptionInfo XML contains invalid UTF-8 bytes".to_string(),
    })?;
    let info = parser::parse_agile_xml(xml)?;
    let p_enc = &info.password_encryptor;

    let mut iv = [0x36u8; 16];
    let copy_len = p_enc.salt_value.len().min(16);
    iv[..copy_len].copy_from_slice(&p_enc.salt_value[..copy_len]);

    let h = ZeroizeBuffer(derive_password_hash(
        password,
        &p_enc.salt_value,
        p_enc.spin_count,
        p_enc.hash_algo,
    ));
    let k_input = ZeroizeBuffer(derive_key(
        &h,
        &BLOCK_KEY_VERIFIER_INPUT,
        p_enc.key_bytes,
        p_enc.hash_algo,
    ));
    let k_val = ZeroizeBuffer(derive_key(
        &h,
        &BLOCK_KEY_VERIFIER_VALUE,
        p_enc.key_bytes,
        p_enc.hash_algo,
    ));
    let k_key = ZeroizeBuffer(derive_key(
        &h,
        &BLOCK_KEY_KEY_VALUE,
        p_enc.key_bytes,
        p_enc.hash_algo,
    ));

    let ver_input =
        aes_cbc_decrypt(&k_input, &iv, &p_enc.encrypted_verifier_hash_input).map_err(|e| {
            ConversionError {
                kind: ConversionErrorKind::Encrypted,
                message: format!("Decryption failed for verifier input: {}", e),
            }
        })?;
    let ver_val =
        aes_cbc_decrypt(&k_val, &iv, &p_enc.encrypted_verifier_hash_value).map_err(|e| {
            ConversionError {
                kind: ConversionErrorKind::Encrypted,
                message: format!("Decryption failed for verifier value: {}", e),
            }
        })?;

    let expected_hash = p_enc.hash_algo.digest(&ver_input);
    if ver_val.len() < expected_hash.len()
        || !constant_time_compare(&ver_val[..expected_hash.len()], expected_hash.as_slice())
    {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: "Decryption failed: Incorrect password".to_string(),
        });
    }

    let dec_key = ZeroizeBuffer(
        aes_cbc_decrypt(&k_key, &iv, &p_enc.encrypted_key_value).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Decryption failed for package key: {}", e),
        })?,
    );

    if dec_key.len() < p_enc.key_bytes {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Decrypted package key buffer shorter than declared key size".to_string(),
        });
    }
    let package_key = &dec_key[..p_enc.key_bytes];

    if pkg_bytes.len() < 8 {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "EncryptedPackage stream is truncated".to_string(),
        });
    }

    let orig_size = u64::from_le_bytes(pkg_bytes[..8].try_into().unwrap());
    if orig_size > crate::format::MAX_INPUT_BYTES as u64 {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "Encrypted package size {} exceeds 128 MiB resource limit",
                orig_size
            ),
        });
    }

    if let Some(ref integrity) = info.data_integrity {
        let mut iv1_buf = Vec::with_capacity(info.key_data.salt_value.len() + 8);
        iv1_buf.extend_from_slice(&info.key_data.salt_value);
        iv1_buf.extend_from_slice(&BLOCK_KEY_DATA_INTEGRITY_HMAC_KEY);
        let iv1_hash = info.key_data.hash_algo.digest(&iv1_buf);

        let hmac_key_buf = ZeroizeBuffer(
            aes_cbc_decrypt(package_key, &iv1_hash[..16], &integrity.encrypted_hmac_key).map_err(
                |e| ConversionError {
                    kind: ConversionErrorKind::CorruptInput,
                    message: format!("Decryption failed for integrity HMAC key: {}", e),
                },
            )?,
        );

        let mut iv2_buf = Vec::with_capacity(info.key_data.salt_value.len() + 8);
        iv2_buf.extend_from_slice(&info.key_data.salt_value);
        iv2_buf.extend_from_slice(&BLOCK_KEY_DATA_INTEGRITY_HMAC_VALUE);
        let iv2_hash = info.key_data.hash_algo.digest(&iv2_buf);

        let exp_hmac_buf = ZeroizeBuffer(
            aes_cbc_decrypt(
                package_key,
                &iv2_hash[..16],
                &integrity.encrypted_hmac_value,
            )
            .map_err(|e| ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: format!("Decryption failed for integrity HMAC value: {}", e),
            })?,
        );

        let dig_len = info.key_data.hash_algo.digest_size();
        let hmac_key = &hmac_key_buf[..dig_len.min(hmac_key_buf.len())];
        let computed = info.key_data.hash_algo.hmac(hmac_key, pkg_bytes);
        let expected = &exp_hmac_buf[..dig_len.min(exp_hmac_buf.len())];

        if !constant_time_compare(&computed, expected) {
            return Err(ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: "Agile encryption data integrity check failed: HMAC mismatch".to_string(),
            });
        }
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

        let mut seg_hash_buf = Vec::with_capacity(info.key_data.salt_value.len() + 4);
        seg_hash_buf.extend_from_slice(&info.key_data.salt_value);
        seg_hash_buf.extend_from_slice(&(seg_idx as u32).to_le_bytes());
        let seg_iv_hash = info.key_data.hash_algo.digest(&seg_hash_buf);

        let seg_dec =
            aes_cbc_decrypt(package_key, &seg_iv_hash[..16], seg_cipher).map_err(|e| {
                ConversionError {
                    kind: ConversionErrorKind::Encrypted,
                    message: format!("Decryption failed on package segment {}: {}", seg_idx, e),
                }
            })?;
        decrypted_payload.extend_from_slice(&seg_dec);
    }

    if decrypted_payload.len() < orig_size as usize {
        return Err(ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!(
                "Decrypted package payload length ({} bytes) is less than declared stream size ({} bytes)",
                decrypted_payload.len(),
                orig_size
            ),
        });
    }
    decrypted_payload.truncate(orig_size as usize);
    Ok(decrypted_payload)
}
