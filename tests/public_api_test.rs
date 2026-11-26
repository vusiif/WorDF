//! WorDF public byte-level API consumer test.
//! Demonstrates usage from an external Rust crate calling `wordf::convert`.
//! License: GPL-3.0-or-later

use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use wordf::{convert, ConversionErrorKind, ConversionOptions, InputFormat};

fn make_test_docx(text: &str) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut buf);
        let opt = SimpleFileOptions::default();

        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
            )
            .unwrap();

        writer.start_file("_rels/.rels", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
            )
            .unwrap();

        writer.start_file("word/document.xml", opt).unwrap();
        let doc_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>{}</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
            text
        );
        writer.write_all(doc_xml.as_bytes()).unwrap();

        writer.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn test_consumer_convert_valid_docx_bytes() {
    let docx = make_test_docx("Hello WorDF Public API Consumer");
    let options = ConversionOptions::default();

    let result = convert(&docx, &options).expect("Conversion of valid DOCX should succeed");

    assert!(!result.pdf.is_empty(), "PDF bytes must not be empty");
    assert!(
        result.pdf.starts_with(b"%PDF-"),
        "PDF must start with %PDF- header"
    );
    assert!(
        result.page_count >= 1,
        "Real page count must be >= 1, got {}",
        result.page_count
    );
}

#[test]
fn test_consumer_reject_invalid_magic_bytes() {
    let invalid_bytes = b"This is a plain text file, not a Word document.";
    let options = ConversionOptions::default();

    let err = convert(invalid_bytes, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::UnsupportedFormat);
}

#[test]
fn test_consumer_reject_rtf_format() {
    let rtf_bytes =
        b"{\\rtf1\\ansi\\deff0 {\\fonttbl {\\f0 Courier;}} \\f0\\fs20 Hello Word!\\par}";
    let options = ConversionOptions::default();

    let err = convert(rtf_bytes, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::UnsupportedFormat);
}

#[test]
fn test_consumer_reject_html_format() {
    let html_bytes = b"<!DOCTYPE html><html><body><h1>Document</h1></body></html>";
    let options = ConversionOptions::default();

    let err = convert(html_bytes, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::UnsupportedFormat);
}

#[test]
fn test_consumer_reject_encrypted_package() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut buf);
        let opt = SimpleFileOptions::default();
        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer.write_all(b"<Types/>").unwrap();
        writer.start_file("EncryptedPackage", opt).unwrap();
        writer.write_all(b"encrypted data stream").unwrap();
        writer.finish().unwrap();
    }
    let encrypted_docx = buf.into_inner();

    let options = ConversionOptions::default();
    let err = convert(&encrypted_docx, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::Encrypted);
}

#[test]
fn test_consumer_reject_encrypted_cfb_package() {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut comp = cfb::CompoundFile::create(&mut cursor).unwrap();
        let mut stream = comp.create_new_stream("/EncryptedPackage").unwrap();
        stream.write_all(b"encrypted office data stream").unwrap();
    }
    let encrypted_cfb = cursor.into_inner();

    let options = ConversionOptions::default();
    let err = convert(&encrypted_cfb, &options).unwrap_err();
    assert_eq!(
        err.kind,
        ConversionErrorKind::Encrypted,
        "CFB container with EncryptedPackage stream must be classified as Encrypted (exit code 3)"
    );
}

#[test]
fn test_consumer_reject_oversized_input() {
    // 129 MiB dummy input
    let fake_oversized = vec![0u8; 129 * 1024 * 1024];
    let options = ConversionOptions::default();

    let err = convert(&fake_oversized, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::ResourceLimit);
}

#[test]
fn test_consumer_format_mismatch() {
    let docx = make_test_docx("Format mismatch test");
    let options = ConversionOptions {
        format: InputFormat::Doc, // Expect DOC but provide DOCX
        ..Default::default()
    };

    let err = convert(&docx, &options).unwrap_err();
    assert_eq!(err.kind, ConversionErrorKind::UnsupportedFormat);
}

#[test]
fn test_consumer_reject_generic_zip_unsupported_format() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut buf);
        let opt = SimpleFileOptions::default();
        writer.start_file("hello.txt", opt).unwrap();
        writer.write_all(b"Hello world from generic zip").unwrap();
        writer.finish().unwrap();
    }
    let generic_zip = buf.into_inner();
    let options = ConversionOptions::default();

    let err = convert(&generic_zip, &options).unwrap_err();
    assert_eq!(
        err.kind,
        ConversionErrorKind::UnsupportedFormat,
        "Structurally valid generic ZIP without Office features must return UnsupportedFormat (exit code 3)"
    );
}

#[test]
fn test_consumer_reject_xlsx_unsupported_format() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut buf);
        let opt = SimpleFileOptions::default();
        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#,
            )
            .unwrap();

        writer.start_file("_rels/.rels", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            )
            .unwrap();

        writer.start_file("xl/workbook.xml", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheets/></workbook>"#,
            )
            .unwrap();

        writer.finish().unwrap();
    }
    let xlsx_bytes = buf.into_inner();
    let options = ConversionOptions::default();

    let err = convert(&xlsx_bytes, &options).unwrap_err();
    assert_eq!(
        err.kind,
        ConversionErrorKind::UnsupportedFormat,
        "Non-Word OOXML packages (XLSX) must return UnsupportedFormat (exit code 3)"
    );
}

#[test]
fn test_consumer_reject_garbage_zip_corrupt_input() {
    let garbage_zip = b"PK\x03\x04\x14\x00\x00\x00\x08\x00invalid_garbage_corrupt_data";
    let options = ConversionOptions::default();

    let err = convert(garbage_zip, &options).unwrap_err();
    assert_eq!(
        err.kind,
        ConversionErrorKind::CorruptInput,
        "Corrupted/unparseable ZIP archive must maintain CorruptInput (exit code 5)"
    );
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_consumer_convert_chinese_docx_with_default_embedded_font() {
    let docx = make_test_docx("你好世界，独立离线中文字体方案测试。");
    // Completely default options: no fallback font name specified, no system font path.
    let options = ConversionOptions::default();

    let result = convert(&docx, &options)
        .expect("Conversion of CJK DOCX with default embedded font must succeed");

    assert!(!result.pdf.is_empty(), "PDF bytes must not be empty");
    assert!(
        result.pdf.starts_with(b"%PDF-"),
        "PDF must start with %PDF- header"
    );
    assert!(
        result.page_count >= 1,
        "Real page count must be >= 1, got {}",
        result.page_count
    );

    // Verify diagnostic warning was emitted for CJK fallback mapping to the embedded font
    let has_font_fallback = result
        .warnings
        .iter()
        .any(|w| w.code == "font.fallback" && w.message.contains(wordf::EMBEDDED_CJK_FONT_NAME));
    assert!(
        has_font_fallback,
        "Warnings must contain font.fallback diagnostic referencing '{}', got: {:?}",
        wordf::EMBEDDED_CJK_FONT_NAME,
        result.warnings
    );
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_consumer_embedded_font_cmap_coverage_and_fail_fast() {
    let font_bytes = wordf::embedded_cjk_font_bytes();
    assert!(
        !font_bytes.is_empty(),
        "Embedded font bytes must not be empty"
    );

    let cmap =
        wordf::FontCmap::from_bytes(font_bytes).expect("Embedded font cmap parsing must succeed");

    // Standard characters must be covered
    let common_chars = [
        '你', '好', '世', '界', '中', '文', '测', '试', '牢', '一', '桥', '梁', '栏', '杆', '固',
    ];
    for &ch in &common_chars {
        assert!(
            cmap.has_glyph(ch, font_bytes),
            "Embedded font must have glyph for '{}' (U+{:04X})",
            ch,
            ch as u32
        );
    }

    // A character outside the font's coverage must fail fast during DOCX conversion
    let unmapped_char = '\u{2A6D0}';
    assert!(!cmap.has_glyph(unmapped_char, font_bytes));

    let docx_unmapped = make_test_docx(&format!("正常文本与生僻字{}", unmapped_char));
    let options = ConversionOptions::default();

    let err = convert(&docx_unmapped, &options)
        .expect_err("Unmapped CJK character must fail fast with MissingFont");
    assert_eq!(err.kind, ConversionErrorKind::MissingFont);
    assert!(
        err.message.contains("Font diagnostic failure"),
        "Message: {}",
        err.message
    );
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_consumer_embedded_font_family_names() {
    let font_bytes = wordf::embedded_cjk_font_bytes();
    let names = wordf::font::extract_font_family_names(font_bytes);
    assert!(
        names.contains(&wordf::EMBEDDED_CJK_FONT_NAME.to_string()),
        "Extracted names {:?} must contain '{}'",
        names,
        wordf::EMBEDDED_CJK_FONT_NAME
    );
}

#[test]
#[cfg(not(feature = "embedded-cjk-font"))]
fn test_consumer_no_embedded_font_cjk_fails_with_missing_font() {
    let docx = make_test_docx("你好世界，无内置字体测试。");
    let options = ConversionOptions::default();
    let err = convert(&docx, &options)
        .expect_err("Conversion of CJK DOCX without embedded font must fail with MissingFont");
    assert_eq!(err.kind, ConversionErrorKind::MissingFont);
}

#[test]
fn test_consumer_convert_binary_doc_route_b_sample() {
    let sample_candidates = [
        ".sandbox-test/cargo-home/registry/src/index.crates.io-1949cf8c6b5b557f/rwml-0.1.4/corpus/public/benchmark/sample/floating_text_bearing.doc",
    ];

    let Some(doc_path) = sample_candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    else {
        return;
    };

    let doc_bytes = std::fs::read(doc_path).expect("Failed to read sample DOC file");
    let options = ConversionOptions::default();

    let result = convert(&doc_bytes, &options).expect("Conversion of binary DOC file must succeed");
    assert!(!result.pdf.is_empty(), "PDF bytes must not be empty");
    assert!(
        result.pdf.starts_with(b"%PDF-"),
        "PDF must start with %PDF- header"
    );
    assert!(result.page_count >= 1, "Page count must be >= 1");
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.code == "layout.approximate"),
        "Route B must produce layout.approximate warning diagnostic"
    );
}

#[test]
fn test_consumer_pages_slice_export_api() {
    let docx = make_test_docx("Page 1 text");
    // 1. Valid page 0 (1-based page 1)
    let opts = ConversionOptions {
        pages: Some(vec![0]),
        ..Default::default()
    };
    let res = convert(&docx, &opts).expect("Slice export of page 0 must succeed");
    assert_eq!(res.page_count, 1);
    assert!(res.pdf.starts_with(b"%PDF-"));

    // 2. Out of bounds page index
    let opts_oob = ConversionOptions {
        pages: Some(vec![5]),
        ..Default::default()
    };
    let err = convert(&docx, &opts_oob).expect_err("Out of bounds page must return error");
    assert_eq!(err.kind, ConversionErrorKind::PageOutOfRange);
}

#[test]
fn test_consumer_encrypted_docx_roundtrip_api() {
    let docx = make_test_docx("Secret Document Protected Content");
    let password = "TopSecretPassword2026!";
    let encrypted_cfb = wordf::crypto::agile::build_agile_encrypted_cfb_for_test(&docx, password);

    // 1. Missing password -> must return Encrypted error (exit code 3)
    let opts_no_pwd = ConversionOptions::default();
    let err_no_pwd = convert(&encrypted_cfb, &opts_no_pwd)
        .expect_err("Encrypted doc without password must fail");
    assert_eq!(err_no_pwd.kind, ConversionErrorKind::Encrypted);
    assert_eq!(err_no_pwd.kind.exit_code(), 3);
    assert!(err_no_pwd.message.contains("password-protected"));

    // 2. Wrong password -> must return Encrypted error with incorrect password message
    let opts_wrong = ConversionOptions {
        password: Some("WrongPassword999".to_string()),
        ..Default::default()
    };
    let err_wrong = convert(&encrypted_cfb, &opts_wrong)
        .expect_err("Encrypted doc with wrong password must fail");
    assert_eq!(err_wrong.kind, ConversionErrorKind::Encrypted);
    assert_eq!(err_wrong.kind.exit_code(), 3);
    assert!(err_wrong.message.contains("Incorrect password"));

    // 3. Correct password -> must succeed and yield valid PDF
    let opts_correct = ConversionOptions {
        password: Some(password.to_string()),
        ..Default::default()
    };
    let res = convert(&encrypted_cfb, &opts_correct)
        .expect("Decryption and conversion with correct password must succeed");
    assert!(!res.pdf.is_empty());
    assert!(res.pdf.starts_with(b"%PDF-"));
    assert!(res.page_count >= 1);

    // 4. Document analysis with password
    let analysis_res = wordf::analyze_document(&encrypted_cfb, &opts_correct)
        .expect("Analysis with correct password must succeed");
    assert_eq!(analysis_res.format_name, "OOXML DOCX");
    assert_eq!(analysis_res.paragraph_count, 1);
}

#[test]
fn test_consumer_metadata_injection_and_pdf_info() {
    let docx = make_test_docx("Metadata Test Document Content");
    let opts = ConversionOptions {
        metadata: Some(wordf::DocumentMetadata {
            title: Some("Annual Financial Report 2026".to_string()),
            author: Some("Finance Department".to_string()),
            subject: Some("Q1 Earnings".to_string()),
            keywords: Some("finance,earnings".to_string()),
        }),
        ..Default::default()
    };

    let result = convert(&docx, &opts).expect("Conversion with metadata must succeed");
    assert!(!result.pdf.is_empty());
    assert!(result.pdf.starts_with(b"%PDF-"));

    let pdf_str = String::from_utf8_lossy(&result.pdf);
    // Info dictionary and XMP metadata checks
    assert!(
        pdf_str.contains("Annual Financial Report 2026"),
        "PDF must embed custom Title"
    );
    assert!(
        pdf_str.contains("Finance Department"),
        "PDF must embed custom Author/Creator"
    );
    assert!(
        pdf_str.contains("Q1 Earnings"),
        "PDF must embed custom Subject"
    );
    assert!(
        pdf_str.contains("finance,earnings"),
        "PDF must embed custom Keywords"
    );

    // Analysis mode check
    let analysis =
        wordf::analyze_document(&docx, &opts).expect("Analyze with metadata must succeed");
    assert_eq!(
        analysis.metadata.title.as_deref(),
        Some("Annual Financial Report 2026")
    );
    assert_eq!(
        analysis.metadata.author.as_deref(),
        Some("Finance Department")
    );
}

#[test]
fn test_conversion_options_debug_redacts_password() {
    let opts = ConversionOptions {
        password: Some("SuperSecret123!".to_string()),
        ..Default::default()
    };
    let debug_str = format!("{:?}", opts);
    assert!(!debug_str.contains("SuperSecret123!"));
    assert!(debug_str.contains("<redacted>"));
}
