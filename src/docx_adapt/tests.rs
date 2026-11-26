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

    let (adapted_zip, count) =
        adapt_docx_cjk_in_memory_with_bytes(&input_zip, Some("SimSun"), None, &[])
            .expect("Adaptation must succeed");

    assert_eq!(count, 2);

    // Verify media entry was preserved byte-for-byte
    let mut archive = ZipArchive::new(Cursor::new(adapted_zip.as_ref())).unwrap();
    let mut media_file = archive
        .by_name("word/media/image1.png")
        .expect("media entry must exist");
    let mut extracted = Vec::new();
    media_file.read_to_end(&mut extracted).unwrap();
    assert_eq!(
        extracted, test_payload,
        "Raw-copied binary stream must be 100% identical"
    );
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

    let (adapted_zip, count) =
        adapt_docx_cjk_in_memory_with_bytes(&input_zip, Some("LXGW WenKai Lite"), None, &[])
            .expect("Adaptation must succeed");

    assert_eq!(count, 2);

    let mut archive = ZipArchive::new(Cursor::new(adapted_zip.as_ref())).unwrap();
    let mut styles_file = archive
        .by_name("word/styles.xml")
        .expect("styles.xml must exist");
    let mut styles_xml = String::new();
    styles_file.read_to_string(&mut styles_xml).unwrap();

    assert!(
        styles_xml.contains(r#"w:ascii="Calibri""#),
        "West-European font must be preserved"
    );
    assert!(
        styles_xml.contains(r#"w:eastAsia="LXGW WenKai Lite""#),
        "East Asian fallback font must be injected into docDefaults"
    );
}

#[test]
fn test_single_pass_cjk_and_metadata_injection() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut buf);
        let opt = SimpleFileOptions::default();
        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer
            .write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>"#)
            .unwrap();
        writer.start_file("_rels/.rels", opt).unwrap();
        writer
            .write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#)
            .unwrap();
        writer.start_file("word/document.xml", opt).unwrap();
        writer
            .write_all(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>测试</w:t></w:r></w:p></w:body></w:document>"#.as_bytes())
            .unwrap();
        writer.finish().unwrap();
    }
    let input_zip = buf.into_inner();
    let resolver = crate::font::FontResolver::new(
        crate::font::FontFallbackChain::new(),
        Vec::new(),
        Some("SimSun".to_string()),
        None,
    );
    let meta = crate::DocumentMetadata {
        title: Some("Single Pass".to_string()),
        ..Default::default()
    };

    let (adapted_zip, count) =
        adapt_docx_with_resolver_and_metadata(&input_zip, &resolver, Some(&meta))
            .expect("Single-pass adapt+metadata must succeed");
    assert_eq!(count, 2);
    let extracted = crate::metadata::docx::extract_docx_metadata(adapted_zip.as_ref());
    assert_eq!(extracted.title.as_deref(), Some("Single Pass"));
}
