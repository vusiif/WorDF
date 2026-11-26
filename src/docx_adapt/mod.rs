//! WorDF DOCX XML text parsing and CJK font adaptation.
//! License: GPL-3.0-or-later

pub mod styles;
pub mod xml;
pub mod xml_util;

pub use styles::{adapt_styles_xml, is_styles_part};
pub use xml::{
    adapt_run_xml, adapt_xml_text, adapt_xml_text_with_resolver, count_cjk_in_xml_text_nodes,
    for_each_xml_text_node, is_text_part,
};
pub use xml_util::extract_attribute_value;

use std::borrow::Cow;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

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
    let resolver = crate::font::FontResolver::new(
        crate::font::FontFallbackChain::new(),
        available_fonts.to_vec(),
        fallback_font.map(|s| s.to_string()),
        fallback_font_bytes.map(|b| b.to_vec()),
    );
    adapt_docx_cjk_with_resolver(docx_bytes, &resolver)
}

fn write_zip_entry<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    name: &str,
    data: &[u8],
    options: SimpleFileOptions,
) -> Result<(), ConversionError> {
    writer
        .start_file(name, options)
        .map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to start zip entry '{}': {}", name, e),
        })?;
    writer.write_all(data).map_err(|e| ConversionError {
        kind: ConversionErrorKind::Internal,
        message: format!("Failed to write zip entry '{}': {}", name, e),
    })
}

pub fn adapt_docx_cjk_with_resolver<'a>(
    docx_bytes: &'a [u8],
    resolver: &crate::font::FontResolver,
) -> Result<(Cow<'a, [u8]>, usize), ConversionError> {
    adapt_docx_with_resolver_and_metadata(docx_bytes, resolver, None)
}

pub fn adapt_docx_with_resolver_and_metadata<'a>(
    docx_bytes: &'a [u8],
    resolver: &crate::font::FontResolver,
    metadata: Option<&crate::DocumentMetadata>,
) -> Result<(Cow<'a, [u8]>, usize), ConversionError> {
    let reader = Cursor::new(docx_bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| ConversionError {
        kind: ConversionErrorKind::CorruptInput,
        message: format!("Corrupt DOCX package: {}", e),
    })?;

    let core_xml = metadata.filter(|m| !m.is_empty()).map(|meta| {
        let merged =
            crate::metadata::docx::extract_docx_metadata(docx_bytes).merge_with_overrides(meta);
        crate::metadata::docx::build_core_properties_xml(&merged)
    });
    let mut replaced_core = false;

    let mut out_buf = Cursor::new(Vec::with_capacity(docx_bytes.len()));
    let mut writer = ZipWriter::new(&mut out_buf);
    let mut total_cjk_in_package = 0;
    let mut modified_entries = 0;

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
            handle
                .read_to_string(&mut xml_content)
                .map_err(|e| ConversionError {
                    kind: ConversionErrorKind::CorruptInput,
                    message: format!("Failed to read XML text part '{}': {}", name, e),
                })?;
            if xml_content.len() as u64 > max_bytes {
                return Err(ConversionError {
                    kind: ConversionErrorKind::ResourceLimit,
                    message: format!("XML text part '{}' decompressed size exceeds limit", name),
                });
            }

            // Glyph coverage fail-fast check using unified FontResolver
            if resolver.has_fallback() {
                let missing_chars = resolver.find_missing_glyphs(&xml_content);
                if !missing_chars.is_empty() {
                    let missing_summary: Vec<String> = missing_chars
                        .iter()
                        .map(|&c| format!("'{}' (U+{:04X})", c, c as u32))
                        .collect();
                    let font_desc = resolver
                        .primary_fallback_name
                        .as_deref()
                        .unwrap_or("fallback font chain");
                    return Err(ConversionError {
                        kind: ConversionErrorKind::MissingFont,
                        message: format!(
                            "Font diagnostic failure: fallback font '{}' lacks glyphs for {} East Asian characters in '{}': [{}]",
                            font_desc,
                            missing_chars.len(),
                            name,
                            missing_summary.join(", ")
                        ),
                    });
                }
            }

            let (adapted_xml, cjk_count) = adapt_xml_text_with_resolver(&xml_content, resolver);
            total_cjk_in_package += cjk_count;

            if adapted_xml != xml_content {
                let options = SimpleFileOptions::default()
                    .compression_method(file.compression())
                    .unix_permissions(file.unix_mode().unwrap_or(0o644));
                write_zip_entry(&mut writer, &name, adapted_xml.as_bytes(), options)?;
                modified_entries += 1;
                continue;
            }
            drop(file);
            let fresh = archive.by_index(i).map_err(|e| ConversionError {
                kind: ConversionErrorKind::CorruptInput,
                message: format!("Corrupt zip entry: {}", e),
            })?;
            writer.raw_copy_file(fresh).map_err(|e| ConversionError {
                kind: ConversionErrorKind::Internal,
                message: format!("Failed to raw-copy entry '{}': {}", name, e),
            })?;
            continue;
        }

        if is_styles_part(&name) {
            if let Some(fb) = resolver.primary_fallback_name.as_deref() {
                let max_bytes = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
                let mut xml_content =
                    String::with_capacity(uncompressed_size.min(max_bytes as usize));
                let mut handle = (&mut file).take(max_bytes + 1);
                if handle.read_to_string(&mut xml_content).is_ok()
                    && xml_content.len() as u64 <= max_bytes
                {
                    let adapted_styles =
                        adapt_styles_xml(&xml_content, fb, &resolver.available_fonts);
                    if adapted_styles != xml_content {
                        let options = SimpleFileOptions::default()
                            .compression_method(file.compression())
                            .unix_permissions(file.unix_mode().unwrap_or(0o644));
                        write_zip_entry(&mut writer, &name, adapted_styles.as_bytes(), options)?;
                        modified_entries += 1;
                        continue;
                    }
                }
                drop(file);
                let fresh = archive.by_index(i).map_err(|e| ConversionError {
                    kind: ConversionErrorKind::CorruptInput,
                    message: format!("Corrupt zip entry: {}", e),
                })?;
                writer.raw_copy_file(fresh).map_err(|e| ConversionError {
                    kind: ConversionErrorKind::Internal,
                    message: format!("Failed to raw-copy entry '{}': {}", name, e),
                })?;
                continue;
            }
        }

        if let Some(ref new_core) = core_xml {
            if name == "docProps/core.xml" {
                write_zip_entry(
                    &mut writer,
                    &name,
                    new_core.as_bytes(),
                    SimpleFileOptions::default(),
                )?;
                replaced_core = true;
                modified_entries += 1;
                continue;
            }
            if name == "[Content_Types].xml" || name == "_rels/.rels" {
                let max_aux = crate::format::docx::MAX_DOCX_AUX_XML_BYTES;
                let mut data = Vec::new();
                if (&mut file).take(max_aux + 1).read_to_end(&mut data).is_ok()
                    && data.len() as u64 <= max_aux
                {
                    let patched = crate::metadata::docx::patch_core_manifest_entry(&name, data);
                    write_zip_entry(&mut writer, &name, &patched, SimpleFileOptions::default())?;
                    modified_entries += 1;
                    continue;
                }
            }
        }

        // Raw-copy unmodified entry without decompression or re-compression
        writer.raw_copy_file(file).map_err(|e| ConversionError {
            kind: ConversionErrorKind::Internal,
            message: format!("Failed to raw-copy entry '{}': {}", name, e),
        })?;
    }

    if let Some(ref new_core) = core_xml {
        if !replaced_core {
            write_zip_entry(
                &mut writer,
                "docProps/core.xml",
                new_core.as_bytes(),
                SimpleFileOptions::default(),
            )?;
            modified_entries += 1;
        }
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
mod tests;
