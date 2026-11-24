//! WorDF DOCX XML text parsing and CJK font adaptation.
//! License: GPL-3.0-or-later

pub mod styles;
pub mod xml;
pub mod xml_util;

pub use styles::{adapt_styles_xml, is_styles_part};
pub use xml::{
    adapt_run_xml, adapt_xml_text, count_cjk_in_xml_text_nodes, for_each_xml_text_node,
    is_text_part,
};
pub use xml_util::extract_attribute_value;

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::font::{is_cjk_char, FontCmap};
use crate::{ConversionError, ConversionErrorKind};

pub fn adapt_docx_cjk_in_memory<'a>(
    docx_bytes: &'a [u8],
    fallback_font: Option<&str>,
    available_fonts: &[String],
) -> Result<(Cow<'a, [u8]>, usize), ConversionError> {
    adapt_docx_cjk_in_memory_with_bytes(docx_bytes, fallback_font, None, available_fonts)
}

pub fn adapt_docx_cjk_in_memory_with_bytes<'a>(
    docx_bytes: &'a [u8],
    fallback_font: Option<&str>,
    fallback_font_bytes: Option<&[u8]>,
    available_fonts: &[String],
) -> Result<(Cow<'a, [u8]>, usize), ConversionError> {
    let reader = Cursor::new(docx_bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupt DOCX package: {}", e),
    })?;

    let mut out_buf = Cursor::new(Vec::with_capacity(docx_bytes.len()));
    let mut writer = ZipWriter::new(&mut out_buf);
    let mut total_cjk_in_package = 0;
    let mut modified_entries = 0;

    let font_cmap = if let Some(fb_bytes) = fallback_font_bytes {
        if !fb_bytes.is_empty() && fb_bytes == crate::font::embedded_cjk_font_bytes() {
            crate::font::get_or_init_embedded_font_cmap(fb_bytes).cloned()
        } else {
            FontCmap::from_bytes(fb_bytes)
        }
    } else {
        None
    };

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Corrupt zip entry: {}", e),
        })?;
        let name = file.name().to_string();
        let uncompressed_size = file.size() as usize;

        if is_text_part(&name) {
            let max_bytes = crate::format::docx::MAX_DOCX_MAIN_XML_BYTES;
            let mut xml_content = String::with_capacity(uncompressed_size.min(max_bytes as usize));
            let mut handle = (&mut file).take(max_bytes + 1);
            handle.read_to_string(&mut xml_content).map_err(|e| ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: format!("Failed to read XML text part '{}': {}", name, e),
            })?;
            if xml_content.len() as u64 > max_bytes {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!("XML text part '{}' decompressed size exceeds limit", name),
                });
            }

            // Glyph coverage fail-fast check
            if let (Some(cmap), Some(f_bytes)) = (&font_cmap, fallback_font_bytes) {
                let mut missing_chars = Vec::new();
                let mut checked_chars = HashSet::new();
                for_each_xml_text_node(&xml_content, |text| {
                    for c in text.chars() {
                        if is_cjk_char(c) && checked_chars.insert(c) && !cmap.has_glyph(c, f_bytes) {
                            missing_chars.push(c);
                        }
                    }
                });

                if !missing_chars.is_empty() {
                    let missing_summary: Vec<String> = missing_chars
                        .iter()
                        .map(|&c| format!("'{}' (U+{:04X})", c, c as u32))
                        .collect();
                    return Err(ConversionError {
                        kind: ConversionErrorKind::MissingFont,
                        message: format!(
                            "Font diagnostic failure: fallback font '{}' lacks glyphs for {} East Asian characters in '{}': [{}]",
                            fallback_font.unwrap_or("unknown"),
                            missing_chars.len(),
                            name,
                            missing_summary.join(", ")
                        ),
                    });
                }
            }

            if let Some(fb) = fallback_font {
                let (adapted_xml, cjk_count) = adapt_xml_text(&xml_content, fb, available_fonts);
                total_cjk_in_package += cjk_count;

                if adapted_xml != xml_content {
                    let options = SimpleFileOptions::default()
                        .compression_method(file.compression())
                        .unix_permissions(file.unix_mode().unwrap_or(0o644));

                    writer.start_file(&name, options).map_err(|e| ConversionError {
                        kind: ConversionErrorKind::Internal,
                        message: format!("Failed to write adapted zip entry '{}': {}", name, e),
                    })?;
                    writer.write_all(adapted_xml.as_bytes()).map_err(|e| ConversionError {
                        kind: ConversionErrorKind::Internal,
                        message: format!("Failed to write data for '{}': {}", name, e),
                    })?;
                    modified_entries += 1;
                    continue;
                }
            } else {
                total_cjk_in_package += count_cjk_in_xml_text_nodes(&xml_content);
            }
        }

        if is_styles_part(&name) {
            if let Some(fb) = fallback_font {
                let max_bytes = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
                let mut xml_content = String::with_capacity(uncompressed_size.min(max_bytes as usize));
                let mut handle = (&mut file).take(max_bytes + 1);
                if handle.read_to_string(&mut xml_content).is_ok() && xml_content.len() as u64 <= max_bytes {
                    let adapted_styles = adapt_styles_xml(&xml_content, fb, available_fonts);
                    if adapted_styles != xml_content {
                        let options = SimpleFileOptions::default()
                            .compression_method(file.compression())
                            .unix_permissions(file.unix_mode().unwrap_or(0o644));

                        writer.start_file(&name, options).map_err(|e| ConversionError {
                            kind: ConversionErrorKind::Internal,
                            message: format!("Failed to write adapted zip entry '{}': {}", name, e),
                        })?;
                        writer.write_all(adapted_styles.as_bytes()).map_err(|e| ConversionError {
                            kind: ConversionErrorKind::Internal,
                            message: format!("Failed to write data for '{}': {}", name, e),
                        })?;
                        modified_entries += 1;
                        continue;
                    }
                }
            }
        }

        // Raw-copy unmodified entry without decompression or re-compression
        writer.raw_copy_file(file).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to raw-copy entry '{}': {}", name, e),
        })?;
    }

    if modified_entries > 0 {
        writer.finish().map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to finalize adapted zip archive: {}", e),
        })?;
        Ok((Cow::Owned(out_buf.into_inner()), total_cjk_in_package))
    } else {
        Ok((Cow::Borrowed(docx_bytes), total_cjk_in_package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_copy_unmodified_binary_entry() {
        let mut buf = Cursor::new(Vec::new());
        let test_payload = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDRfake_image_bytes_here";
        {
            let mut writer = ZipWriter::new(&mut buf);
            let opt = SimpleFileOptions::default();
            writer.start_file("word/document.xml", opt).unwrap();
            writer.write_all(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>中文</w:t></w:r></w:p></w:body></w:document>"#.as_bytes()).unwrap();

            writer.start_file("word/media/image1.png", opt).unwrap();
            writer.write_all(test_payload).unwrap();
            writer.finish().unwrap();
        }
        let input_zip = buf.into_inner();

        let (adapted_zip, count) = adapt_docx_cjk_in_memory_with_bytes(
            &input_zip,
            Some("SimSun"),
            None,
            &[],
        ).expect("Adaptation must succeed");

        assert_eq!(count, 2);

        // Verify media entry was preserved byte-for-byte
        let mut archive = ZipArchive::new(Cursor::new(adapted_zip.as_ref())).unwrap();
        let mut media_file = archive.by_name("word/media/image1.png").expect("media entry must exist");
        let mut extracted = Vec::new();
        media_file.read_to_end(&mut extracted).unwrap();
        assert_eq!(extracted, test_payload, "Raw-copied binary stream must be 100% identical");
    }

    #[test]
    fn test_adapt_docx_styles_xml_injection() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buf);
            let opt = SimpleFileOptions::default();
            writer.start_file("word/document.xml", opt).unwrap();
            writer.write_all(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>正文</w:t></w:r></w:p></w:body></w:document>"#.as_bytes()).unwrap();

            writer.start_file("word/styles.xml", opt).unwrap();
            writer.write_all(r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        let input_zip = buf.into_inner();

        let (adapted_zip, count) = adapt_docx_cjk_in_memory_with_bytes(
            &input_zip,
            Some("LXGW WenKai Lite"),
            None,
            &[],
        ).expect("Adaptation must succeed");

        assert_eq!(count, 2);

        let mut archive = ZipArchive::new(Cursor::new(adapted_zip.as_ref())).unwrap();
        let mut styles_file = archive.by_name("word/styles.xml").expect("styles.xml must exist");
        let mut styles_xml = String::new();
        styles_file.read_to_string(&mut styles_xml).unwrap();

        assert!(styles_xml.contains(r#"w:ascii="Calibri""#), "West-European font must be preserved");
        assert!(styles_xml.contains(r#"w:eastAsia="LXGW WenKai Lite""#), "East Asian fallback font must be injected into docDefaults");
    }
}
