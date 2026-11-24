//! WorDF backend dispatch (DOC via rwml DOCX bridge, DOCX via docxide-pdf).
//! License: GPL-3.0-or-later

use crate::docx_adapt::adapt_docx_cjk_in_memory_with_bytes;
use crate::{
    ConversionError, ConversionErrorKind, ConversionOptions, ConversionOutput, Diagnostic,
};

pub fn convert_doc_route_b(
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionOutput, ConversionError> {
    let doc_meta = crate::metadata::doc::extract_doc_metadata(bytes);
    let merged_meta = match &options.metadata {
        Some(overrides) => doc_meta.merge_with_overrides(overrides),
        None => doc_meta,
    };

    let document = rwml::Document::open(bytes).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("rwml DOC open error: {:?}", e),
    })?;

    let docx_bytes = rwml::try_write_docx(&document.model()).map_err(|e| ConversionError {
        kind: ConversionErrorKind::BackendFailure,
        message: format!("rwml DOCX bridge export error: {:?}", e),
    })?;

    let mut opts_with_meta = options.clone();
    opts_with_meta.metadata = if merged_meta.is_empty() {
        None
    } else {
        Some(merged_meta)
    };

    let mut output = convert_docx_route_c(&docx_bytes, &opts_with_meta)?;

    output.warnings.insert(
        0,
        Diagnostic {
            code: "layout.approximate".to_string(),
            message: "Preview-grade layout via rwml DOCX bridge".to_string(),
        },
    );

    Ok(output)
}

pub fn convert_docx_route_c(
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionOutput, ConversionError> {
    let embedded_bytes = crate::font::embedded_cjk_font_bytes();
    if !embedded_bytes.is_empty() {
        docxide_pdf::register_default_embedded_font(
            crate::font::embedded_cjk_font_names(),
            embedded_bytes,
        );
    }

    let (fallback_name, fallback_bytes) = match options.fallback_font_name.as_deref() {
        Some(name) => {
            let bytes = options.fallback_font_bytes.as_deref().or_else(|| {
                if !embedded_bytes.is_empty()
                    && (name == crate::font::EMBEDDED_CJK_FONT_NAME
                        || name == crate::font::EMBEDDED_CJK_FONT_NAME_ALT)
                {
                    Some(embedded_bytes)
                } else {
                    None
                }
            });
            (Some(name), bytes)
        }
        None => {
            if !embedded_bytes.is_empty() {
                (
                    Some(crate::font::EMBEDDED_CJK_FONT_NAME),
                    Some(embedded_bytes),
                )
            } else {
                (None, None)
            }
        }
    };

    let mut available_fonts = options.available_font_names.clone();
    for name in crate::font::embedded_cjk_font_names() {
        if !available_fonts.contains(&name) {
            available_fonts.push(name);
        }
    }

    let (adapted_docx, cjk_count) = adapt_docx_cjk_in_memory_with_bytes(
        bytes,
        fallback_name,
        fallback_bytes,
        &available_fonts,
    )?;

    if cjk_count > 0 && fallback_name.is_none() {
        return Err(ConversionError {
            kind: ConversionErrorKind::MissingFont,
            message: format!(
                "Document contains {} East Asian characters, but no CJK font is available (built without 'embedded-cjk-font' feature and no fallback font configured).",
                cjk_count
            ),
        });
    }

    let final_docx: std::borrow::Cow<'_, [u8]> = match &options.metadata {
        Some(meta) if !meta.is_empty() => std::borrow::Cow::Owned(
            crate::metadata::docx::inject_docx_metadata(&adapted_docx, meta)?,
        ),
        _ => adapted_docx,
    };

    let (pdf_bytes, raw_page_count) =
        docxide_pdf::convert_docx_bytes_to_pdf_bytes_with_page_filter(
            final_docx.as_ref(),
            options.pages.as_deref(),
        )
        .map_err(|e| {
            let msg = format!("docxide-pdf render error: {:?}", e);
            if msg.contains("out of range") {
                ConversionError {
                    kind: ConversionErrorKind::PageOutOfRange,
                    message: msg,
                }
            } else {
                ConversionError {
                    kind: ConversionErrorKind::BackendFailure,
                    message: msg,
                }
            }
        })?;

    if raw_page_count == 0 {
        return Err(ConversionError {
            kind: ConversionErrorKind::BackendFailure,
            message: "Generated PDF contains zero pages".to_string(),
        });
    }

    if pdf_bytes.len() < 100 || !pdf_bytes.starts_with(b"%PDF-") {
        return Err(ConversionError {
            kind: ConversionErrorKind::BackendFailure,
            message: "Generated PDF header is corrupt or truncated".to_string(),
        });
    }

    let page_count = u32::try_from(raw_page_count).map_err(|_| ConversionError {
        kind: ConversionErrorKind::BackendFailure,
        message: format!("Page count {} overflows u32", raw_page_count),
    })?;

    let mut warnings = Vec::new();
    if cjk_count > 0 {
        if let Some(fb) = fallback_name {
            warnings.push(Diagnostic {
                code: "font.fallback".to_string(),
                message: format!(
                    "Generic CJK fallback active: {} characters mapped to '{}'",
                    cjk_count, fb
                ),
            });
        }
    }

    Ok(ConversionOutput {
        pdf: pdf_bytes,
        page_count,
        warnings,
    })
}
