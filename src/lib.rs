//! WorDF: Independent DOC / DOCX to PDF converter in Rust
//! License: GPL-3.0-or-later

pub mod analyze;
pub mod backend;
pub mod compat;
pub mod crypto;
pub mod docx_adapt;
pub mod font;
pub mod format;
pub mod metadata;
pub mod pages;
pub mod report;
pub mod transaction;

use std::error::Error;
use std::fmt;

pub use analyze::{analyze_document, DocumentAnalysis};
pub use compat::*;
pub use font::{
    count_cjk_chars, embedded_cjk_font_bytes, embedded_cjk_font_names, is_cjk_char,
    FallbackFontItem, FontCmap, FontFallbackChain, Format12Group, Format4Subtable,
    EMBEDDED_CJK_FONT_BYTES, EMBEDDED_CJK_FONT_NAME, EMBEDDED_CJK_FONT_NAME_ALT,
};
pub use metadata::DocumentMetadata;
pub use pages::PageRange;

/// Input format explicit declaration or automatic detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputFormat {
    /// Detect format from container magic and structural inspection (default).
    #[default]
    Auto,
    /// Explicitly expect binary Word 97-2003 DOC.
    Doc,
    /// Explicitly expect OOXML DOCX.
    Docx,
}

/// Conversion options for the pure-memory byte API.
#[derive(Clone)]
pub struct ConversionOptions {
    /// Expected input format or Auto.
    pub format: InputFormat,
    /// Optional explicit CJK fallback font family name.
    pub fallback_font_name: Option<String>,
    /// Optional explicit CJK fallback font file bytes (for cmap pre-flight).
    pub fallback_font_bytes: Option<Vec<u8>>,
    /// System or environment available font family names.
    pub available_font_names: Vec<String>,
    /// Optional multi-tiered CJK fallback font chain for cascading glyph resolution.
    pub fallback_chain: Option<FontFallbackChain>,
    /// Optional 0-based sorted, deduplicated page indices to export (e.g. from --pages).
    pub pages: Option<Vec<usize>>,
    /// Optional decryption password for password-protected documents.
    pub password: Option<String>,
    /// Optional document metadata overrides (Title, Author, Subject, Keywords) to inject into PDF.
    pub metadata: Option<DocumentMetadata>,
}

impl fmt::Debug for ConversionOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConversionOptions")
            .field("format", &self.format)
            .field("fallback_font_name", &self.fallback_font_name)
            .field(
                "fallback_font_bytes",
                &self.fallback_font_bytes.as_ref().map(|b| b.len()),
            )
            .field("available_font_names", &self.available_font_names)
            .field("fallback_chain", &self.fallback_chain)
            .field("pages", &self.pages)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            format: InputFormat::Auto,
            fallback_font_name: None,
            fallback_font_bytes: None,
            available_font_names: Vec::new(),
            fallback_chain: None,
            pages: None,
            password: None,
            metadata: None,
        }
    }
}

impl ConversionOptions {
    /// Returns the unified `FontResolver` configured from these options.
    pub fn font_resolver(&self) -> font::FontResolver {
        let embedded_bytes = font::embedded_cjk_font_bytes();
        let fallback_name = self.fallback_font_name.clone().or_else(|| {
            if !embedded_bytes.is_empty() {
                Some(font::EMBEDDED_CJK_FONT_NAME.to_string())
            } else {
                None
            }
        });
        let fallback_bytes: Option<std::borrow::Cow<'static, [u8]>> = match self
            .fallback_font_bytes
            .as_ref()
        {
            Some(b) => Some(std::borrow::Cow::Owned(b.clone())),
            None if !embedded_bytes.is_empty() => Some(std::borrow::Cow::Borrowed(embedded_bytes)),
            None => None,
        };

        let mut available = self.available_font_names.clone();
        for name in font::embedded_cjk_font_names() {
            if !available.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                available.push(name);
            }
        }
        if let Some(ref chain) = self.fallback_chain {
            for name in chain.all_font_names() {
                if !available.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    available.push(name);
                }
            }
        }

        font::FontResolver::from_cow(
            self.fallback_chain.clone().unwrap_or_default(),
            available,
            fallback_name,
            fallback_bytes,
        )
    }
}

/// Stable diagnostic warning entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable machine diagnostic code (e.g. "layout.approximate", "font.fallback").
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// Conversion output structure.
#[derive(Debug, Clone)]
pub struct ConversionOutput {
    /// Complete, valid PDF bytes.
    pub pdf: Vec<u8>,
    /// Real positive page count from layout assembly (1..=u32::MAX).
    pub page_count: u32,
    /// Diagnostic warnings.
    pub warnings: Vec<Diagnostic>,
}

/// Strongly typed conversion error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionErrorKind {
    /// Unsupported format (e.g. RTF/HTML/plain text/non-Word containers).
    UnsupportedFormat,
    /// Unsupported Word version or legacy format.
    UnsupportedVersion,
    /// Password-protected or encrypted document.
    Encrypted,
    /// Corrupted or illegal Word container/stream.
    CorruptInput,
    /// Defensible resource budget exceeded (size, entries, uncompressed bytes, pixels).
    ResourceLimit,
    /// Missing font or font lacks required glyphs (fail-fast).
    MissingFont,
    /// Internal backend render failure or zero-page PDF.
    BackendFailure,
    /// Requested unsupported capability.
    UnsupportedCapability,
    /// Requested page out of range.
    PageOutOfRange,
    /// Internal library error.
    Internal,
}

impl ConversionErrorKind {
    /// Standard CLI exit code corresponding to this conversion error category.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UnsupportedFormat => 3,
            Self::UnsupportedVersion => 3,
            Self::Encrypted => 3,
            Self::MissingFont => 3,
            Self::CorruptInput => 5,
            Self::ResourceLimit => 6,
            Self::BackendFailure => 8,
            Self::UnsupportedCapability => 3,
            Self::PageOutOfRange => 3,
            Self::Internal => 8,
        }
    }
}

/// Strongly typed conversion error entity implementing standard error traits.
#[derive(Debug, Clone)]
pub struct ConversionError {
    pub kind: ConversionErrorKind,
    pub message: String,
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl Error for ConversionError {}

/// Convert in-memory DOC or DOCX bytes into complete PDF bytes and actual page count.
///
/// Guarantees:
/// 1. Pure in-memory invocation on input slice snapshot.
/// 2. No side effects: does not exit process, does not write stdout/stderr.
/// 3. No background lingering tasks or threads.
/// 4. Page count is validated >= 1; fails on 0 pages.
pub fn convert(
    input: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionOutput, ConversionError> {
    if input.len() > format::MAX_INPUT_BYTES {
        return Err(ConversionError {
            kind: ConversionErrorKind::ResourceLimit,
            message: format!(
                "Input size ({} bytes) exceeds maximum allowed limit of {} bytes (128 MiB)",
                input.len(),
                format::MAX_INPUT_BYTES
            ),
        });
    }

    if let Some(ref pages) = options.pages {
        pages::validate_page_indices(pages)?;
    }

    let decrypted_holder = crypto::decrypt_office_document(input, options.password.as_deref())?;
    let effective_input: &[u8] = match decrypted_holder.as_deref() {
        Some(d) => d,
        None => input,
    };

    let (fmt, mut pre_warnings) =
        format::verify_and_detect_format(effective_input, options.format)?;

    let mut output = match fmt {
        InputFormat::Doc => backend::convert_doc_route_b(effective_input, options)?,
        InputFormat::Docx => backend::convert_docx_route_c(effective_input, options)?,
        InputFormat::Auto => unreachable!(),
    };

    if !pre_warnings.is_empty() {
        pre_warnings.extend(output.warnings);
        output.warnings = pre_warnings;
    }

    Ok(output)
}
