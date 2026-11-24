//! Historical defect reproduction and XML/Font regression integration tests.
//! License: GPL-3.0-or-later

use std::io::Write;
use wordf::docx_adapt::{
    adapt_docx_cjk_in_memory, adapt_xml_text, count_cjk_in_xml_text_nodes, for_each_xml_text_node,
};
use wordf::font::is_cjk_char;
#[cfg(feature = "embedded-cjk-font")]
use wordf::font::FontCmap;
#[cfg(feature = "embedded-cjk-font")]
use wordf::{adapt_docx_cjk_in_memory_with_bytes, EMBEDDED_CJK_FONT_BYTES, EMBEDDED_CJK_FONT_NAME};

pub fn make_minimal_docx(text: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        let opt = zip::write::SimpleFileOptions::default();
        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            )
            .unwrap();
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
    assert_eq!(
        east_asia_count, 29434,
        "East Asian count must be exactly 29,434"
    );
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_defect_1_font_cmap_parsing_and_lookup() {
    let font_candidates = [
        ".sandbox-test/fonts/方正黑体简体.TTF",
        "/output/fonts/方正黑体简体.TTF",
    ];
    let font_bytes = font_candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| std::fs::read(p).unwrap())
        .unwrap_or_else(|| EMBEDDED_CJK_FONT_BYTES.to_vec());
    let cmap = FontCmap::from_bytes(&font_bytes).expect("Failed to parse font cmap");

    assert!(
        cmap.has_glyph('牢', &font_bytes),
        "牢 (U+7262) must have glyph"
    );
    assert!(
        cmap.has_glyph('一', &font_bytes),
        "一 (U+4E00) must have glyph"
    );
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_defect_1_missing_glyph_fails_fast_with_diagnostic() {
    let (font_name, font_bytes, unmapped_char) = if let Some(p) = [
        ".sandbox-test/fonts/方正黑体简体.TTF",
        "/output/fonts/方正黑体简体.TTF",
    ]
    .iter()
    .find(|p| std::path::Path::new(p).exists())
    {
        ("方正黑体简体", std::fs::read(p).unwrap(), '窂')
    } else {
        (
            EMBEDDED_CJK_FONT_NAME,
            EMBEDDED_CJK_FONT_BYTES.to_vec(),
            '\u{2A6D0}',
        )
    };

    let docx_missing = make_minimal_docx(&format!("桥梁栏杆或护栏{}固", unmapped_char));
    let res_err =
        adapt_docx_cjk_in_memory_with_bytes(&docx_missing, Some(font_name), Some(&font_bytes), &[]);
    assert!(res_err.is_err(), "Must fail fast when font lacks glyph");
    let err_msg = res_err.unwrap_err();
    assert!(
        err_msg.contains("Font diagnostic failure"),
        "Message: {}",
        err_msg
    );
    assert!(
        err_msg.contains("lacks glyphs for 1 East Asian characters"),
        "Message: {}",
        err_msg
    );

    let docx_covered = make_minimal_docx("桥梁栏杆或护栏牢固");
    let res_ok =
        adapt_docx_cjk_in_memory_with_bytes(&docx_covered, Some(font_name), Some(&font_bytes), &[]);
    assert!(res_ok.is_ok(), "Must succeed when all glyphs covered");
}

#[test]
fn test_defect_self_closing_run_not_greedy() {
    let xml = r#"<w:p><w:r/><w:r><w:t>正文</w:t></w:r></w:p>"#;
    let (adapted, count) = adapt_xml_text(xml, "SimSun", &[]);
    assert_eq!(count, 2, "Expected 2 CJK characters, got {}", count);
    assert!(adapted.starts_with("<w:p><w:r/>"), "Adapted: {}", adapted);
    assert!(
        adapted.contains(
            r#"<w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>正文</w:t></w:r>"#
        ),
        "Adapted: {}",
        adapted
    );

    let xml_attr = r#"<w:p><w:r rsidR="001234"/><w:r /><w:r><w:t>测试</w:t></w:r></w:p>"#;
    let (adapted_attr, count_attr) = adapt_xml_text(xml_attr, "SimSun", &[]);
    assert_eq!(count_attr, 2);
    assert!(adapted_attr.contains(r#"<w:r rsidR="001234"/>"#));
    assert!(adapted_attr.contains(r#"<w:r />"#));
    assert!(
        adapted_attr.contains(r#"<w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>测试</w:t>"#)
    );

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
        adapted.contains(
            r#"<w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>中文正文</w:t></w:r>"#
        ),
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
