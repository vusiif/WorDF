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
    let rtf_bytes = b"{\\rtf1\\ansi\\deff0 {\\fonttbl {\\f0 Courier;}} \\f0\\fs20 Hello Word!\\par}";
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
