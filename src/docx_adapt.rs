//! WorDF DOCX XML text parsing and CJK font adaptation.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::font::{count_cjk_chars, is_cjk_char, FontCmap};
use crate::{ConversionError, ConversionErrorKind};

pub fn is_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name.starts_with("word/header")
        || name.starts_with("word/footer")
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        || name == "word/comments.xml"
}

pub fn for_each_xml_text_node<F>(xml: &str, mut f: F)
where
    F: FnMut(&str),
{
    let mut cursor = 0;
    while let Some(rel_tag_start) = xml[cursor..].find('<') {
        let tag_start = cursor + rel_tag_start;
        let rem = &xml[tag_start..];

        let (prefix_len, close_tag) = if rem.starts_with("<w:tblText") {
            (10, "</w:tblText>")
        } else if rem.starts_with("<w:t") {
            (4, "</w:t>")
        } else {
            cursor = tag_start + 1;
            continue;
        };

        // Boundary check: the character immediately following the tag name
        // must be '>', ' ', '\t', '\r', '\n', or '/'
        let byte_after = rem.as_bytes().get(prefix_len);
        let is_valid_tag = matches!(byte_after, Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/'));
        if !is_valid_tag {
            cursor = tag_start + prefix_len;
            continue;
        }

        let Some(tag_end_rel) = rem[prefix_len..].find('>') else {
            break;
        };
        let abs_tag_end = tag_start + prefix_len + tag_end_rel;
        let open_tag = &xml[tag_start..=abs_tag_end];

        if open_tag.ends_with("/>") {
            cursor = abs_tag_end + 1;
            continue;
        }

        let content_start = abs_tag_end + 1;
        if let Some(close_rel) = xml[content_start..].find(close_tag) {
            let abs_close = content_start + close_rel;
            let text_inside = &xml[content_start..abs_close];
            f(text_inside);
            cursor = abs_close + close_tag.len();
        } else {
            cursor = content_start;
        }
    }
}

pub fn count_cjk_in_xml_text_nodes(xml: &str) -> usize {
    let mut total = 0;
    for_each_xml_text_node(xml, |text| {
        total += count_cjk_chars(text);
    });
    total
}

pub fn adapt_run_xml(
    run_xml: &str,
    fallback_font: &str,
    available_fonts: &[String],
) -> String {
    let mut has_cjk = false;
    for_each_xml_text_node(run_xml, |text| {
        if !has_cjk && count_cjk_chars(text) > 0 {
            has_cjk = true;
        }
    });

    if !has_cjk {
        return run_xml.to_string();
    }

    if let Some(rfonts_start) = run_xml.find("<w:rFonts") {
        if let Some(rfonts_end) = run_xml[rfonts_start..].find("/>") {
            let rfonts_tag = &run_xml[rfonts_start..rfonts_start + rfonts_end + 2];
            let mut already_has_cjk_font = false;
            for font in available_fonts {
                if rfonts_tag.contains(font) {
                    already_has_cjk_font = true;
                    break;
                }
            }
            if already_has_cjk_font {
                return run_xml.to_string();
            }

            let mut new_rfonts = rfonts_tag.trim_end_matches("/>").to_string();
            if new_rfonts.contains("w:eastAsia=") {
                let re_start = new_rfonts.find("w:eastAsia=\"").unwrap();
                let val_start = re_start + 12;
                if let Some(val_end) = new_rfonts[val_start..].find('"') {
                    let before = &new_rfonts[..val_start];
                    let after = &new_rfonts[val_start + val_end..];
                    new_rfonts = format!("{}{}{}", before, fallback_font, after);
                }
            } else {
                new_rfonts.push_str(&format!(r#" w:eastAsia="{}""#, fallback_font));
            }
            new_rfonts.push_str("/>");

            let mut result = String::with_capacity(run_xml.len() + 64);
            result.push_str(&run_xml[..rfonts_start]);
            result.push_str(&new_rfonts);
            result.push_str(&run_xml[rfonts_start + rfonts_end + 2..]);
            return result;
        }
    }

    if let Some(rpr_idx) = run_xml.find("<w:rPr") {
        let rpr_after = rpr_idx + "<w:rPr".len();
        if let Some(rpr_end_tag) = run_xml[rpr_after..].find('>') {
            let rpr_content_start = rpr_after + rpr_end_tag + 1;
            let rpr_tag = &run_xml[rpr_idx..rpr_content_start];
            if rpr_tag.ends_with("/>") {
                // Self-closing <w:rPr ... />: replace trailing "/>" with children and </w:rPr>
                let open_prefix = &rpr_tag[..rpr_tag.len() - 2];
                let mut result = String::with_capacity(run_xml.len() + 64);
                result.push_str(&run_xml[..rpr_idx]);
                result.push_str(open_prefix);
                result.push_str(&format!(r#"><w:rFonts w:eastAsia="{}"/></w:rPr>"#, fallback_font));
                result.push_str(&run_xml[rpr_content_start..]);
                return result;
            }
        }
    }

    if let Some(rpr_end) = run_xml.find("</w:rPr>") {
        let mut result = String::with_capacity(run_xml.len() + 64);
        result.push_str(&run_xml[..rpr_end]);
        result.push_str(&format!(r#"<w:rFonts w:eastAsia="{}"/>"#, fallback_font));
        result.push_str(&run_xml[rpr_end..]);
        return result;
    }

    if let Some(r_tag_end) = run_xml.find('>') {
        let mut result = String::with_capacity(run_xml.len() + 80);
        result.push_str(&run_xml[..=r_tag_end]);
        result.push_str(&format!(
            r#"<w:rPr><w:rFonts w:eastAsia="{}"/></w:rPr>"#,
            fallback_font
        ));
        result.push_str(&run_xml[r_tag_end + 1..]);
        return result;
    }

    run_xml.to_string()
}

pub fn adapt_xml_text(
    xml: &str,
    fallback_font: &str,
    available_fonts: &[String],
) -> (String, usize) {
    let mut out = String::with_capacity(xml.len() + 1024);
    let mut total_cjk = 0;
    let mut cursor = 0;

    while let Some(r_start) = xml[cursor..].find("<w:r") {
        let abs_r_start = cursor + r_start;
        let after_wr = abs_r_start + 4;
        if after_wr < xml.len() {
            let next_b = xml.as_bytes()[after_wr];
            if next_b == b'>' || next_b == b' ' || next_b == b'\t' || next_b == b'\r' || next_b == b'\n' || next_b == b'/' {
                let tag_end_opt = xml[after_wr..].find('>');
                let abs_tag_end = match tag_end_opt {
                    Some(idx) => after_wr + idx,
                    None => {
                        out.push_str(&xml[cursor..]);
                        return (out, total_cjk);
                    }
                };

                let is_self_closing = xml[after_wr..abs_tag_end].trim_end().ends_with('/');
                if is_self_closing {
                    out.push_str(&xml[cursor..=abs_tag_end]);
                    cursor = abs_tag_end + 1;
                    continue;
                }
                if let Some(r_end) = xml[abs_tag_end + 1..].find("</w:r>") {
                    let abs_r_end = abs_tag_end + 1 + r_end + 6;
                    out.push_str(&xml[cursor..abs_r_start]);
                    let run_slice = &xml[abs_r_start..abs_r_end];
                    total_cjk += count_cjk_in_xml_text_nodes(run_slice);
                    let adapted = adapt_run_xml(run_slice, fallback_font, available_fonts);
                    out.push_str(&adapted);
                    cursor = abs_r_end;
                    continue;
                }
            }
        }
        out.push_str(&xml[cursor..abs_r_start + 4]);
        cursor = abs_r_start + 4;
    }

    out.push_str(&xml[cursor..]);
    (out, total_cjk)
}

pub fn adapt_docx_cjk_in_memory(
    docx_bytes: &[u8],
    fallback_font: Option<&str>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, usize), ConversionError> {
    adapt_docx_cjk_in_memory_with_bytes(docx_bytes, fallback_font, None, available_fonts)
}

pub fn adapt_docx_cjk_in_memory_with_bytes(
    docx_bytes: &[u8],
    fallback_font: Option<&str>,
    fallback_font_bytes: Option<&[u8]>,
    available_fonts: &[String],
) -> Result<(Vec<u8>, usize), ConversionError> {
    let reader = Cursor::new(docx_bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupt DOCX package: {}", e),
    })?;

    let mut out_buf = Cursor::new(Vec::with_capacity(docx_bytes.len()));
    let mut writer = ZipWriter::new(&mut out_buf);
    let mut total_cjk_in_package = 0;
    let mut modified_entries = 0;

    let font_cmap = fallback_font_bytes.and_then(FontCmap::from_bytes);

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Corrupt zip entry: {}", e),
        })?;
        let name = file.name().to_string();

        if is_text_part(&name) {
            let mut xml_content = String::new();
            file.read_to_string(&mut xml_content).map_err(|e| ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: format!("Failed to read XML text part '{}': {}", name, e),
            })?;

            // Glyph coverage fail-fast check
            if let (Some(cmap), Some(f_bytes)) = (&font_cmap, fallback_font_bytes) {
                let mut missing_chars = Vec::new();
                for_each_xml_text_node(&xml_content, |text| {
                    for c in text.chars() {
                        if is_cjk_char(c) && !cmap.has_glyph(c, f_bytes) {
                            if !missing_chars.contains(&c) {
                                missing_chars.push(c);
                            }
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
            } else {
                total_cjk_in_package += count_cjk_in_xml_text_nodes(&xml_content);
            }
        }

        // Copy unmodified entry
        let mut raw_bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut raw_bytes).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Failed to copy entry '{}': {}", name, e),
        })?;

        let options = SimpleFileOptions::default()
            .compression_method(file.compression())
            .unix_permissions(file.unix_mode().unwrap_or(0o644));

        writer.start_file(&name, options).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to write copied entry '{}': {}", name, e),
        })?;
        writer.write_all(&raw_bytes).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to write data for '{}': {}", name, e),
        })?;
    }

    writer.finish().map_err(|e| ConversionError {
        kind: ConversionErrorKind::Internal,
        message: format!("Failed to finalize adapted zip archive: {}", e),
    })?;

    if modified_entries > 0 {
        Ok((out_buf.into_inner(), total_cjk_in_package))
    } else {
        Ok((docx_bytes.to_vec(), total_cjk_in_package))
    }
}
