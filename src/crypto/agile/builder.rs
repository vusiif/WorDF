//! Test helper builders for MS-OFFCRYPTO Agile encrypted packages.
//! License: GPL-3.0-or-later

use base64::prelude::*;
use std::io::Cursor;

use super::{
    derive_key, derive_password_hash, BLOCK_KEY_DATA_INTEGRITY_HMAC_KEY,
    BLOCK_KEY_DATA_INTEGRITY_HMAC_VALUE, BLOCK_KEY_KEY_VALUE, BLOCK_KEY_VERIFIER_INPUT,
    BLOCK_KEY_VERIFIER_VALUE,
};
use crate::crypto::cipher::{aes_cbc_encrypt, HashAlgorithm};

/// Helper for constructing standard MS-OFFCRYPTO Agile encrypted CFB package in tests.
pub fn build_agile_encrypted_cfb_for_test(docx_bytes: &[u8], password: &str) -> Vec<u8> {
    build_agile_encrypted_cfb_with_spin_for_test(docx_bytes, password, 1_000)
}

pub fn build_agile_encrypted_cfb_with_spin_for_test(
    docx_bytes: &[u8],
    password: &str,
    spin_count: u32,
) -> Vec<u8> {
    let (salt, enc_algo, key_bytes) = ([0x7au8; 16], HashAlgorithm::Sha512, 32usize);
    let h = derive_password_hash(password, &salt, spin_count, enc_algo);
    let k_input = derive_key(&h, &BLOCK_KEY_VERIFIER_INPUT, key_bytes, enc_algo);
    let k_val = derive_key(&h, &BLOCK_KEY_VERIFIER_VALUE, key_bytes, enc_algo);
    let k_key = derive_key(&h, &BLOCK_KEY_KEY_VALUE, key_bytes, enc_algo);

    let ver_input = [0x55u8; 16];
    let mut ver_hash_padded = enc_algo.digest(&ver_input);
    while !ver_hash_padded.len().is_multiple_of(16) {
        ver_hash_padded.push(0);
    }

    let pkg_key = [0x33u8; 32];
    let enc_ver_input = aes_cbc_encrypt(&k_input, &salt, &ver_input).unwrap();
    let enc_ver_val = aes_cbc_encrypt(&k_val, &salt, &ver_hash_padded).unwrap();
    let enc_key_val = aes_cbc_encrypt(&k_key, &salt, &pkg_key).unwrap();

    let mut pkg_stream = Vec::new();
    pkg_stream.extend_from_slice(&(docx_bytes.len() as u64).to_le_bytes());
    let (segment_size, num_segments) = (4096, docx_bytes.len().div_ceil(4096));
    for seg_idx in 0..num_segments {
        let (start, end) = (
            seg_idx * segment_size,
            (seg_idx * segment_size + segment_size).min(docx_bytes.len()),
        );
        let mut slice = docx_bytes[start..end].to_vec();
        while !slice.len().is_multiple_of(16) {
            slice.push(0);
        }
        let mut seg_hash_buf = Vec::with_capacity(salt.len() + 4);
        seg_hash_buf.extend_from_slice(&salt);
        seg_hash_buf.extend_from_slice(&(seg_idx as u32).to_le_bytes());
        let seg_iv_hash = enc_algo.digest(&seg_hash_buf);
        pkg_stream
            .extend_from_slice(&aes_cbc_encrypt(&pkg_key, &seg_iv_hash[..16], &slice).unwrap());
    }

    let hmac_salt = [0x5au8; 64];
    let mut iv1_buf = Vec::new();
    iv1_buf.extend_from_slice(&salt);
    iv1_buf.extend_from_slice(&BLOCK_KEY_DATA_INTEGRITY_HMAC_KEY);
    let iv1 = enc_algo.digest(&iv1_buf);
    let enc_hmac_key = aes_cbc_encrypt(&pkg_key, &iv1[..16], &hmac_salt).unwrap();

    let computed_hmac = enc_algo.hmac(&hmac_salt, &pkg_stream);
    let mut iv2_buf = Vec::new();
    iv2_buf.extend_from_slice(&salt);
    iv2_buf.extend_from_slice(&BLOCK_KEY_DATA_INTEGRITY_HMAC_VALUE);
    let iv2 = enc_algo.digest(&iv2_buf);
    let enc_hmac_val = aes_cbc_encrypt(&pkg_key, &iv2[..16], &computed_hmac).unwrap();

    let (s_b64, v_in, v_val, k_val, h_key, h_val) = (
        BASE64_STANDARD.encode(salt),
        BASE64_STANDARD.encode(enc_ver_input),
        BASE64_STANDARD.encode(enc_ver_val),
        BASE64_STANDARD.encode(enc_key_val),
        BASE64_STANDARD.encode(enc_hmac_key),
        BASE64_STANDARD.encode(enc_hmac_val),
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\"><keyData saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashAlgorithm=\"SHA512\" cipherAlgorithm=\"AES\" saltValue=\"{0}\"/><dataIntegrity encryptedHmacKey=\"{4}\" encryptedHmacValue=\"{5}\"/><keyEncryptors><keyEncryptor uri=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\"><p:encryptedKey xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\" spinCount=\"{1}\" saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashAlgorithm=\"SHA512\" saltValue=\"{0}\" encryptedVerifierHashInput=\"{2}\" encryptedVerifierHashValue=\"{3}\" encryptedKeyValue=\"{6}\"/></keyEncryptor></keyEncryptors></encryption>",
        s_b64, spin_count, v_in, v_val, h_key, h_val, k_val
    );

    let mut info_bytes = Vec::new();
    info_bytes.extend_from_slice(&4u16.to_le_bytes());
    info_bytes.extend_from_slice(&4u16.to_le_bytes());
    info_bytes.extend_from_slice(&0x40u32.to_le_bytes());
    info_bytes.extend_from_slice(xml.as_bytes());

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
