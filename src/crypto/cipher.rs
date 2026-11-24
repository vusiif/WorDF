//! AES-CBC and cryptographic hash primitives for Office document decryption.
//! License: GPL-3.0-or-later

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes::{Aes128, Aes256, Block};
use sha1::Digest;
use sha2::Digest as Sha2Digest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha512,
    Sha256,
    Sha1,
}

impl HashAlgorithm {
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        if upper.contains("SHA512") {
            Some(Self::Sha512)
        } else if upper.contains("SHA256") {
            Some(Self::Sha256)
        } else if upper.contains("SHA1") {
            Some(Self::Sha1)
        } else {
            None
        }
    }

    pub fn digest(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha512 => {
                let mut hasher = sha2::Sha512::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
            Self::Sha256 => {
                let mut hasher = sha2::Sha256::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
            Self::Sha1 => {
                let mut hasher = sha1::Sha1::new();
                Digest::update(&mut hasher, data);
                Digest::finalize(hasher).to_vec()
            }
        }
    }

    pub fn block_size(&self) -> usize {
        match self {
            Self::Sha512 => 128,
            Self::Sha256 | Self::Sha1 => 64,
        }
    }

    pub fn digest_size(&self) -> usize {
        match self {
            Self::Sha512 => 64,
            Self::Sha256 => 32,
            Self::Sha1 => 20,
        }
    }

    /// RFC 2104 standard HMAC computation.
    pub fn hmac(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        let block_len = self.block_size();
        let mut k = if key.len() > block_len {
            self.digest(key)
        } else {
            key.to_vec()
        };
        k.resize(block_len, 0x00);

        let mut i_key_pad = vec![0x36u8; block_len];
        let mut o_key_pad = vec![0x5cu8; block_len];
        for i in 0..block_len {
            i_key_pad[i] ^= k[i];
            o_key_pad[i] ^= k[i];
        }
        k.fill(0);

        let mut inner_input = Vec::with_capacity(block_len + data.len());
        inner_input.extend_from_slice(&i_key_pad);
        inner_input.extend_from_slice(data);
        let inner_hash = self.digest(&inner_input);

        let mut outer_input = Vec::with_capacity(block_len + inner_hash.len());
        outer_input.extend_from_slice(&o_key_pad);
        outer_input.extend_from_slice(&inner_hash);
        self.digest(&outer_input)
    }
}

/// AES-CBC decryption for 128-bit and 256-bit keys.
pub fn aes_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, &'static str> {
    if iv.len() != 16 {
        return Err("IV must be exactly 16 bytes");
    }
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err("Ciphertext length must be a non-zero multiple of 16 bytes");
    }

    let mut output = ciphertext.to_vec();
    let num_blocks = ciphertext.len() / 16;

    if key.len() == 32 {
        let cipher = Aes256::new_from_slice(key).map_err(|_| "Invalid 256-bit AES key")?;
        let mut prev_block = [0u8; 16];
        prev_block.copy_from_slice(iv);

        for i in 0..num_blocks {
            let start = i * 16;
            let end = start + 16;
            let mut curr_cipher = [0u8; 16];
            curr_cipher.copy_from_slice(&ciphertext[start..end]);

            #[allow(deprecated)]
            let block = Block::from_mut_slice(&mut output[start..end]);
            cipher.decrypt_block(block);

            for j in 0..16 {
                output[start + j] ^= prev_block[j];
            }
            prev_block = curr_cipher;
        }
    } else if key.len() == 16 {
        let cipher = Aes128::new_from_slice(key).map_err(|_| "Invalid 128-bit AES key")?;
        let mut prev_block = [0u8; 16];
        prev_block.copy_from_slice(iv);

        for i in 0..num_blocks {
            let start = i * 16;
            let end = start + 16;
            let mut curr_cipher = [0u8; 16];
            curr_cipher.copy_from_slice(&ciphertext[start..end]);

            #[allow(deprecated)]
            let block = Block::from_mut_slice(&mut output[start..end]);
            cipher.decrypt_block(block);

            for j in 0..16 {
                output[start + j] ^= prev_block[j];
            }
            prev_block = curr_cipher;
        }
    } else {
        return Err("Key length must be 16 or 32 bytes");
    }

    Ok(output)
}

/// AES-CBC encryption for 128-bit and 256-bit keys (used for test package generation).
pub fn aes_cbc_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    if iv.len() != 16 {
        return Err("IV must be exactly 16 bytes");
    }
    if plaintext.is_empty() || !plaintext.len().is_multiple_of(16) {
        return Err("Plaintext length must be a non-zero multiple of 16 bytes");
    }

    let mut output = plaintext.to_vec();
    let num_blocks = plaintext.len() / 16;

    if key.len() == 32 {
        let cipher = Aes256::new_from_slice(key).map_err(|_| "Invalid 256-bit AES key")?;
        let mut prev_block = [0u8; 16];
        prev_block.copy_from_slice(iv);

        for i in 0..num_blocks {
            let start = i * 16;
            let end = start + 16;
            for j in 0..16 {
                output[start + j] ^= prev_block[j];
            }

            #[allow(deprecated)]
            let block = Block::from_mut_slice(&mut output[start..end]);
            cipher.encrypt_block(block);
            prev_block.copy_from_slice(&output[start..end]);
        }
    } else if key.len() == 16 {
        let cipher = Aes128::new_from_slice(key).map_err(|_| "Invalid 128-bit AES key")?;
        let mut prev_block = [0u8; 16];
        prev_block.copy_from_slice(iv);

        for i in 0..num_blocks {
            let start = i * 16;
            let end = start + 16;
            for j in 0..16 {
                output[start + j] ^= prev_block[j];
            }

            #[allow(deprecated)]
            let block = Block::from_mut_slice(&mut output[start..end]);
            cipher.encrypt_block(block);
            prev_block.copy_from_slice(&output[start..end]);
        }
    } else {
        return Err("Key length must be 16 or 32 bytes");
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_cbc_256_roundtrip() {
        let key = [0x42u8; 32];
        let iv = [0x13u8; 16];
        let plaintext = b"Hello, world! This is a secret block 1234567890."; // 48 bytes (multiple of 16)
        let encrypted = aes_cbc_encrypt(&key, &iv, plaintext).unwrap();
        assert_ne!(&encrypted[..], plaintext);
        let decrypted = aes_cbc_decrypt(&key, &iv, &encrypted).unwrap();
        assert_eq!(&decrypted[..], plaintext);
    }

    #[test]
    fn test_aes_cbc_128_roundtrip() {
        let key = [0x99u8; 16];
        let iv = [0x77u8; 16];
        let plaintext = b"Block length 16!";
        let encrypted = aes_cbc_encrypt(&key, &iv, plaintext).unwrap();
        let decrypted = aes_cbc_decrypt(&key, &iv, &encrypted).unwrap();
        assert_eq!(&decrypted[..], plaintext);
    }

    #[test]
    fn test_hmac_sha256_rfc4231() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let hmac = HashAlgorithm::Sha256.hmac(&key, data);
        let expected = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(hmac.as_slice(), &expected);
    }

    #[test]
    fn test_hmac_sha1_rfc2202() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let hmac = HashAlgorithm::Sha1.hmac(&key, data);
        let expected = [
            0xb6, 0x17, 0x31, 0x86, 0x55, 0x05, 0x72, 0x64, 0xe2, 0x8b, 0xc0, 0xb6, 0xfb, 0x37,
            0x8c, 0x8e, 0xf1, 0x46, 0xbe, 0x00,
        ];
        assert_eq!(hmac.as_slice(), &expected);
    }
}
