//! ECMA-376 / MS-OFFCRYPTO Agile Encryption XML parser.
//! License: GPL-3.0-or-later

use base64::prelude::*;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::crypto::cipher::HashAlgorithm;
use crate::{ConversionError, ConversionErrorKind};

pub const MAX_SPIN_COUNT: u32 = 1_000_000;
pub const PASSWORD_URI: &str = "http://schemas.microsoft.com/office/2006/keyEncryptor/password";

#[derive(Debug, Clone)]
pub struct AgileKeyData {
    pub salt_value: Vec<u8>,
    pub hash_algo: HashAlgorithm,
    pub key_bytes: usize,
    pub block_size: usize,
}

#[derive(Debug, Clone)]
pub struct AgileDataIntegrity {
    pub encrypted_hmac_key: Vec<u8>,
    pub encrypted_hmac_value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AgilePasswordEncryptor {
    pub spin_count: u32,
    pub salt_value: Vec<u8>,
    pub hash_algo: HashAlgorithm,
    pub key_bytes: usize,
    pub encrypted_verifier_hash_input: Vec<u8>,
    pub encrypted_verifier_hash_value: Vec<u8>,
    pub encrypted_key_value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AgileEncryptionInfo {
    pub key_data: AgileKeyData,
    pub data_integrity: Option<AgileDataIntegrity>,
    pub password_encryptor: AgilePasswordEncryptor,
}

fn b64(val: &str, field: &str) -> Result<Vec<u8>, ConversionError> {
    BASE64_STANDARD
        .decode(val.trim())
        .map_err(|_| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Invalid base64 in attribute '{}'", field),
        })
}

fn parse_algo(val: &str) -> Result<HashAlgorithm, ConversionError> {
    HashAlgorithm::parse(val).ok_or_else(|| ConversionError {
        kind: ConversionErrorKind::Encrypted,
        message: format!("Unsupported encryption hashAlgorithm: '{}'", val),
    })
}

fn check_cipher(cipher: &str, chaining: &str) -> Result<(), ConversionError> {
    if !cipher.is_empty() && !cipher.eq_ignore_ascii_case("AES") {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported encryption cipherAlgorithm: '{}'", cipher),
        });
    }
    if !chaining.is_empty() && !chaining.eq_ignore_ascii_case("ChainingModeCBC") {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported cipherChaining mode: '{}'", chaining),
        });
    }
    Ok(())
}

fn attr_val<'a>(
    attr: &'a quick_xml::events::attributes::Attribute<'a>,
) -> std::borrow::Cow<'a, str> {
    let s = std::str::from_utf8(&attr.value).unwrap_or_default();
    quick_xml::escape::unescape(s).unwrap_or(std::borrow::Cow::Borrowed(s))
}

fn parse_key_data(e: &quick_xml::events::BytesStart) -> Result<AgileKeyData, ConversionError> {
    let (mut salt, mut algo, mut bits, mut blk, mut cipher, mut chain) =
        (None, None, 256usize, 16usize, String::new(), String::new());
    for attr in e.attributes().flatten() {
        let (k, v) = (attr.key.local_name(), attr_val(&attr));
        match k.as_ref() {
            b"saltValue" => salt = Some(b64(&v, "keyData.saltValue")?),
            b"hashAlgorithm" => algo = Some(parse_algo(&v)?),
            b"keyBits" => bits = v.parse().unwrap_or(256),
            b"blockSize" => blk = v.parse().unwrap_or(16),
            b"cipherAlgorithm" => cipher = v.into_owned(),
            b"cipherChaining" => chain = v.into_owned(),
            _ => {}
        }
    }
    check_cipher(&cipher, &chain)?;
    let key_bytes = bits / 8;
    if key_bytes != 16 && key_bytes != 32 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported keyData keyBits: {} bits", bits),
        });
    }
    Ok(AgileKeyData {
        salt_value: salt.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'saltValue' in keyData element".to_string(),
        })?,
        hash_algo: algo.unwrap_or(HashAlgorithm::Sha512),
        key_bytes,
        block_size: blk,
    })
}

fn parse_integrity(
    e: &quick_xml::events::BytesStart,
) -> Result<Option<AgileDataIntegrity>, ConversionError> {
    let (mut k, mut v) = (None, None);
    for attr in e.attributes().flatten() {
        let (key, val) = (attr.key.local_name(), attr_val(&attr));
        match key.as_ref() {
            b"encryptedHmacKey" => k = Some(b64(&val, "encryptedHmacKey")?),
            b"encryptedHmacValue" => v = Some(b64(&val, "encryptedHmacValue")?),
            _ => {}
        }
    }
    if let (Some(k), Some(v)) = (k, v) {
        Ok(Some(AgileDataIntegrity {
            encrypted_hmac_key: k,
            encrypted_hmac_value: v,
        }))
    } else {
        Ok(None)
    }
}

fn parse_enc_key(
    e: &quick_xml::events::BytesStart,
) -> Result<AgilePasswordEncryptor, ConversionError> {
    let (mut spin, mut salt, mut algo, mut bits) = (100_000u32, None, None, 256usize);
    let (mut v_in, mut v_val, mut k_val) = (None, None, None);
    let (mut cipher, mut chain) = (String::new(), String::new());
    for attr in e.attributes().flatten() {
        let (k, v) = (attr.key.local_name(), attr_val(&attr));
        match k.as_ref() {
            b"spinCount" => spin = v.parse().unwrap_or(100_000),
            b"saltValue" => salt = Some(b64(&v, "encryptedKey.saltValue")?),
            b"hashAlgorithm" => algo = Some(parse_algo(&v)?),
            b"keyBits" => bits = v.parse().unwrap_or(256),
            b"encryptedVerifierHashInput" => v_in = Some(b64(&v, "encryptedVerifierHashInput")?),
            b"encryptedVerifierHashValue" => v_val = Some(b64(&v, "encryptedVerifierHashValue")?),
            b"encryptedKeyValue" => k_val = Some(b64(&v, "encryptedKeyValue")?),
            b"cipherAlgorithm" => cipher = v.into_owned(),
            b"cipherChaining" => chain = v.into_owned(),
            _ => {}
        }
    }
    if spin > MAX_SPIN_COUNT {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "Encryption spinCount {} exceeds limit {}",
                spin, MAX_SPIN_COUNT
            ),
        });
    }
    check_cipher(&cipher, &chain)?;
    let key_bytes = bits / 8;
    if key_bytes != 16 && key_bytes != 32 {
        return Err(ConversionError {
            kind: ConversionErrorKind::Encrypted,
            message: format!("Unsupported encryptedKey keyBits: {} bits", bits),
        });
    }
    Ok(AgilePasswordEncryptor {
        spin_count: spin,
        salt_value: salt.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'saltValue' in encryptedKey element".to_string(),
        })?,
        hash_algo: algo.unwrap_or(HashAlgorithm::Sha512),
        key_bytes,
        encrypted_verifier_hash_input: v_in.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'encryptedVerifierHashInput' in encryptedKey element".to_string(),
        })?,
        encrypted_verifier_hash_value: v_val.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'encryptedVerifierHashValue' in encryptedKey element".to_string(),
        })?,
        encrypted_key_value: k_val.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing 'encryptedKeyValue' in encryptedKey element".to_string(),
        })?,
    })
}

pub fn parse_agile_xml(xml: &str) -> Result<AgileEncryptionInfo, ConversionError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let (mut key_data, mut integrity, mut password_enc) = (None, None, None);
    let mut in_pwd_encryptor = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => match e.local_name().as_ref() {
                b"keyData" => key_data = Some(parse_key_data(e)?),
                b"dataIntegrity" => integrity = parse_integrity(e)?,
                b"keyEncryptor" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"uri"
                            && attr_val(&attr) == PASSWORD_URI
                        {
                            in_pwd_encryptor = true;
                        }
                    }
                }
                b"encryptedKey" if in_pwd_encryptor || password_enc.is_none() => {
                    password_enc = Some(parse_enc_key(e)?);
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref() == b"keyEncryptor" {
                    in_pwd_encryptor = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ConversionError {
                    kind: ConversionErrorKind::CorruptInput,
                    message: format!("Malformed Agile encryption XML: {}", e),
                });
            }
            _ => {}
        }
    }

    Ok(AgileEncryptionInfo {
        key_data: key_data.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing '<keyData>' element in EncryptionInfo XML".to_string(),
        })?,
        data_integrity: integrity,
        password_encryptor: password_enc.ok_or_else(|| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: "Missing password '<encryptedKey>' element in EncryptionInfo XML".to_string(),
        })?,
    })
}
