//! Key derivation functions and constant-time helpers for Agile encryption.
//! License: GPL-3.0-or-later

use crate::crypto::cipher::HashAlgorithm;

pub const BLOCK_KEY_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
pub const BLOCK_KEY_VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
pub const BLOCK_KEY_KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
pub const BLOCK_KEY_DATA_INTEGRITY_HMAC_KEY: [u8; 8] =
    [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
pub const BLOCK_KEY_DATA_INTEGRITY_HMAC_VALUE: [u8; 8] =
    [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// RAII buffer that clears sensitive cryptographic material on drop.
pub struct ZeroizeBuffer(pub Vec<u8>);

impl Drop for ZeroizeBuffer {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl std::ops::Deref for ZeroizeBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn derive_key(h: &[u8], block_key: &[u8], key_bytes: usize, algo: HashAlgorithm) -> Vec<u8> {
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
