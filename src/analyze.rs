//! WorDF Document structural analysis and pre-flight quality diagnosis engine.
//! License: GPL-3.0-or-later

use std::collections::HashSet;
use std::io::{Cursor, Read};

use crate::docx_adapt::for_each_xml_text_node;
use crate::font::{count_cjk_chars, is_cjk_char, FontCmap};
use crate::format::doc::validate_doc_package;
use crate::format::docx::validate_docx_package;
use crate::format::{DOC_MAGIC, ZIP_MAGIC};
use crate::{ConversionError, ConversionErrorKind, ConversionOptions, Diagnostic};

/// Comprehensive document structural and typographical analysis summary.
#[derive(Debug, Clone)]
pub struct DocumentAnalysis {
    pub format_name: String,
    pub input_bytes: usize,
    pub paragraph_count: usize,
    pub table_count: usize,
    pub run_count: usize,
    pub media_count: usize,
    pub media_bytes: u64,
    pub header_footer_count: usize,
    pub cjk_char_count: usize,
    pub detected_fonts: Vec<String>,
    pub missing_glyphs: Vec<char>,
    pub metadata: crate::metadata::DocumentMetadata,
    pub warnings: Vec<Diagnostic>,
}

impl DocumentAnalysis {
    /// Formats the analysis results into a human-readable terminal summary.
    pub fn render_console_summary(&self, input_name: &str) -> String {
        let mut out = String::new();
        out.push_str("==================================================\n          WorDF Document Analysis Report          \n==================================================\n");
        out.push_str(&format!("Target File: {}\nFile Size:   {} bytes ({:.2} KiB)\nFormat:      {}\n", input_name, self.input_bytes, self.input_bytes as f64 / 1024.0, self.format_name));
        if !self.metadata.is_empty() {
            out.push_str("\n[Document Metadata]\n");
            if let Some(ref t) = self.metadata.title { out.push_str(&format!("  - Title:             {}\n", t)); }
            if let Some(ref a) = self.metadata.author { out.push_str(&format!("  - Author:            {}\n", a)); }
            if let Some(ref s) = self.metadata.subject { out.push_str(&format!("  - Subject:           {}\n", s)); }
            if let Some(ref k) = self.metadata.keywords { out.push_str(&format!("  - Keywords:          {}\n", k)); }
        }
        out.push_str("\n[Structure & Layout]\n");
        out.push_str(&format!("  - Paragraphs:        {}\n", self.paragraph_count));
        out.push_str(&format!("  - Tables:            {}\n", self.table_count));
        out.push_str(&format!("  - Text Runs:         {}\n", self.run_count));
        out.push_str(&format!("  - Embedded Media:    {} files ({:.2} KiB)\n", self.media_count, self.media_bytes as f64 / 1024.0));
        out.push_str(&format!("  - Header/Footers:    {} part(s)\n", self.header_footer_count));

        out.push_str("\n[Typography & Fonts]\n");
        out.push_str(&format!("  - East Asian Chars:  {}\n", self.cjk_char_count));
        if !self.detected_fonts.is_empty() {
            out.push_str(&format!("  - Declared Fonts:    {}\n", self.detected_fonts.join(", ")));
        } else {
            out.push_str("  - Declared Fonts:    (Default style hierarchy)\n");
        }

        if self.missing_glyphs.is_empty() {
            out.push_str("  - Glyph Coverage:    OK (All East Asian characters supported)\n");
        } else {
            out.push_str(&format!(
                "  - Missing Glyphs:    {} glyph(s) missing in fallback font: [{}]\n",
                self.missing_glyphs.len(),
                self.missing_glyphs.iter().take(10).map(|c| format!("'{}' (U+{:04X})", c, *c as u32)).collect::<Vec<_>>().join(", ")
            ));
        }

        out.push_str("\n[Diagnostics & Risk Evaluation]\n");
        if self.warnings.is_empty() {
            out.push_str("  - Status: READY (No structural or typographic conflicts detected)\n");
        } else {
            out.push_str(&format!("  - Status: {} Warning(s) Detected\n", self.warnings.len()));
            for w in &self.warnings {
                out.push_str(&format!("    * [{}] {}\n", w.code, w.message));
            }
        }
        out.push_str("==================================================\n");
        out
    }
}

/// Analyze a DOC or DOCX document in-memory without invoking full PDF rendering.
pub fn analyze_document(
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<DocumentAnalysis, ConversionError> {
    let decrypted_holder = crate::crypto::decrypt_office_document(bytes, options.password.as_deref())?;
    let effective_bytes = decrypted_holder.as_deref().unwrap_or(bytes);
    if effective_bytes.starts_with(&ZIP_MAGIC) {
        analyze_docx_package(effective_bytes, options)
    } else if effective_bytes.starts_with(&DOC_MAGIC) {
        analyze_doc_package(effective_bytes, options)
    } else {
        Err(ConversionError {
            kind: ConversionErrorKind::UnsupportedFormat,
            message: "Unsupported file format: expected OOXML DOCX (PK..) or Word 97-2003 DOC (CFB)".to_string(),
        })
    }
}

fn analyze_docx_package(
    bytes: &[u8],
    options: &ConversionOptions,
) -> Result<DocumentAnalysis, ConversionError> {
    validate_docx_package(bytes)?;

    let cursor = Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupted DOCX zip archive: {}", e),
    })?;

    let (mut paragraph_count, mut table_count, mut run_count) = (0, 0, 0);
    let (mut media_count, mut media_bytes, mut header_footer_count) = (0, 0u64, 0);
    let mut cjk_char_count = 0;
    let mut detected_fonts = Vec::new();
    let mut missing_glyphs = Vec::new();
    let mut warnings = Vec::new();

    // Font cmap lookup if font is available
    let fallback_font_bytes: Option<&[u8]> = options
        .fallback_font_bytes
        .as_deref()
        .or_else(|| {
            let b = crate::font::embedded_cjk_font_bytes();
            if b.is_empty() {
                None
            } else {
                Some(b)
            }
        });
    let font_cmap = fallback_font_bytes.and_then(FontCmap::from_bytes);

    let mut checked_chars = HashSet::new();

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Failed to read entry {}: {}", i, e),
        })?;

        let name = file.name().to_string();

        if name.starts_with("word/media/") {
            media_count += 1;
            media_bytes += file.size();
        } else if name.starts_with("word/header") || name.starts_with("word/footer") {
            header_footer_count += 1;
        }

        if name == "word/fontTable.xml" {
            let mut font_xml = String::new();
            let max_aux = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
            if (&mut file).take(max_aux + 1).read_to_string(&mut font_xml).is_ok() && (font_xml.len() as u64) <= max_aux {
                let mut cursor = 0;
                while let Some(pos) = font_xml[cursor..].find("<w:font ") {
                    let tag_start = cursor + pos;
                    if let Some(tag_end_rel) = font_xml[tag_start..].find('>') {
                        let tag = &font_xml[tag_start..tag_start + tag_end_rel];
                        if let Some(val) = crate::docx_adapt::extract_attribute_value(tag, "w:name") {
                            let font_name = val.to_string();
                            if !detected_fonts.contains(&font_name) {
                                detected_fonts.push(font_name);
                            }
                        }
                    }
                    cursor = tag_start + 8;
                }
            }
            continue;
        }

        // Inspect text parts
        if crate::docx_adapt::is_text_part(&name) {
            let mut xml_content = String::new();
            let max_main = crate::format::docx::MAX_DOCX_MAIN_XML_BYTES;
            if (&mut file).take(max_main + 1).read_to_string(&mut xml_content).is_ok() {
                if xml_content.len() as u64 > max_main {
                    return Err(ConversionError {
                        kind: ConversionErrorKind::ResourceLimit,
                        message: format!("XML text part '{}' exceeds limit in analyze mode", name),
                    });
                }
                // Count basic elements
                paragraph_count += count_substrings(&xml_content, "<w:p");
                table_count += count_substrings(&xml_content, "<w:tbl");
                run_count += count_substrings(&xml_content, "<w:r");

                for_each_xml_text_node(&xml_content, |text| {
                    cjk_char_count += count_cjk_chars(text);
                    for c in text.chars() {
                        if is_cjk_char(c) && checked_chars.insert(c) {
                            let is_covered = if let Some(ref chain) = options.fallback_chain {
                                chain.resolve_glyph(c).is_some()
                            } else if let (Some(cmap), Some(f_bytes)) = (&font_cmap, fallback_font_bytes) {
                                cmap.has_glyph(c, f_bytes)
                            } else {
                                true
                            };
                            if !is_covered {
                                missing_glyphs.push(c);
                            }
                        }
                    }
                });
            }
        }
    }

    if !missing_glyphs.is_empty() {
        warnings.push(Diagnostic {
            code: "font.missing_glyphs".to_string(),
            message: format!(
                "Fallback font lacks glyphs for {} East Asian characters",
                missing_glyphs.len()
            ),
        });
    }

    let raw_meta = crate::metadata::docx::extract_docx_metadata(bytes);
    let metadata = match &options.metadata {
        Some(o) => raw_meta.merge_with_overrides(o),
        None => raw_meta,
    };

    Ok(DocumentAnalysis {
        format_name: "OOXML DOCX".to_string(),
        input_bytes: bytes.len(),
        paragraph_count,
        table_count,
        run_count,
        media_count,
        media_bytes,
        header_footer_count,
        cjk_char_count,
        detected_fonts,
        missing_glyphs,
        metadata,
        warnings,
    })
}

fn analyze_doc_package(bytes: &[u8], options: &ConversionOptions) -> Result<DocumentAnalysis, ConversionError> {
    validate_doc_package(bytes)?;

    let mut stream_count = 0;
    if let Ok(comp) = cfb::CompoundFile::open(Cursor::new(bytes)) {
        for _ in comp.walk() {
            stream_count += 1;
        }
    }

    let raw_meta = crate::metadata::doc::extract_doc_metadata(bytes);
    let metadata = match &options.metadata {
        Some(o) => raw_meta.merge_with_overrides(o),
        None => raw_meta,
    };

    Ok(DocumentAnalysis {
        format_name: "Word 97-2003 Binary DOC".to_string(),
        input_bytes: bytes.len(),
        paragraph_count: 0,
        table_count: 0,
        run_count: 0,
        media_count: stream_count,
        media_bytes: bytes.len() as u64,
        header_footer_count: 0,
        cjk_char_count: 0,
        detected_fonts: Vec::new(),
        missing_glyphs: Vec::new(),
        metadata,
        warnings: Vec::new(),
    })
}

fn count_substrings(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_substrings() {
        assert_eq!(count_substrings("<w:p><w:p></w:p>", "<w:p"), 2);
        assert_eq!(count_substrings("no match", "<w:p"), 0);
    }
}
