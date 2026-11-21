//! WorDF: Independent DOC / DOCX to PDF converter in Rust
//! License: GPL-3.0-or-later

pub mod backend;
pub mod compat;
pub mod docx_adapt;
pub mod font;
pub mod format;
pub mod report;
pub mod transaction;

use std::error::Error;
use std::fmt;

pub use compat::*;
pub use font::{
    count_cjk_chars, embedded_cjk_font_bytes, embedded_cjk_font_names, is_cjk_char, FontCmap,
    Format12Group, Format4Subtable, EMBEDDED_CJK_FONT_BYTES, EMBEDDED_CJK_FONT_NAME,
    EMBEDDED_CJK_FONT_NAME_ALT,
};

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
#[derive(Debug, Clone)]
pub struct ConversionOptions {
    /// Expected input format or Auto.
    pub format: InputFormat,
    /// Optional explicit CJK fallback font family name.
    pub fallback_font_name: Option<String>,
    /// Optional explicit CJK fallback font file bytes (for cmap pre-flight).
    pub fallback_font_bytes: Option<Vec<u8>>,
    /// System or environment available font family names.
    pub available_font_names: Vec<String>,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            format: InputFormat::Auto,
            fallback_font_name: None,
            fallback_font_bytes: None,
            available_font_names: Vec::new(),
        }
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
    /// Internal library error.
    Internal,
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
    let (fmt, mut pre_warnings) = format::verify_and_detect_format(input, options.format)?;

    let mut output = match fmt {
        InputFormat::Doc => backend::convert_doc_route_b(input, options)?,
        InputFormat::Docx => backend::convert_docx_route_c(input, options)?,
        InputFormat::Auto => unreachable!(),
    };

    if !pre_warnings.is_empty() {
        pre_warnings.extend(output.warnings);
        output.warnings = pre_warnings;
    }

    Ok(output)
}
