//! Unit tests for schema-v1 JSON reports.
//! License: GPL-3.0-or-later

use super::*;
use crate::analyze::DocumentAnalysis;

#[test]
fn test_format_analysis_json() {
    let analysis = DocumentAnalysis {
        format_name: "OOXML DOCX".to_string(),
        input_bytes: 4096,
        paragraph_count: 10,
        table_count: 2,
        run_count: 25,
        media_count: 1,
        media_bytes: 512,
        header_footer_count: 2,
        cjk_char_count: 150,
        detected_fonts: vec!["SimSun".to_string(), "Calibri".to_string()],
        missing_glyphs: vec!['"'],
        metadata: crate::metadata::DocumentMetadata::default(),
        warnings: vec![],
    };

    let json = format_analysis_json(&analysis);
    assert!(json.contains(r#""mode": "analyze""#));
    assert!(json.contains(r#""format": "OOXML DOCX""#));
    assert!(json.contains(r#""paragraphs": 10"#));
    assert!(json.contains(r#""cjk_characters": 150"#));
    assert!(json.contains(r#""status": "ok""#));
    assert!(json.contains(r#"\""#));
}

#[test]
fn test_format_batch_report_json() {
    let items = vec![
        BatchItemReport {
            input_path: "input/test1.docx".to_string(),
            output_path: "output/test1.pdf".to_string(),
            status: "succeeded".to_string(),
            page_count: 3,
            elapsed_ms: 12.5,
            error: None,
        },
        BatchItemReport {
            input_path: "input/test2.doc".to_string(),
            output_path: "output/test2.pdf".to_string(),
            status: "failed".to_string(),
            page_count: 0,
            elapsed_ms: 5.0,
            error: Some("CorruptInput: Invalid FIB".to_string()),
        },
    ];

    let batch_data = BatchReportData {
        tool_name: "wordf",
        tool_version: "1.0.0",
        total: 2,
        succeeded: 1,
        failed: 1,
        skipped: 0,
        elapsed_ms: 20.0,
        items: &items,
    };

    let json = format_batch_report_json(&batch_data);
    assert!(json.contains(r#""mode": "batch""#));
    assert!(json.contains(r#""total": 2"#));
    assert!(json.contains(r#""succeeded": 1"#));
    assert!(json.contains(r#""failed": 1"#));
    assert!(json.contains(r#""status": "succeeded""#));
    assert!(json.contains(r#""error": "CorruptInput: Invalid FIB""#));
}
