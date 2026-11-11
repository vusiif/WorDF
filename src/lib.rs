//! WorDF: Independent DOC / DOCX to PDF converter in Rust
//! License: GPL-3.0-or-later

pub mod backend;
pub mod docx_adapt;
pub mod font;
pub mod format;
pub mod report;
pub mod transaction;

use std::error::Error;
use std::fmt;

// Re-export core items
pub use font::{count_cjk_chars, is_cjk_char, FontCmap, Format12Group, Format4Subtable};

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

// ==================== Legacy Compatibility Layer (for existing tests & callers) ====================

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_defect_2_tag_prefix_mismatch_repro() {
        let xml = r#"<w:r><w:rPr><w:rFonts w:ascii="仿宋"/></w:rPr><w:tab/></w:r><w:r><w:rPr><w:rFonts w:ascii="仿宋"/></w:rPr><w:t>正文</w:t></w:r>"#;
        let count = count_cjk_in_xml_text_nodes(xml);
        assert_eq!(count, 2, "Expected 2 CJK characters, got {}", count);

        let edge_case_xml = r#"
            <w:tblHeader/>
            <w:tab/>
            <w:tc><w:p><w:r><w:t xml:space="preserve"> 包含空格和中文 </w:t></w:r></w:p></w:tc>
            <w:r><w:t/></w:r>
            <w:r><w:tblText>表格内文本</w:tblText></w:r>
            <w:rPr><w:rFonts w:eastAsia="黑体"/></w:rPr>
        "#;
        assert_eq!(count_cjk_in_xml_text_nodes(edge_case_xml), 12);
    }

    #[test]
    fn test_chinese_body_2_document_xml_counts() {
        let candidates = [
            ".sandbox-test/word-corpus-local/0060058ef4456ad6efe272ca01724305046465c998a01522ef4acd82fc965012.docx",
            "/output/word-corpus-local/0060058ef4456ad6efe272ca01724305046465c998a01522ef4acd82fc965012.docx",
        ];
        let cb2_path = candidates.iter().find(|p| std::path::Path::new(p).exists());
        let Some(path) = cb2_path else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).unwrap();
        let mut file = archive.by_name("word/document.xml").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut file, &mut xml).unwrap();

        let mut hanzi_count = 0;
        let mut east_asia_count = 0;
        for_each_xml_text_node(&xml, |text| {
            for c in text.chars() {
                let o = c as u32;
                if (0x4E00..=0x9FFF).contains(&o) {
                    hanzi_count += 1;
                }
                if is_cjk_char(c) {
                    east_asia_count += 1;
                }
            }
        });
        assert_eq!(hanzi_count, 26267, "Hanzi count must be exactly 26,267");
        assert_eq!(east_asia_count, 29434, "East Asian count must be exactly 29,434");
    }

    pub fn make_minimal_docx(text: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let opt = zip::write::SimpleFileOptions::default();
            writer.start_file("[Content_Types].xml", opt).unwrap();
            writer.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
            writer.start_file("word/document.xml", opt).unwrap();
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
                text
            );
            writer.write_all(xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_defect_1_font_cmap_parsing_and_lookup() {
        let font_candidates = [
            ".sandbox-test/fonts/方正黑体简体.TTF",
            "/output/fonts/方正黑体简体.TTF",
        ];
        let font_path = font_candidates.iter().find(|p| std::path::Path::new(p).exists());
        let Some(path) = font_path else {
            return;
        };
        let font_bytes = std::fs::read(path).unwrap();
        let cmap = FontCmap::from_bytes(&font_bytes).expect("Failed to parse font cmap");

        assert!(!cmap.has_glyph('窂', &font_bytes), "窂 (U+7A82) must not have glyph");
        assert!(cmap.has_glyph('牢', &font_bytes), "牢 (U+7262) must have glyph");
        assert!(cmap.has_glyph('一', &font_bytes), "一 (U+4E00) must have glyph");
    }

    #[test]
    fn test_defect_1_missing_glyph_fails_fast_with_diagnostic() {
        let font_candidates = [
            ".sandbox-test/fonts/方正黑体简体.TTF",
            "/output/fonts/方正黑体简体.TTF",
        ];
        let font_path = font_candidates.iter().find(|p| std::path::Path::new(p).exists());
        let Some(path) = font_path else {
            return;
        };
        let font_bytes = std::fs::read(path).unwrap();

        let docx_missing = make_minimal_docx("桥梁栏杆或护栏窂固");
        let res_err = adapt_docx_cjk_in_memory_with_bytes(
            &docx_missing,
            Some("方正黑体简体"),
            Some(&font_bytes),
            &[],
        );
        assert!(res_err.is_err(), "Must fail fast when font lacks glyph");
        let err_msg = res_err.unwrap_err();
        assert!(err_msg.contains("Font diagnostic failure"), "Message: {}", err_msg);
        assert!(err_msg.contains("lacks glyphs for 1 East Asian characters"), "Message: {}", err_msg);
        assert!(err_msg.contains("'窂' (U+7A82)"), "Message: {}", err_msg);

        let docx_covered = make_minimal_docx("桥梁栏杆或护栏牢固");
        let res_ok = adapt_docx_cjk_in_memory_with_bytes(
            &docx_covered,
            Some("方正黑体简体"),
            Some(&font_bytes),
            &[],
        );
        assert!(res_ok.is_ok(), "Must succeed when all glyphs covered");
    }

    #[test]
    fn test_defect_self_closing_run_not_greedy() {
        let xml = r#"<w:p><w:r/><w:r><w:t>正文</w:t></w:r></w:p>"#;
        let (adapted, count) = adapt_xml_text(xml, "SimSun", &[]);
        assert_eq!(count, 2, "Expected 2 CJK characters, got {}", count);
        assert!(adapted.starts_with("<w:p><w:r/>"), "Adapted: {}", adapted);
        assert!(
            adapted.contains(r#"<w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>正文</w:t></w:r>"#),
            "Adapted: {}",
            adapted
        );

        let xml_attr = r#"<w:p><w:r rsidR="001234"/><w:r /><w:r><w:t>测试</w:t></w:r></w:p>"#;
        let (adapted_attr, count_attr) = adapt_xml_text(xml_attr, "SimSun", &[]);
        assert_eq!(count_attr, 2);
        assert!(adapted_attr.contains(r#"<w:r rsidR="001234"/>"#));
        assert!(adapted_attr.contains(r#"<w:r />"#));
        assert!(adapted_attr.contains(r#"<w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>测试</w:t>"#));

        let xml_newline = "<w:p><w:r\n><w:t>换行</w:t></w:r></w:p>";
        let (adapted_nl, count_nl) = adapt_xml_text(xml_newline, "SimSun", &[]);
        assert_eq!(count_nl, 2);
        assert!(adapted_nl.contains(r#"w:eastAsia="SimSun""#));

        let docx_with_self_closing = {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut writer = zip::ZipWriter::new(&mut buf);
                let opt = zip::write::SimpleFileOptions::default();
                writer.start_file("[Content_Types].xml", opt).unwrap();
                writer.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
                writer.start_file("word/document.xml", opt).unwrap();
                let doc_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r/><w:r><w:t>端到端测试</w:t></w:r></w:p></w:body></w:document>"#;
                writer.write_all(doc_xml.as_bytes()).unwrap();
                writer.finish().unwrap();
            }
            buf.into_inner()
        };
        let adapted_docx = adapt_docx_cjk_in_memory(&docx_with_self_closing, Some("SimSun"), &[])
            .expect("DOCX adaptation should succeed");
        let mut reader = zip::ZipArchive::new(std::io::Cursor::new(adapted_docx.0)).unwrap();
        let mut doc_file = reader.by_name("word/document.xml").unwrap();
        let mut xml_back = String::new();
        std::io::Read::read_to_string(&mut doc_file, &mut xml_back).unwrap();
        assert!(
            xml_back.contains(r#"<w:r/><w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>端到端测试</w:t></w:r>"#),
            "Expected correct run structure, got: {}",
            xml_back
        );
    }

    #[test]
    fn test_defect_expand_self_closing_rpr_single_rpr() {
        let xml = r#"<w:p><w:r><w:rPr/><w:t>中文正文</w:t></w:r></w:p>"#;
        let (adapted, count) = adapt_xml_text(xml, "SimSun", &[]);
        assert_eq!(count, 4);
        assert!(
            adapted.contains(r#"<w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>中文正文</w:t></w:r>"#),
            "Expected single in-place expanded rPr, got: {}",
            adapted
        );
        assert_eq!(
            adapted.matches("<w:rPr").count(),
            1,
            "Must have at most one <w:rPr opening tag per run, got: {}",
            adapted
        );
        assert_eq!(
            adapted.matches("</w:rPr>").count(),
            1,
            "Must have exactly one </w:rPr> closing tag per run, got: {}",
            adapted
        );

        let xml_space = r#"<w:p><w:r><w:rPr /><w:t>测试</w:t></w:r></w:p>"#;
        let (adapted_space, _) = adapt_xml_text(xml_space, "SimSun", &[]);
        assert_eq!(adapted_space.matches("<w:rPr").count(), 1);
        assert_eq!(adapted_space.matches("</w:rPr>").count(), 1);

        let xml_attr = r#"<w:p><w:r><w:rPr rsidR="001234"/><w:t>属性</w:t></w:r></w:p>"#;
        let (adapted_attr, _) = adapt_xml_text(xml_attr, "SimSun", &[]);
        assert_eq!(adapted_attr.matches("<w:rPr").count(), 1);
        assert_eq!(adapted_attr.matches("</w:rPr>").count(), 1);
        assert!(adapted_attr.contains(r#"rsidR="001234""#));
        assert!(adapted_attr.contains(r#"w:eastAsia="SimSun""#));
    }
}
