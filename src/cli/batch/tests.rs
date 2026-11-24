//! Unit tests for batch directory conversion and recursive discovery.
//! License: GPL-3.0-or-later

use super::*;
use std::fs::File;
use std::path::Path;

#[test]
fn test_batch_rejects_output_filename_collision() {
    let temp_dir = std::env::temp_dir().join(format!(
        "wordf_batch_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    File::create(input_dir.join("report.doc")).unwrap();
    File::create(input_dir.join("report.docx")).unwrap();

    let config = BatchConfig {
        input_dir,
        output_dir,
        report_json: None,
        options: ConversionOptions::default(),
        overwrite: false,
        recursive: false,
        strict: false,
        resume: false,
        jobs: Some(1),
        verbose: false,
        quiet: true,
    };

    let res = run_batch_conversion(&config);
    assert_eq!(
        res,
        Err(2),
        "Batch conversion must reject same-stem output collisions with exit code 2"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_batch_report_json_generation() {
    let temp_dir = std::env::temp_dir().join(format!(
        "wordf_batch_rep_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    let report_json = temp_dir.join("batch_report.json");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    fs::write(input_dir.join("sample.docx"), b"not a valid zip").unwrap();

    let config = BatchConfig {
        input_dir,
        output_dir,
        report_json: Some(report_json.clone()),
        options: ConversionOptions::default(),
        overwrite: true,
        recursive: false,
        strict: false,
        resume: false,
        jobs: Some(1),
        verbose: false,
        quiet: true,
    };

    let res = run_batch_conversion(&config);
    assert_eq!(
        res,
        Err(1),
        "Batch conversion with failed item must return Err(1)"
    );
    assert!(
        report_json.exists(),
        "Batch report JSON must be created even when items fail"
    );

    let rep_content = fs::read_to_string(&report_json).unwrap();
    assert!(rep_content.contains(r#""mode": "batch""#));
    assert!(rep_content.contains(r#""total": 1"#));
    assert!(rep_content.contains(r#""failed": 1"#));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_batch_recursive_discovery_and_mirroring() {
    let temp_dir = std::env::temp_dir().join(format!(
        "wordf_rec_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input_dir = temp_dir.join("input");
    let sub_dir = input_dir.join("sub").join("nested");
    fs::create_dir_all(&sub_dir).unwrap();

    let doc1 = input_dir.join("root.docx");
    let doc2 = sub_dir.join("deep.docx");
    fs::write(&doc1, b"root").unwrap();
    fs::write(&doc2, b"deep").unwrap();

    let non_recursive_files = discover_word_documents_ext(&input_dir, false).unwrap();
    assert_eq!(
        non_recursive_files.len(),
        1,
        "Non-recursive must only find root doc"
    );

    let recursive_files = discover_word_documents_ext(&input_dir, true).unwrap();
    assert_eq!(recursive_files.len(), 2, "Recursive must find nested docs");

    let output_base = Path::new("/virtual/output");
    let computed = compute_output_pdf_path(&doc2, &input_dir, output_base).unwrap();
    assert!(
        computed.ends_with(Path::new("sub").join("nested").join("deep.pdf")),
        "Output PDF path must mirror relative subdirectories: {:?}",
        computed
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[cfg(unix)]
#[test]
fn test_compute_output_pdf_path_preserves_non_utf8_stem() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let input_base = Path::new("/in");
    let in_path = Path::new(OsStr::from_bytes(b"/in/bad\xffname.docx"));
    let out = compute_output_pdf_path(in_path, input_base, Path::new("/out"))
        .expect("Non-UTF-8 stems must still map to an output path");
    assert_eq!(
        out,
        Path::new(OsStr::from_bytes(b"/out/bad\xffname.pdf")),
        "Output PDF must preserve the non-UTF-8 stem byte-for-byte"
    );
}

#[test]
fn test_batch_resume_skips_up_to_date_output() {
    let temp_dir = std::env::temp_dir().join(format!(
        "wordf_resume_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    let doc_path = input_dir.join("test.docx");
    let pdf_path = output_dir.join("test.pdf");
    fs::write(&doc_path, b"dummy docx bytes").unwrap();

    let config = BatchConfig {
        input_dir: input_dir.clone(),
        output_dir: output_dir.clone(),
        report_json: None,
        options: ConversionOptions::default(),
        overwrite: false,
        recursive: false,
        strict: false,
        resume: true,
        jobs: Some(1),
        verbose: false,
        quiet: true,
    };
    let res = run_batch_conversion(&config);
    assert_eq!(res, Err(1), "Fails conversion on invalid docx, not skipped");

    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&pdf_path, b"%PDF-1.4 dummy").unwrap();

    let summary =
        run_batch_conversion(&config).expect("Must succeed by skipping up-to-date output");
    assert_eq!(summary.total, 1);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.succeeded, 0);

    let _ = fs::remove_dir_all(&temp_dir);
}
