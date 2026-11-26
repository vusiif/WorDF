//! DOCX (OOXML) core.xml metadata extraction and injection.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use super::DocumentMetadata;
use crate::{ConversionError, ConversionErrorKind};

fn extract_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let start_idx = xml.find(&open)?;
    let tag_tail = &xml[start_idx + open.len()..];
    let gt_rel = tag_tail.find('>')?;
    if gt_rel > 0 && tag_tail.as_bytes()[gt_rel - 1] == b'/' {
        return None;
    }
    let content_start = start_idx + open.len() + gt_rel + 1;
    let close = format!("</{}", tag);
    let end_idx = content_start + xml[content_start..].find(&close)?;
    let raw = xml[content_start..end_idx].trim();
    if raw.is_empty() {
        return None;
    }
    let unescaped = quick_xml::escape::unescape(raw)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| raw.to_string());
    let trimmed = unescaped.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extract metadata from DOCX docProps/core.xml.
pub fn extract_docx_metadata(bytes: &[u8]) -> DocumentMetadata {
    let cursor = Cursor::new(bytes);
    let mut zip = match ZipArchive::new(cursor) {
        Ok(z) => z,
        Err(_) => return DocumentMetadata::default(),
    };

    let mut core_xml = String::new();
    if let Ok(mut file) = zip.by_name("docProps/core.xml") {
        let max_aux = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
        let _ = (&mut file).take(max_aux + 1).read_to_string(&mut core_xml);
        if core_xml.len() as u64 > max_aux {
            return DocumentMetadata::default();
        }
    }

    if core_xml.is_empty() {
        return DocumentMetadata::default();
    }

    DocumentMetadata {
        title: extract_tag_value(&core_xml, "dc:title"),
        author: extract_tag_value(&core_xml, "dc:creator"),
        subject: extract_tag_value(&core_xml, "dc:subject"),
        keywords: extract_tag_value(&core_xml, "cp:keywords"),
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Build standard OOXML docProps/core.xml content.
pub fn build_core_properties_xml(metadata: &DocumentMetadata) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    out.push_str("<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
        xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
        xmlns:dcterms=\"http://purl.org/dc/terms/\" \
        xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" \
        xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    if let Some(ref t) = metadata.title {
        out.push_str(&format!("  <dc:title>{}</dc:title>\n", escape_xml(t)));
    }
    if let Some(ref a) = metadata.author {
        out.push_str(&format!("  <dc:creator>{}</dc:creator>\n", escape_xml(a)));
    }
    if let Some(ref s) = metadata.subject {
        out.push_str(&format!("  <dc:subject>{}</dc:subject>\n", escape_xml(s)));
    }
    if let Some(ref k) = metadata.keywords {
        out.push_str(&format!("  <cp:keywords>{}</cp:keywords>\n", escape_xml(k)));
    }

    out.push_str("</cp:coreProperties>");
    out
}

/// Inject or update metadata in DOCX bytes in memory.
pub fn inject_docx_metadata(
    docx_bytes: &[u8],
    metadata: &DocumentMetadata,
) -> Result<Vec<u8>, ConversionError> {
    if metadata.is_empty() {
        return Ok(docx_bytes.to_vec());
    }

    let existing = extract_docx_metadata(docx_bytes);
    let merged = existing.merge_with_overrides(metadata);
    let new_core_xml = build_core_properties_xml(&merged);

    let cursor = Cursor::new(docx_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupted DOCX zip when injecting metadata: {}", e),
    })?;

    let mut out_buf = Cursor::new(Vec::with_capacity(docx_bytes.len() + 1024));
    let mut zip_writer = ZipWriter::new(&mut out_buf);
    let opt = SimpleFileOptions::default();

    let mut replaced_core = false;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| ConversionError {
            kind: ConversionErrorKind::CorruptInput,
            message: format!("Error reading zip entry {}: {}", i, e),
        })?;
        let name = file.name().to_string();

        if name == "docProps/core.xml" {
            zip_writer
                .start_file(&name, opt)
                .map_err(|e| ConversionError {
                    kind: ConversionErrorKind::Internal,
                    message: format!("Error writing core.xml: {}", e),
                })?;
            zip_writer
                .write_all(new_core_xml.as_bytes())
                .map_err(|e| ConversionError {
                    kind: ConversionErrorKind::Internal,
                    message: format!("Error writing core.xml: {}", e),
                })?;
            replaced_core = true;
            continue;
        }

        if name == "[Content_Types].xml" || name == "_rels/.rels" {
            let max_aux = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
            let mut data = Vec::new();
            (&mut file)
                .take(max_aux + 1)
                .read_to_end(&mut data)
                .map_err(|e| ConversionError {
                    kind: ConversionErrorKind::CorruptInput,
                    message: format!("Error reading content of {}: {}", name, e),
                })?;
            if data.len() as u64 > max_aux {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!("XML entry '{}' exceeds auxiliary size limit", name),
                });
            }

            if name == "[Content_Types].xml" {
                let mut ct_str = String::from_utf8_lossy(&data).to_string();
                if !ct_str.contains("docProps/core.xml") {
                    if let Some(end_pos) = ct_str.find("</Types>") {
                        ct_str.insert_str(
                            end_pos,
                            "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\n",
                        );
                        data = ct_str.into_bytes();
                    }
                }
            } else {
                let mut rel_str = String::from_utf8_lossy(&data).to_string();
                if !rel_str.contains("docProps/core.xml") {
                    if let Some(end_pos) = rel_str.find("</Relationships>") {
                        rel_str.insert_str(
                            end_pos,
                            "<Relationship Id=\"rIdCoreMeta\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\n",
                        );
                        data = rel_str.into_bytes();
                    }
                }
            }

            zip_writer
                .start_file(&name, opt)
                .map_err(|e| ConversionError {
                    kind: ConversionErrorKind::Internal,
                    message: format!("Error writing entry {}: {}", name, e),
                })?;
            zip_writer.write_all(&data).map_err(|e| ConversionError {
                kind: ConversionErrorKind::Internal,
                message: format!("Error writing data for {}: {}", name, e),
            })?;
            continue;
        }

        zip_writer
            .raw_copy_file(file)
            .map_err(|e| ConversionError {
                kind: ConversionErrorKind::Internal,
                message: format!("Error raw-copying entry {}: {}", name, e),
            })?;
    }

    if !replaced_core {
        zip_writer
            .start_file("docProps/core.xml", opt)
            .map_err(|e| ConversionError {
                kind: ConversionErrorKind::Internal,
                message: format!("Error appending core.xml: {}", e),
            })?;
        zip_writer
            .write_all(new_core_xml.as_bytes())
            .map_err(|e| ConversionError {
                kind: ConversionErrorKind::Internal,
                message: format!("Error writing core.xml: {}", e),
            })?;
    }

    zip_writer.finish().map_err(|e| ConversionError {
        kind: ConversionErrorKind::Internal,
        message: format!("Error finalizing zip: {}", e),
    })?;

    Ok(out_buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docx_metadata_roundtrip() {
        let meta = DocumentMetadata {
            title: Some("Docx Title".to_string()),
            author: Some("Docx Author".to_string()),
            subject: Some("Docx Subject".to_string()),
            keywords: Some("k1, k2".to_string()),
        };
        let xml = build_core_properties_xml(&meta);
        assert!(xml.contains("<dc:title>Docx Title</dc:title>"));
        assert!(xml.contains("<dc:creator>Docx Author</dc:creator>"));

        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buf);
            let opt = SimpleFileOptions::default();
            writer.start_file("docProps/core.xml", opt).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }

        let extracted = extract_docx_metadata(&buf.into_inner());
        assert_eq!(extracted.title.as_deref(), Some("Docx Title"));
        assert_eq!(extracted.author.as_deref(), Some("Docx Author"));
        assert_eq!(extracted.subject.as_deref(), Some("Docx Subject"));
        assert_eq!(extracted.keywords.as_deref(), Some("k1, k2"));
    }

    #[test]
    fn test_extract_tag_value_unescapes_and_skips_self_closing() {
        let xml = "<cp:coreProperties><dc:title/><dc:creator>Alice &amp; Bob</dc:creator></cp:coreProperties>";
        assert_eq!(extract_tag_value(xml, "dc:title"), None);
        assert_eq!(
            extract_tag_value(xml, "dc:creator").as_deref(),
            Some("Alice & Bob")
        );
    }
}
