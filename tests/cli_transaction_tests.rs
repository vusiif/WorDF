//! WorDF CLI path transaction, alias isolation, and fault injection integration tests.
//! Tests real file system state, hashes, directory contents, and exit codes.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn get_bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wordf"))
}

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

fn setup_test_sandbox(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("wordf_test_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn test_case_1_input_output_alias_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case1");
    let file = dir.join("doc.docx");
    let content = make_test_docx("Alias test");
    fs::write(&file, &content).unwrap();

    // Call wordf doc.docx -o doc.docx
    let status = Command::new(&bin)
        .arg(&file)
        .arg("-o")
        .arg(&file)
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(2),
        "Same input and output path must exit with code 2"
    );
    // Verify file content is unmodified
    assert_eq!(fs::read(&file).unwrap(), content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_2_input_report_alias_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case2");
    let input = dir.join("doc.docx");
    let out_pdf = dir.join("out.pdf");
    let content = make_test_docx("Report alias test");
    fs::write(&input, &content).unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--report-json")
        .arg(&input)
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(2),
        "Report path matching input path must exit with code 2"
    );
    assert_eq!(fs::read(&input).unwrap(), content);
    assert!(!out_pdf.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_3_output_exists_without_overwrite_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case3");
    let input = dir.join("doc.docx");
    let out_pdf = dir.join("out.pdf");
    fs::write(&input, make_test_docx("No clobber test")).unwrap();

    let existing_bytes = b"existing pristine file content";
    fs::write(&out_pdf, existing_bytes).unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(4),
        "Existing output without --overwrite must exit with code 4"
    );
    assert_eq!(
        fs::read(&out_pdf).unwrap(),
        existing_bytes,
        "Existing output must be untouched"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_4_output_exists_with_overwrite_succeeds() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case4");
    let input = dir.join("doc.docx");
    let out_pdf = dir.join("out.pdf");
    fs::write(&input, make_test_docx("Overwrite test")).unwrap();

    let old_bytes = b"old file content";
    fs::write(&out_pdf, old_bytes).unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--overwrite")
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(0),
        "Existing output with --overwrite must exit with code 0"
    );
    let new_bytes = fs::read(&out_pdf).unwrap();
    assert_ne!(new_bytes, old_bytes);
    assert!(new_bytes.starts_with(b"%PDF-"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[cfg(unix)]
fn test_case_5_symlink_output_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case5");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("Symlink test")).unwrap();

    let real_file = dir.join("real_target.pdf");
    fs::write(&real_file, b"sensitive data").unwrap();

    let symlink_path = dir.join("symlink_out.pdf");
    std::os::unix::fs::symlink(&real_file, &symlink_path).unwrap();

    // Call wordf with symlink target, even with --overwrite
    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&symlink_path)
        .arg("--overwrite")
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(4),
        "Symlink output target must be strictly rejected with exit code 4"
    );
    assert_eq!(
        fs::read(&real_file).unwrap(),
        b"sensitive data",
        "Target pointed by symlink must not be overwritten"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_6_nonexistent_parent_dir_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case6");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("Parent dir test")).unwrap();

    let non_existent_dir = dir.join("missing_subdir");
    let out_pdf = non_existent_dir.join("out.pdf");

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(4),
        "Non-existent parent directory must exit with code 4"
    );
    assert!(
        !non_existent_dir.exists(),
        "Parent directory must NOT be implicitly created"
    );
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr_str.is_empty(),
        "stderr must not be empty when parent directory does not exist"
    );
    assert!(
        stderr_str.contains("Parent directory"),
        "stderr must contain 'Parent directory', got: {}",
        stderr_str
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_7_report_commit_failure_preserves_pdf_returns_9() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case7");
    let input = dir.join("doc.docx");
    let out_pdf = dir.join("out.pdf");
    let report = dir.join("report.json");
    fs::write(&input, make_test_docx("Case 7 test")).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--report-json")
        .arg(&report)
        .env("_WORDF_TEST_FAIL_REPORT_COMMIT", "1")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(9),
        "When PDF is committed but report commit fails, exit code must be strictly 9"
    );

    // Assert: PDF must be successfully committed and valid
    assert!(out_pdf.exists(), "PDF must exist and be committed");
    let pdf_bytes = fs::read(&out_pdf).unwrap();
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "PDF file must be valid PDF"
    );

    // Assert: Report must NOT exist
    assert!(!report.exists(), "Report file must not exist");

    // Assert: stderr must contain output.report_commit_failed
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr_str.contains("output.report_commit_failed"),
        "stderr must contain 'output.report_commit_failed', got: {}",
        stderr_str
    );

    // Assert: No temporary files lingering in parent directory
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        assert!(
            !fname.starts_with(".wordf_tmp_"),
            "Lingering temporary file found after exit 9: {}",
            fname
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_8_future_target_alias_rejected() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case8");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("Case 8 test")).unwrap();

    let collision_target = dir.join("same_name.dat");

    // Both PDF and report point to same non-existent file
    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&collision_target)
        .arg("--report-json")
        .arg(&collision_target)
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(2),
        "Target collision on non-existent targets must exit with code 2"
    );
    assert!(!collision_target.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_9_p0_positional_syntax_rejected() {
    let bin = get_bin_path();
    let status = Command::new(&bin)
        .arg("c")
        .arg("input.docx")
        .arg("output.pdf")
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(2),
        "P0 positional route syntax 'wordf <a|b|c>' must exit with code 2"
    );
}

#[test]
fn test_case_10_no_temp_files_lingering() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case10");
    let input = dir.join("doc.docx");
    let out_pdf = dir.join("out.pdf");
    fs::write(&input, make_test_docx("Temp check")).unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(status.code(), Some(0));

    // Check directory contents: must not contain any .wordf_tmp_*
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        assert!(
            !fname.starts_with(".wordf_tmp_"),
            "Lingering temporary file found: {}",
            fname
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_11_xlsx_and_generic_zip_rejected_code_3() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case11");
    let out_pdf = dir.join("out.pdf");

    // 1. Generic ZIP
    let generic_zip_path = dir.join("plain.zip");
    {
        let mut f = fs::File::create(&generic_zip_path).unwrap();
        let mut writer = ZipWriter::new(&mut f);
        let opt = SimpleFileOptions::default();
        writer.start_file("note.txt", opt).unwrap();
        writer.write_all(b"plain file in zip").unwrap();
        writer.finish().unwrap();
    }
    let status_zip = Command::new(&bin)
        .arg(&generic_zip_path)
        .arg("-o")
        .arg(&out_pdf)
        .status()
        .expect("Failed to execute wordf binary");
    assert_eq!(
        status_zip.code(),
        Some(3),
        "Generic ZIP must be rejected with exit code 3 (UnsupportedFormat)"
    );

    // 2. XLSX package
    let xlsx_path = dir.join("sheet.xlsx");
    {
        let mut f = fs::File::create(&xlsx_path).unwrap();
        let mut writer = ZipWriter::new(&mut f);
        let opt = SimpleFileOptions::default();
        writer.start_file("[Content_Types].xml", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
            )
            .unwrap();
        writer.start_file("_rels/.rels", opt).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            )
            .unwrap();
        writer.start_file("xl/workbook.xml", opt).unwrap();
        writer.write_all(b"<workbook/>").unwrap();
        writer.finish().unwrap();
    }
    let status_xlsx = Command::new(&bin)
        .arg(&xlsx_path)
        .arg("-o")
        .arg(&out_pdf)
        .status()
        .expect("Failed to execute wordf binary");
    assert_eq!(
        status_xlsx.code(),
        Some(3),
        "XLSX package must be rejected with exit code 3 (UnsupportedFormat)"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_12_garbage_zip_rejected_code_5() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case12");
    let corrupt_path = dir.join("corrupted.docx");
    fs::write(
        &corrupt_path,
        b"PK\x03\x04\x14\x00\x00\x00\x08\x00invalid_garbage_corrupt_data",
    )
    .unwrap();
    let out_pdf = dir.join("out.pdf");

    let status = Command::new(&bin)
        .arg(&corrupt_path)
        .arg("-o")
        .arg(&out_pdf)
        .status()
        .expect("Failed to execute wordf binary");
    assert_eq!(
        status.code(),
        Some(5),
        "Corrupted ZIP archive must be rejected with exit code 5 (CorruptInput)"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[cfg(unix)]
fn test_case_13_report_temp_failure_cleans_up_pdf_temp() {
    use std::os::unix::fs::PermissionsExt;
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case13");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("Report failure test")).unwrap();

    let pdf_dir = dir.join("pdf_out");
    fs::create_dir_all(&pdf_dir).unwrap();
    let out_pdf = pdf_dir.join("out.pdf");

    let report_dir = dir.join("report_out");
    fs::create_dir_all(&report_dir).unwrap();
    let out_report = report_dir.join("out.json");

    // Make report_dir read-only so create_exclusive_temp_file fails with EACCES
    fs::set_permissions(&report_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--report-json")
        .arg(&out_report)
        .status()
        .expect("Failed to execute wordf binary");

    // Must exit with code 4
    assert_eq!(
        status.code(),
        Some(4),
        "Report temp creation failure must exit with code 4"
    );

    // Assert zero .wordf_tmp_* files in pdf_dir
    let pdf_entries: Vec<_> = fs::read_dir(&pdf_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".wordf_tmp_"))
        .collect();
    assert!(
        pdf_entries.is_empty(),
        "Lingering temporary file found in PDF directory after report creation failure: {:?}",
        pdf_entries
    );
    assert!(
        !out_pdf.exists(),
        "Final PDF must not be committed on failure"
    );

    // Restore permissions for cleanup
    let _ = fs::set_permissions(&report_dir, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_14_font_path_overrides_preset_env() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case14");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("Font env test")).unwrap();
    let out_pdf = dir.join("out.pdf");
    let dummy_font = dir.join("dummy_font.ttf");
    fs::write(&dummy_font, b"dummy font content").unwrap();

    let status = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--font-path")
        .arg(&dummy_font)
        .env("DOCXIDE_FONTS", "/invalid_nonexistent_font_dir_xyz")
        .status()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        status.code(),
        Some(0),
        "CLI --font-path must override preset invalid DOCXIDE_FONTS and succeed"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_case_15_chinese_docx_cli_default_embedded_font() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case15");
    let input = dir.join("cjk_doc.docx");
    let out_pdf = dir.join("cjk_out.pdf");
    let report_json = dir.join("cjk_report.json");

    fs::write(&input, make_test_docx("中文文档命令行离线转换测试。")).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("--report-json")
        .arg(&report_json)
        .env_remove("DOCXIDE_FONTS")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "CLI conversion with default embedded font must succeed"
    );
    assert!(out_pdf.exists(), "Output PDF must exist");
    let pdf_bytes = fs::read(&out_pdf).unwrap();
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "Output PDF must have valid header"
    );
    assert!(pdf_bytes.len() > 100, "Output PDF must not be truncated");

    assert!(report_json.exists(), "JSON report must exist");
    let rep_content = fs::read_to_string(&report_json).unwrap();
    assert!(rep_content.contains("\"conversion_status\": \"success_with_warnings\""));
    assert!(rep_content.contains("\"fallback_used\": true"));

    // Stderr should contain diagnostic warning
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("font.fallback"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[cfg(feature = "embedded-cjk-font")]
fn test_case_16_chinese_docx_unsupported_glyph_fails_fast_exit_3() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case16");
    let input = dir.join("cjk_unsupported.docx");
    let out_pdf = dir.join("cjk_unsupported_out.pdf");

    let unmapped_char = '\u{2A6D0}';
    fs::write(
        &input,
        make_test_docx(&format!("包含生僻字{}测试", unmapped_char)),
    )
    .unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .env_remove("DOCXIDE_FONTS")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(3),
        "Missing font glyph must fail fast with exit code 3"
    );
    assert!(
        !out_pdf.exists(),
        "Output PDF must not be committed on fail-fast error"
    );

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("Font diagnostic failure"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_17_cli_quiet_suppresses_warnings() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case17_quiet");
    // Extension mismatch triggers a warning
    let input = dir.join("mismatch_name.doc");
    let out_pdf = dir.join("quiet_out.pdf");
    fs::write(&input, make_test_docx("Quiet flag testing")).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("-q")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(output.status.code(), Some(0), "CLI with -q must succeed");
    assert!(out_pdf.exists(), "Output PDF must exist");

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr_str.trim().is_empty(),
        "Quiet mode must suppress all diagnostic warnings on stderr, got: '{}'",
        stderr_str
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_18_cli_verbose_prints_phase_timings() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case18_verbose");
    let input = dir.join("verbose_test.docx");
    let out_pdf = dir.join("verbose_out.pdf");
    fs::write(&input, make_test_docx("Verbose flag testing")).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .arg("-v")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(output.status.code(), Some(0), "CLI with -v must succeed");
    assert!(out_pdf.exists(), "Output PDF must exist");

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("[verbose] Reading input file:"));
    assert!(stderr_str.contains("[verbose] Font discovery policy:"));
    assert!(stderr_str.contains("[verbose] Converted document in"));
    assert!(stderr_str.contains("[verbose] Committed PDF to"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_19_batch_directory_conversion_scoped_concurrency() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case19_batch");
    let in_dir = dir.join("inputs");
    let out_dir = dir.join("outputs");
    fs::create_dir_all(&in_dir).unwrap();

    // Create valid test files
    fs::write(in_dir.join("doc_a.docx"), make_test_docx("Doc A content")).unwrap();
    fs::write(in_dir.join("doc_b.docx"), make_test_docx("Doc B content")).unwrap();
    fs::write(in_dir.join("doc_c.docx"), make_test_docx("Doc C content")).unwrap();

    // Create an Office temporary/lock file that must be skipped
    fs::write(in_dir.join("~$doc_a.docx"), b"office temp lock bytes").unwrap();

    let output = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("-j")
        .arg("2")
        .arg("-v")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Batch conversion must succeed"
    );
    assert!(out_dir.join("doc_a.pdf").exists(), "doc_a.pdf must exist");
    assert!(out_dir.join("doc_b.pdf").exists(), "doc_b.pdf must exist");
    assert!(out_dir.join("doc_c.pdf").exists(), "doc_c.pdf must exist");
    assert!(
        !out_dir.join("~$doc_a.pdf").exists(),
        "Temporary lock file must NOT be converted"
    );

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("[verbose] Discovered 3 documents"));
    assert!(stderr_str.contains("Batch conversion complete. Total: 3, Succeeded: 3"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_20_batch_directory_conversion_report_json() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case20_batch_report");
    let in_dir = dir.join("inputs");
    let out_dir = dir.join("outputs");
    let report_json = dir.join("batch_summary.json");
    fs::create_dir_all(&in_dir).unwrap();

    fs::write(in_dir.join("alpha.docx"), make_test_docx("Alpha content")).unwrap();
    fs::write(in_dir.join("beta.docx"), make_test_docx("Beta content")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("--report-json")
        .arg(&report_json)
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Batch conversion must succeed"
    );
    assert!(out_dir.join("alpha.pdf").exists(), "alpha.pdf must exist");
    assert!(out_dir.join("beta.pdf").exists(), "beta.pdf must exist");
    assert!(report_json.exists(), "batch_summary.json must be generated");

    let report_content = fs::read_to_string(&report_json).unwrap();
    assert!(report_content.contains(r#""mode": "batch""#));
    assert!(report_content.contains(r#""total": 2"#));
    assert!(report_content.contains(r#""succeeded": 2"#));
    assert!(report_content.contains("alpha.docx"));
    assert!(report_content.contains("beta.docx"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_21_batch_recursive_directory_conversion() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case21_batch_rec");
    let in_dir = dir.join("inputs");
    let sub_dir = in_dir.join("sub_a").join("nested_b");
    let out_dir = dir.join("outputs");
    fs::create_dir_all(&sub_dir).unwrap();

    fs::write(in_dir.join("root.docx"), make_test_docx("Root content")).unwrap();
    fs::write(sub_dir.join("leaf.docx"), make_test_docx("Leaf content")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("--recursive")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Recursive batch conversion must succeed"
    );
    assert!(out_dir.join("root.pdf").exists(), "root.pdf must exist");
    let nested_pdf = out_dir.join("sub_a").join("nested_b").join("leaf.pdf");
    assert!(
        nested_pdf.exists(),
        "leaf.pdf must exist under mirrored subdirectory structure: {:?}",
        nested_pdf
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_22_cli_analyze_mode_no_output_flag() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case22_analyze_no_output");
    let in_file = dir.join("document.docx");
    fs::write(&in_file, make_test_docx("WorDF 分析测试文本 12345")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .output()
        .expect("Failed to execute wordf binary in --analyze mode");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Analyze mode without -o must succeed with exit code 0"
    );
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout_str.contains("WorDF Document Analysis Report"),
        "Stdout must contain analysis report header, got: {}",
        stdout_str
    );
    assert!(stdout_str.contains("East Asian Chars:"));
    assert!(stdout_str.contains("Paragraphs:"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_23_cli_analyze_mode_report_json() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case23_analyze_json");
    let in_file = dir.join("diag.docx");
    let report_json = dir.join("analysis_report.json");
    fs::write(&in_file, make_test_docx("诊断模式 JSON 输出测试")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--report-json")
        .arg(&report_json)
        .output()
        .expect("Failed to execute wordf binary in --analyze mode with --report-json");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Analyze mode with --report-json must succeed"
    );
    assert!(
        report_json.exists(),
        "Analysis JSON report must be created on disk"
    );

    let json_content = fs::read_to_string(&report_json).unwrap();
    assert!(json_content.contains(r#""mode": "analyze""#));
    assert!(json_content.contains(r#""paragraphs": 1"#));
    assert!(json_content.contains(r#""cjk_characters":"#));
    assert!(json_content.contains(r#""diagnostics":"#));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_24_cli_strict_mode_rejects_warnings() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case24_strict_rejection");
    // Write DOCX bytes into a filename ending with .doc to trigger input.extension_mismatch warning
    let in_file = dir.join("mismatch_container.doc");
    let out_non_strict = dir.join("out_regular.pdf");
    let out_strict = dir.join("out_strict.pdf");
    let strict_report = dir.join("strict_report.json");
    fs::write(&in_file, make_test_docx("Strict mode warning test content")).unwrap();

    // 1. Regular conversion should tolerate warnings and succeed (exit code 0)
    let output_reg = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_non_strict)
        .output()
        .expect("Failed to execute wordf in regular mode");
    assert_eq!(
        output_reg.status.code(),
        Some(0),
        "Regular mode should succeed despite extension mismatch warning"
    );
    assert!(
        out_non_strict.exists(),
        "Regular mode must produce output PDF"
    );

    // 2. Strict conversion must reject warning and abort without writing PDF (exit code 3)
    let output_strict = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_strict)
        .arg("--strict")
        .arg("--report-json")
        .arg(&strict_report)
        .output()
        .expect("Failed to execute wordf in strict mode");
    assert_eq!(
        output_strict.status.code(),
        Some(3),
        "Strict mode must fail with exit code 3 when warnings are present"
    );
    assert!(
        !out_strict.exists(),
        "Strict mode must NOT produce output PDF when aborted"
    );
    assert!(
        strict_report.exists(),
        "Strict mode should produce diagnostic JSON report if requested"
    );

    let report_content = fs::read_to_string(&strict_report).unwrap();
    assert!(report_content.contains(r#""conversion_status": "failure_strict""#));
    assert!(report_content.contains(r#""delivery_status": "aborted""#));
    assert!(report_content.contains("input.extension_mismatch"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_25_cli_analyze_strict_mode_rejects_warnings() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case25_analyze_strict");
    let in_file = dir.join("mismatch.doc");
    fs::write(&in_file, make_test_docx("Analyze strict warning test")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--strict")
        .output()
        .expect("Failed to execute wordf in analyze strict mode");

    assert_eq!(
        output.status.code(),
        Some(3),
        "Analyze mode with --strict must fail with exit code 3 when warnings are detected"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_26_batch_resume_incremental_conversion() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case26_batch_resume");
    let in_dir = dir.join("inputs");
    let out_dir = dir.join("outputs");
    let rep_json = dir.join("resume_report.json");
    fs::create_dir_all(&in_dir).unwrap();

    let doc1 = in_dir.join("doc1.docx");
    let doc2 = in_dir.join("doc2.docx");
    let pdf1 = out_dir.join("doc1.pdf");
    let pdf2 = out_dir.join("doc2.pdf");

    fs::write(&doc1, make_test_docx("Initial content 1")).unwrap();
    fs::write(&doc2, make_test_docx("Initial content 2")).unwrap();

    // 1. Initial conversion without --resume
    let out1 = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .output()
        .expect("Failed initial batch conversion");
    assert_eq!(out1.status.code(), Some(0));
    assert!(pdf1.exists());
    assert!(pdf2.exists());

    let pdf1_mtime_init = pdf1.metadata().unwrap().modified().unwrap();

    // 2. Run again with --resume immediately -> both should be skipped
    std::thread::sleep(std::time::Duration::from_millis(100));
    let out2 = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("--resume")
        .arg("--report-json")
        .arg(&rep_json)
        .output()
        .expect("Failed resume batch conversion");
    assert_eq!(out2.status.code(), Some(0));

    let rep2 = fs::read_to_string(&rep_json).unwrap();
    assert!(
        rep2.contains(r#""skipped": 2"#),
        "Both files must be skipped when up-to-date"
    );
    assert!(rep2.contains(r#""succeeded": 0"#));

    let pdf1_mtime_after_skip = pdf1.metadata().unwrap().modified().unwrap();
    assert_eq!(
        pdf1_mtime_init, pdf1_mtime_after_skip,
        "Up-to-date PDF must not be re-written"
    );

    // 3. Update doc1 only, run again with --resume -> doc1 converted, doc2 skipped
    std::thread::sleep(std::time::Duration::from_millis(150));
    fs::write(&doc1, make_test_docx("Updated content for doc1")).unwrap();

    let out3 = Command::new(&bin)
        .arg(&in_dir)
        .arg("-o")
        .arg(&out_dir)
        .arg("--resume")
        .arg("--report-json")
        .arg(&rep_json)
        .output()
        .expect("Failed incremental batch conversion");
    assert_eq!(out3.status.code(), Some(0));

    let rep3 = fs::read_to_string(&rep_json).unwrap();
    assert!(
        rep3.contains(r#""succeeded": 1"#),
        "Modified document must be converted"
    );
    assert!(
        rep3.contains(r#""skipped": 1"#),
        "Unmodified document must be skipped"
    );

    let pdf1_mtime_updated = pdf1.metadata().unwrap().modified().unwrap();
    assert!(
        pdf1_mtime_updated > pdf1_mtime_init,
        "Modified doc1.pdf must be updated"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_27_single_file_resume_incremental() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case27_single_resume");
    let in_file = dir.join("single.docx");
    let out_file = dir.join("single.pdf");

    fs::write(&in_file, make_test_docx("Single file initial")).unwrap();

    // Initial conversion
    let out1 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_file)
        .output()
        .expect("Failed initial conversion");
    assert_eq!(out1.status.code(), Some(0));
    assert!(out_file.exists());

    // Run again with --resume -> should skip without error
    std::thread::sleep(std::time::Duration::from_millis(100));
    let out2 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_file)
        .arg("--resume")
        .output()
        .expect("Failed single file resume");
    assert_eq!(out2.status.code(), Some(0));
    let stderr_str = String::from_utf8_lossy(&out2.stderr);
    assert!(stderr_str.contains("up-to-date"));

    // Modify input file -> should convert and overwrite
    std::thread::sleep(std::time::Duration::from_millis(150));
    fs::write(&in_file, make_test_docx("Single file modified")).unwrap();

    let out3 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_file)
        .arg("--resume")
        .output()
        .expect("Failed single file resume re-conversion");
    assert_eq!(out3.status.code(), Some(0));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_28_cli_stdin_to_file() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case28_stdin_to_file");
    let out_file = dir.join("from_stdin.pdf");
    let docx_bytes = make_test_docx("Stdin to file stream conversion");

    let mut child = Command::new(&bin)
        .arg("-")
        .arg("-o")
        .arg(&out_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn wordf process");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&docx_bytes)
        .expect("Failed to write to stdin");

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdin -> file must exit with code 0"
    );
    assert!(out_file.exists(), "Output PDF must be generated");

    let pdf_bytes = fs::read(&out_file).unwrap();
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "Output file must be a valid PDF"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_29_cli_file_to_stdout() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case29_file_to_stdout");
    let in_file = dir.join("local.docx");
    fs::write(&in_file, make_test_docx("File to stdout stream conversion")).unwrap();

    let output = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg("-")
        .output()
        .expect("Failed to execute wordf file -> stdout");

    assert_eq!(
        output.status.code(),
        Some(0),
        "file -> stdout must exit with code 0"
    );
    assert!(
        output.stdout.starts_with(b"%PDF-"),
        "stdout stream must start with %PDF- header"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_30_cli_stdin_to_stdout_pipeline() {
    let bin = get_bin_path();
    let docx_bytes = make_test_docx("Full pipeline stream test: stdin -> stdout");

    let mut child = Command::new(&bin)
        .arg("-")
        .arg("-o")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn wordf pipeline process");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&docx_bytes)
        .expect("Failed to write to pipeline stdin");

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert_eq!(
        output.status.code(),
        Some(0),
        "Full pipeline must exit with code 0"
    );
    assert!(
        output.stdout.starts_with(b"%PDF-"),
        "Pipeline stdout must output valid PDF bytes"
    );
}

fn build_minimal_ttf_format12(char_code: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    buf.extend_from_slice(&1u16.to_be_bytes()); // 1 table
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

    let cmap_tag_offset = buf.len();
    buf.extend_from_slice(b"cmap");
    buf.extend_from_slice(&[0, 0, 0, 0]); // checksum
    let table_offset = (buf.len() + 8) as u32;
    buf.extend_from_slice(&table_offset.to_be_bytes());
    buf.extend_from_slice(&[0, 0, 0, 0]); // len placeholder

    // cmap header
    buf.extend_from_slice(&0u16.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes()); // 1 subtable

    // subtable 0: Unicode Format 12
    buf.extend_from_slice(&3u16.to_be_bytes()); // Windows
    buf.extend_from_slice(&10u16.to_be_bytes()); // UCS-4
    let sub_offset = 4 + 8;
    buf.extend_from_slice(&(sub_offset as u32).to_be_bytes());

    // Format 12 subtable
    buf.extend_from_slice(&12u16.to_be_bytes());
    buf.extend_from_slice(&0u16.to_be_bytes());
    let f12_len = 16 + 12;
    buf.extend_from_slice(&(f12_len as u32).to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes()); // language
    buf.extend_from_slice(&1u32.to_be_bytes()); // 1 group

    buf.extend_from_slice(&char_code.to_be_bytes());
    buf.extend_from_slice(&char_code.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes()); // glyph id 1

    let total_table_len = buf.len() - (table_offset as usize);
    buf[cmap_tag_offset + 12..cmap_tag_offset + 16]
        .copy_from_slice(&(total_table_len as u32).to_be_bytes());

    buf
}

#[test]
fn test_case_31_cli_multi_font_fallback_chain() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case31_font_chain");
    let font_a = dir.join("font_alpha.ttf");
    let font_b = dir.join("font_beta.ttf");
    let in_file = dir.join("compound.docx");

    // Font Alpha covers '甲' (U+7532)
    fs::write(&font_a, build_minimal_ttf_format12(0x7532)).unwrap();
    // Font Beta covers '乙' (U+4E59)
    fs::write(&font_b, build_minimal_ttf_format12(0x4E59)).unwrap();

    // Document contains both '甲' and '乙'
    fs::write(&in_file, make_test_docx("甲乙")).unwrap();

    // 1. Single font (alpha only) in strict analyze mode fails due to missing '乙'
    let out_single = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--strict")
        .arg("--font-path")
        .arg(&font_a)
        .output()
        .expect("Failed to execute wordf in single font mode");
    assert_eq!(
        out_single.status.code(),
        Some(3),
        "Single font lacking '乙' must fail in strict analyze mode"
    );

    // 2. Multi-font fallback chain (both alpha and beta) covers all glyphs -> succeeds
    let chain_arg = format!("{},{}", font_a.to_string_lossy(), font_b.to_string_lossy());
    let out_chain = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--strict")
        .arg("--font-path")
        .arg(&chain_arg)
        .output()
        .expect("Failed to execute wordf in multi-font chain mode");
    assert_eq!(
        out_chain.status.code(),
        Some(0),
        "Multi-font fallback chain covering all glyphs must succeed in strict analyze mode"
    );

    let _ = fs::remove_dir_all(&dir);
}

fn make_multipage_docx(pages: &[&str]) -> Vec<u8> {
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
        let mut body_content = String::new();
        for (i, text) in pages.iter().enumerate() {
            if i > 0 {
                body_content.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
            }
            body_content.push_str(&format!(r#"<w:p><w:r><w:t>{}</w:t></w:r></w:p>"#, text));
        }
        let doc_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {}
  </w:body>
</w:document>"#,
            body_content
        );
        writer.write_all(doc_xml.as_bytes()).unwrap();

        writer.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn test_case_32_cli_pages_slice_export() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case32_pages_slice");
    let in_file = dir.join("multi.docx");
    let out_p2 = dir.join("p2.pdf");
    let rep_p2 = dir.join("p2_rep.json");
    let out_p12 = dir.join("p12.pdf");
    let rep_p12 = dir.join("p12_rep.json");
    let out_p5 = dir.join("p5.pdf");

    fs::write(
        &in_file,
        make_multipage_docx(&[
            "First page content",
            "Second page content",
            "Third page content",
        ]),
    )
    .unwrap();

    // 1. Single page slice: --pages 2
    let out1 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_p2)
        .arg("--pages")
        .arg("2")
        .arg("--report-json")
        .arg(&rep_p2)
        .output()
        .expect("Failed to execute wordf with --pages 2");
    assert_eq!(out1.status.code(), Some(0));
    assert!(out_p2.exists());
    let rep1_str = fs::read_to_string(&rep_p2).unwrap();
    assert!(
        rep1_str.contains(r#""page_count": 1"#),
        "Extracted single page must have page_count 1"
    );

    // 2. Page range slice: --pages 1-2
    let out2 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_p12)
        .arg("--pages")
        .arg("1-2")
        .arg("--report-json")
        .arg(&rep_p12)
        .output()
        .expect("Failed to execute wordf with --pages 1-2");
    assert_eq!(out2.status.code(), Some(0));
    assert!(out_p12.exists());
    let rep2_str = fs::read_to_string(&rep_p12).unwrap();
    assert!(
        rep2_str.contains(r#""page_count": 2"#),
        "Extracted page range 1-2 must have page_count 2"
    );

    // 3. Page range exceeding document total pages: --pages 5 -> exit code 3
    let out3 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_p5)
        .arg("--pages")
        .arg("5")
        .output()
        .expect("Failed to execute wordf with --pages 5");
    assert_eq!(
        out3.status.code(),
        Some(3),
        "Out-of-range page selection must fail with exit code 3"
    );
    let stderr3 = String::from_utf8_lossy(&out3.stderr);
    assert!(
        stderr3.contains("out of range") || stderr3.contains("exceeds"),
        "Error message must indicate page index out of range: {}",
        stderr3
    );

    // 4. Invalid CLI syntax: --pages 0 -> exit code 2
    let out4 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(dir.join("invalid.pdf"))
        .arg("--pages")
        .arg("0")
        .output()
        .expect("Failed to execute wordf with invalid --pages 0");
    assert_eq!(
        out4.status.code(),
        Some(2),
        "Invalid 0-page input must be rejected by CLI parser with exit code 2"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_33_encrypted_docx_cli_scenarios() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case33_encrypted");
    let in_file = dir.join("protected.docx");
    let out_no_pwd = dir.join("no_pwd.pdf");
    let out_wrong_pwd = dir.join("wrong_pwd.pdf");
    let out_correct = dir.join("correct.pdf");

    let docx = make_test_docx("Classified Document Content for CLI Test");
    let password = "SecretWordfPassword#2026";
    let encrypted_cfb = wordf::crypto::agile::build_agile_encrypted_cfb_for_test(&docx, password);
    fs::write(&in_file, &encrypted_cfb).unwrap();

    // 1. Missing --password -> exit code 3
    let out1 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_no_pwd)
        .output()
        .expect("Failed to execute wordf without password");
    assert_eq!(
        out1.status.code(),
        Some(3),
        "Missing password must fail with exit code 3"
    );
    assert!(!out_no_pwd.exists(), "Output PDF must not be created");
    let stderr1 = String::from_utf8_lossy(&out1.stderr);
    assert!(stderr1.contains("password-protected") || stderr1.contains("--password"));

    // 2. Wrong --password -> exit code 3
    let out2 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_wrong_pwd)
        .arg("--password")
        .arg("IncorrectPass")
        .output()
        .expect("Failed to execute wordf with wrong password");
    assert_eq!(
        out2.status.code(),
        Some(3),
        "Wrong password must fail with exit code 3"
    );
    assert!(!out_wrong_pwd.exists(), "Output PDF must not be created");
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(stderr2.contains("Incorrect password"));

    // 3. Correct --password -> exit code 0 and valid PDF created
    let out3 = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_correct)
        .arg("--password")
        .arg(password)
        .output()
        .expect("Failed to execute wordf with correct password");
    assert_eq!(
        out3.status.code(),
        Some(0),
        "Correct password must succeed with exit code 0"
    );
    assert!(out_correct.exists(), "Output PDF must exist");
    let pdf_bytes = fs::read(&out_correct).unwrap();
    assert!(pdf_bytes.starts_with(b"%PDF-"));

    // 4. Document analysis with --password
    let out4 = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--password")
        .arg(password)
        .output()
        .expect("Failed to execute wordf --analyze with password");
    assert_eq!(out4.status.code(), Some(0));
    let stdout4 = String::from_utf8_lossy(&out4.stdout);
    assert!(stdout4.contains("WorDF Document Analysis Report"));
    assert!(stdout4.contains("Format:      OOXML DOCX"));

    // 5. Stdin/Stdout stream conversion with --password
    let mut child = Command::new(&bin)
        .arg("-")
        .arg("-o")
        .arg("-")
        .arg("--password")
        .arg(password)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn wordf stdin/stdout process");

    {
        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        stdin
            .write_all(&encrypted_cfb)
            .expect("Failed to write to stdin");
    }

    let out5 = child.wait_with_output().expect("Failed to wait on child");
    assert_eq!(out5.status.code(), Some(0));
    assert!(out5.stdout.starts_with(b"%PDF-"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_34_cli_metadata_injection_and_pdf_info() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case34_metadata");
    let in_file = dir.join("plain.docx");
    let out_file = dir.join("meta.pdf");
    let report_file = dir.join("meta_analysis.json");

    fs::write(
        &in_file,
        make_test_docx("Metadata CLI verification test document."),
    )
    .unwrap();

    // 1. Conversion with --title, --author, --subject, --keywords
    let output = Command::new(&bin)
        .arg(&in_file)
        .arg("-o")
        .arg(&out_file)
        .arg("--title")
        .arg("CLI Custom Title")
        .arg("--author")
        .arg("CLI Custom Author")
        .arg("--subject")
        .arg("CLI Custom Subject")
        .arg("--keywords")
        .arg("rust,metadata,cli")
        .output()
        .expect("Failed to run wordf with metadata flags");

    assert_eq!(output.status.code(), Some(0));
    assert!(out_file.exists());
    let pdf_bytes = fs::read(&out_file).unwrap();
    let pdf_str = String::from_utf8_lossy(&pdf_bytes);
    assert!(pdf_str.contains("CLI Custom Title"));
    assert!(pdf_str.contains("CLI Custom Author"));
    assert!(pdf_str.contains("CLI Custom Subject"));
    assert!(pdf_str.contains("rust,metadata,cli"));

    // 2. Analyze mode with metadata flags and --report-json
    let out_an = Command::new(&bin)
        .arg(&in_file)
        .arg("--analyze")
        .arg("--title")
        .arg("Analysis Title")
        .arg("--author")
        .arg("Analysis Author")
        .arg("--report-json")
        .arg(&report_file)
        .output()
        .expect("Failed to run wordf --analyze with metadata");

    assert_eq!(out_an.status.code(), Some(0));
    let stdout_an = String::from_utf8_lossy(&out_an.stdout);
    assert!(stdout_an.contains("[Document Metadata]"));
    assert!(stdout_an.contains("Analysis Title"));
    assert!(stdout_an.contains("Analysis Author"));

    assert!(report_file.exists());
    let rep_str = fs::read_to_string(&report_file).unwrap();
    assert!(rep_str.contains("Analysis Title"));
    assert!(rep_str.contains("Analysis Author"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_35_oversized_file_input_exits_6() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case35_oversize_file");
    let input = dir.join("huge.docx");
    // Sparse-enough 129 MiB placeholder: only metadata is read before rejection.
    let f = fs::File::create(&input).unwrap();
    f.set_len(129 * 1024 * 1024).unwrap();
    drop(f);
    let out_pdf = dir.join("out.pdf");

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(
        output.status.code(),
        Some(6),
        "Oversized input (129 MiB) must exit with code 6 (resource budget), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exceeds maximum limit"),
        "Rejection message must name the size limit"
    );
    assert!(
        !out_pdf.exists(),
        "No PDF must be committed for oversized input"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_36_oversized_stdin_exits_6() {
    let bin = get_bin_path();
    // Exactly MAX_INPUT_BYTES + 1, matching the bounded stdin read in single.rs.
    let payload = vec![0u8; 128 * 1024 * 1024 + 1];

    let mut child = Command::new(&bin)
        .arg("-")
        .arg("-o")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn wordf pipeline");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&payload)
        .expect("Failed to write oversized payload to stdin");
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("Failed to wait for wordf");

    assert_eq!(
        output.status.code(),
        Some(6),
        "Oversized stdin must exit with code 6 (resource budget), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "No PDF bytes may be written to stdout when input is rejected"
    );
}

#[cfg(unix)]
#[test]
fn test_case_37_stdout_route_report_failure_returns_9() {
    use std::os::unix::fs::PermissionsExt;
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case37_stdout_report_failure");
    let input = dir.join("doc.docx");
    fs::write(&input, make_test_docx("stdout route report failure")).unwrap();

    let report_dir = dir.join("report_out");
    fs::create_dir_all(&report_dir).unwrap();
    let out_report = report_dir.join("out.json");

    // Read-only report directory: temp file creation fails after PDF delivery.
    fs::set_permissions(&report_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg("-")
        .arg("--report-json")
        .arg(&out_report)
        .output()
        .expect("Failed to execute wordf binary");

    assert!(
        output.stdout.starts_with(b"%PDF-"),
        "PDF must still be delivered to stdout before the report failure"
    );
    assert_eq!(
        output.status.code(),
        Some(9),
        "Report failure after stdout delivery must exit with code 9, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("report_commit_failed"),
        "Report failure must be reported on stderr, not swallowed"
    );

    // Restore permissions for cleanup
    let _ = fs::set_permissions(&report_dir, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn test_case_38_batch_case_variant_collision_rejects() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case38_case_collision");
    let input_dir = dir.join("input");
    let output_dir = dir.join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    fs::write(
        input_dir.join("Mixed.docx"),
        make_test_docx("case variant A"),
    )
    .unwrap();
    fs::write(
        input_dir.join("mixed.doc"),
        make_test_docx("case variant B"),
    )
    .unwrap();

    let output = Command::new(&bin)
        .arg(&input_dir)
        .arg("-o")
        .arg(&output_dir)
        .output()
        .expect("Failed to execute wordf batch");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Case-variant outputs colliding on a case-insensitive filesystem must be rejected with exit 2, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output_dir.join("Mixed.pdf").exists(),
        "No output may be committed when a collision is detected"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_case_39_batch_case_variant_outputs_distinct() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case39_case_distinct");
    let input_dir = dir.join("input");
    let output_dir = dir.join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    // Dynamically detect if filesystem is case-insensitive (e.g. macOS APFS default)
    let probe_upper = dir.join("PROBE.TMP");
    let probe_lower = dir.join("probe.tmp");
    fs::write(&probe_upper, b"probe").unwrap();
    if probe_lower.exists() {
        let _ = fs::remove_dir_all(&dir);
        return; // Safely skip distinct-name test on case-folding filesystems
    }
    let _ = fs::remove_file(&probe_upper);

    fs::write(
        input_dir.join("Mixed.docx"),
        make_test_docx("case variant A"),
    )
    .unwrap();
    fs::write(
        input_dir.join("mixed.doc"),
        make_test_docx("case variant B"),
    )
    .unwrap();

    let output = Command::new(&bin)
        .arg(&input_dir)
        .arg("-o")
        .arg(&output_dir)
        .output()
        .expect("Failed to execute wordf batch");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Distinct case-variant outputs must convert on case-sensitive filesystems, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output_dir.join("Mixed.pdf").exists() && output_dir.join("mixed.pdf").exists(),
        "Both distinct PDFs must be committed"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_case_40_batch_symlink_output_rejected_and_preserved() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case40_batch_symlink");
    let input_dir = dir.join("input");
    let output_dir = dir.join("output");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    fs::write(input_dir.join("doc.docx"), make_test_docx("symlink guard")).unwrap();
    let target = dir.join("elsewhere.bin");
    fs::write(&target, b"precious").unwrap();
    std::os::unix::fs::symlink(&target, output_dir.join("doc.pdf")).unwrap();

    let output = Command::new(&bin)
        .arg(&input_dir)
        .arg("-o")
        .arg(&output_dir)
        .arg("--overwrite")
        .output()
        .expect("Failed to execute wordf batch");

    assert_eq!(
        output.status.code(),
        Some(1),
        "Symlink destination must fail the batch item, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let link_meta = fs::symlink_metadata(output_dir.join("doc.pdf")).unwrap();
    assert!(
        link_meta.is_symlink(),
        "Symlink destination must be rejected, not silently replaced"
    );
    assert_eq!(
        fs::read(&target).unwrap(),
        b"precious",
        "Symlink target content must stay untouched"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_41_analyze_mode_report_alias_rejection() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case41_analyze_alias");
    let input = dir.join("diag.docx");
    let content = make_test_docx("Analyze report alias protection test");
    fs::write(&input, &content).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("--analyze")
        .arg("--report-json")
        .arg(&input)
        .arg("--overwrite")
        .output()
        .expect("Failed to execute wordf in --analyze mode");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Analyze mode report pointing to input must fail with exit code 2, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&input).unwrap(),
        content,
        "Source file must be untouched when report target conflicts with input"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_case_42_batch_report_target_collision_rejection() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case42_batch_collision");
    let input_dir = dir.join("inputs");
    let output_dir = dir.join("outputs");
    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&output_dir).unwrap();

    let doc = input_dir.join("sample.docx");
    let content = make_test_docx("Batch collision protection");
    fs::write(&doc, &content).unwrap();

    // 1. Report points to input document
    let out1 = Command::new(&bin)
        .arg(&input_dir)
        .arg("-o")
        .arg(&output_dir)
        .arg("--report-json")
        .arg(&doc)
        .arg("--overwrite")
        .output()
        .expect("Failed to execute batch");
    assert_eq!(out1.status.code(), Some(2));
    assert_eq!(
        fs::read(&doc).unwrap(),
        content,
        "Input document must remain untouched"
    );

    // 2. Report points to output PDF target
    let target_pdf = output_dir.join("sample.pdf");
    let out2 = Command::new(&bin)
        .arg(&input_dir)
        .arg("-o")
        .arg(&output_dir)
        .arg("--report-json")
        .arg(&target_pdf)
        .arg("--overwrite")
        .output()
        .expect("Failed to execute batch");
    assert_eq!(out2.status.code(), Some(2));
    assert!(
        !target_pdf.exists(),
        "Target PDF must not have been created or corrupted"
    );

    let _ = fs::remove_dir_all(&dir);
}
