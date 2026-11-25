//! Versioned incremental conversion fingerprinting and PDF integrity verification.
//! License: GPL-3.0-or-later

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use wordf::ConversionOptions;

pub const RESUME_SIDECAR_EXT: &str = ".wordf-fingerprint";

/// Computes a canonical SHA-256 fingerprint for a conversion task based on input content,
/// engine version, font fallback policies, page range, metadata, strict mode, and salted password.
pub fn compute_conversion_fingerprint(
    input_bytes: &[u8],
    options: &ConversionOptions,
    strict: bool,
) -> String {
    let mut hasher = Sha256::new();

    // 1. Tool version
    hasher.update(b"VERSION:");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

    // 2. Input bytes SHA-256
    let input_hash = Sha256::digest(input_bytes);
    hasher.update(b"|INPUT:");
    hasher.update(input_hash);

    // 3. Format override
    hasher.update(b"|FMT:");
    hasher.update(format!("{:?}", options.format).as_bytes());

    // 4. Primary fallback font name and bytes
    hasher.update(b"|FB_NAME:");
    if let Some(ref name) = options.fallback_font_name {
        hasher.update(name.as_bytes());
    }
    hasher.update(b"|FB_BYTES:");
    if let Some(ref bytes) = options.fallback_font_bytes {
        hasher.update(Sha256::digest(bytes));
    }

    // 5. Available font names (sorted)
    let mut sorted_fonts = options.available_font_names.clone();
    sorted_fonts.sort();
    hasher.update(b"|FONTS:");
    for f in &sorted_fonts {
        hasher.update(f.as_bytes());
        hasher.update(b",");
    }

    // 6. Fallback chain
    hasher.update(b"|CHAIN:");
    if let Some(ref chain) = options.fallback_chain {
        for item in &chain.items {
            hasher.update(item.family_name.as_bytes());
            hasher.update(b":");
            hasher.update(Sha256::digest(item.font_bytes.as_slice()));
            hasher.update(b";");
        }
    }

    // 7. Pages range
    hasher.update(b"|PAGES:");
    if let Some(ref pages) = options.pages {
        hasher.update(format!("{:?}", pages).as_bytes());
    }

    // 8. Salted password hash (never store plain password)
    hasher.update(b"|PWD:");
    if let Some(ref pwd) = options.password {
        let mut pwd_hasher = Sha256::new();
        pwd_hasher.update(b"SALT_WORDF_PWD_V1:");
        pwd_hasher.update(pwd.as_bytes());
        hasher.update(pwd_hasher.finalize());
    }

    // 9. Document metadata
    hasher.update(b"|META:");
    if let Some(ref meta) = options.metadata {
        if let Some(ref t) = meta.title {
            hasher.update(b"T:");
            hasher.update(t.as_bytes());
        }
        if let Some(ref a) = meta.author {
            hasher.update(b"A:");
            hasher.update(a.as_bytes());
        }
        if let Some(ref s) = meta.subject {
            hasher.update(b"S:");
            hasher.update(s.as_bytes());
        }
        if let Some(ref k) = meta.keywords {
            hasher.update(b"K:");
            hasher.update(k.as_bytes());
        }
    }

    // 10. Strict mode
    hasher.update(b"|STRICT:");
    hasher.update(if strict { b"1" } else { b"0" });

    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in result {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    hex
}

/// Computes the sidecar file path for an output PDF destination.
pub fn sidecar_path(pdf_path: &Path) -> PathBuf {
    let mut s = pdf_path.as_os_str().to_os_string();
    s.push(RESUME_SIDECAR_EXT);
    PathBuf::from(s)
}

/// Verifies that `path` is an existing, non-corrupt PDF file.
/// Validates:
/// 1. File exists and is regular file.
/// 2. Size >= 32 bytes.
/// 3. Starts with b"%PDF-".
/// 4. Within the last 1024 bytes (or whole file), contains b"%%EOF".
pub fn verify_output_pdf_integrity(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() < 32 {
        return false;
    }

    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };

    // Check header
    let mut header = [0u8; 5];
    if file.read_exact(&mut header).is_err() || &header != b"%PDF-" {
        return false;
    }

    // Check trailer: last 1024 bytes
    let total_len = meta.len();
    let tail_read_len = (1024u64).min(total_len) as usize;
    let seek_pos = total_len.saturating_sub(tail_read_len as u64);

    if file.seek(SeekFrom::Start(seek_pos)).is_err() {
        return false;
    }

    let mut tail_buf = vec![0u8; tail_read_len];
    if file.read_exact(&mut tail_buf).is_err() {
        return false;
    }

    tail_buf.windows(5).any(|w| w == b"%%EOF")
}

/// Checks whether an existing output PDF qualifies for skipping under `--resume`.
/// Validates both physical PDF integrity and fingerprint equality.
pub fn check_resume_match(
    input_bytes: &[u8],
    out_pdf: &Path,
    options: &ConversionOptions,
    strict: bool,
) -> bool {
    if !verify_output_pdf_integrity(out_pdf) {
        return false;
    }

    let expected_fp = compute_conversion_fingerprint(input_bytes, options, strict);
    let sidecar = sidecar_path(out_pdf);

    if let Ok(recorded_fp) = fs::read_to_string(&sidecar) {
        if recorded_fp.trim() == expected_fp {
            return true;
        }
    }

    false
}

/// Writes a fingerprint sidecar file adjacent to the committed output PDF.
pub fn write_resume_fingerprint(out_pdf: &Path, fingerprint: &str) -> std::io::Result<()> {
    let sidecar = sidecar_path(out_pdf);
    fs::write(&sidecar, fingerprint.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_fingerprint_deterministic_and_sensitive() {
        let input = b"Hello WorDF Test";
        let opts1 = ConversionOptions::default();
        let fp1 = compute_conversion_fingerprint(input, &opts1, false);
        let fp2 = compute_conversion_fingerprint(input, &opts1, false);
        assert_eq!(fp1, fp2);

        // Strict change affects fingerprint
        let fp_strict = compute_conversion_fingerprint(input, &opts1, true);
        assert_ne!(fp1, fp_strict);

        // Page change affects fingerprint
        let mut opts_pages = opts1.clone();
        opts_pages.pages = Some(vec![0, 1]);
        let fp_pages = compute_conversion_fingerprint(input, &opts_pages, false);
        assert_ne!(fp1, fp_pages);

        // Font change affects fingerprint
        let mut opts_font = opts1.clone();
        opts_font.fallback_font_name = Some("CustomFont".to_string());
        let fp_font = compute_conversion_fingerprint(input, &opts_font, false);
        assert_ne!(fp1, fp_font);
    }
}
