//! WorDF backend dispatch (DOC via rwml DOCX bridge, DOCX via docxide-pdf).
//! License: GPL-3.0-or-later

use crate::docx_adapt::adapt_docx_cjk_in_memory_with_bytes;
use crate::{ConversionError, ConversionErrorKind, ConversionOptions, ConversionOutput, Diagnostic};

pub fn convert_doc_route_b(
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<ConversionOutput, ConversionError> {
    let document = rwml::Document::open(bytes).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("rwml DOC open error: {:?}", e),
    })?;

    let docx_bytes = rwml::try_write_docx(&document.model()).map_err(|e| ConversionError {
        kind: ConversionErrorKind::BackendFailure,
        message: format!("rwml DOCX bridge export error: {:?}", e),
    })?;

    let mut output = convert_docx_route_c(&docx_bytes, options)?;

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
    let fallback_name = options.fallback_font_name.as_deref();
    let fallback_bytes = options.fallback_font_bytes.as_deref();

    let (adapted_docx, cjk_count) = adapt_docx_cjk_in_memory_with_bytes(
        bytes,
        fallback_name,
        fallback_bytes,
        &options.available_font_names,
    )?;

    let (pdf_bytes, raw_page_count) =
        docxide_pdf::convert_docx_bytes_to_pdf_bytes_with_page_count(&adapted_docx).map_err(
            |e| ConversionError {
                kind: ConversionErrorKind::BackendFailure,
                message: format!("docxide-pdf render error: {:?}", e),
            },
        )?;

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
