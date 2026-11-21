//! WorDF legacy compatibility functions and conversion routes.
//! License: GPL-3.0-or-later

use crate::backend;
use crate::docx_adapt;
use crate::format;
use crate::{ConversionOptions, InputFormat};

pub fn detect_format(bytes: &[u8]) -> InputFormat {
    match format::detect_format_magic(bytes) {
        Some(f) => f,
        None => InputFormat::Auto,
    }
}

pub fn convert_route_a_doc_rwml(bytes: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    convert_route_a_doc_rwml_with_fonts(bytes, &[])
}

pub fn convert_route_a_doc_rwml_with_fonts(
    bytes: &[u8],
    fonts: &[Vec<u8>],
) -> Result<(Vec<u8>, Vec<String>), String> {
    let document = rwml::Document::open(bytes).map_err(|e| format!("rwml open error: {:?}", e))?;
    let rendered = rwml::try_render_pdf_with_fonts_and_report(&document.model(), fonts)
        .map_err(|e| format!("rwml render error: {:?}", e))?;

    let mut warnings: Vec<String> = rendered
        .report
        .warnings
        .into_iter()
        .map(|w| format!("{:?}", w))
        .collect();

    if rendered.pdf.len() < 1500 && !document.model().blocks.is_empty() {
        warnings.push(
            "Route A diagnostic: PDF page stream appears empty or text-less (potential ICU4X complex script segmentation model missing or unconfigured font stack)"
                .to_string(),
        );
    }

    Ok((rendered.pdf, warnings))
}

pub fn convert_route_b_doc_bridge(bytes: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    convert_route_b_doc_bridge_with_fallback(bytes, None, &[])
}

pub fn convert_route_b_doc_bridge_with_fallback(
    bytes: &[u8],
    fallback_font: Option<&str>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, Vec<String>), String> {
    convert_route_b_doc_bridge_with_font_bytes(bytes, fallback_font, None, available_fonts)
}

pub fn convert_route_b_doc_bridge_with_font_bytes(
    bytes: &[u8],
    fallback_font: Option<&str>,
    fallback_font_bytes: Option<&[u8]>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, Vec<String>), String> {
    let opts = ConversionOptions {
        format: InputFormat::Doc,
        fallback_font_name: fallback_font.map(|s| s.to_string()),
        fallback_font_bytes: fallback_font_bytes.map(|b| b.to_vec()),
        available_font_names: available_fonts.to_vec(),
    };
    convert_doc_route_b_compat(bytes, &opts)
}

pub fn convert_route_c_docx_docxide(bytes: &[u8]) -> Result<Vec<u8>, String> {
    convert_route_c_docx_docxide_with_fallback(bytes, None, &[]).map(|(pdf, _)| pdf)
}

pub fn convert_route_c_docx_docxide_with_fallback(
    bytes: &[u8],
    fallback_font: Option<&str>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, Vec<String>), String> {
    convert_route_c_docx_docxide_with_font_bytes(bytes, fallback_font, None, available_fonts)
}

pub fn convert_route_c_docx_docxide_with_font_bytes(
    bytes: &[u8],
    fallback_font: Option<&str>,
    fallback_font_bytes: Option<&[u8]>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, Vec<String>), String> {
    let opts = ConversionOptions {
        format: InputFormat::Docx,
        fallback_font_name: fallback_font.map(|s| s.to_string()),
        fallback_font_bytes: fallback_font_bytes.map(|b| b.to_vec()),
        available_font_names: available_fonts.to_vec(),
    };
    convert_docx_route_c_compat(bytes, &opts)
}

fn convert_doc_route_b_compat(bytes: &[u8], opts: &ConversionOptions) -> Result<(Vec<u8>, Vec<String>), String> {
    let out = backend::convert_doc_route_b(bytes, opts).map_err(|e| e.to_string())?;
    let warns = out.warnings.into_iter().map(|w| w.message).collect();
    Ok((out.pdf, warns))
}

fn convert_docx_route_c_compat(bytes: &[u8], opts: &ConversionOptions) -> Result<(Vec<u8>, Vec<String>), String> {
    let out = backend::convert_docx_route_c(bytes, opts).map_err(|e| e.to_string())?;
    let warns = out.warnings.into_iter().map(|w| w.message).collect();
    Ok((out.pdf, warns))
}

pub fn adapt_docx_cjk_in_memory(
    bytes: &[u8],
    fallback_font: Option<&str>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, usize), String> {
    docx_adapt::adapt_docx_cjk_in_memory(bytes, fallback_font, available_fonts)
        .map(|(cow, count)| (cow.into_owned(), count))
        .map_err(|e| e.to_string())
}

pub fn adapt_docx_cjk_in_memory_with_bytes(
    bytes: &[u8],
    fallback_font: Option<&str>,
    fallback_font_bytes: Option<&[u8]>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, usize), String> {
    docx_adapt::adapt_docx_cjk_in_memory_with_bytes(
        bytes,
        fallback_font,
        fallback_font_bytes,
        available_fonts,
    )
    .map(|(cow, count)| (cow.into_owned(), count))
    .map_err(|e| e.to_string())
}

pub fn for_each_xml_text_node<F>(xml: &str, cb: F)
where
    F: FnMut(&str),
{
    docx_adapt::for_each_xml_text_node(xml, cb);
}

pub fn count_cjk_in_xml_text_nodes(xml: &str) -> usize {
    docx_adapt::count_cjk_in_xml_text_nodes(xml)
}

pub fn adapt_xml_text(xml: &str, fallback_font: &str, available_fonts: &[String]) -> (String, usize) {
    docx_adapt::adapt_xml_text(xml, fallback_font, available_fonts)
}
