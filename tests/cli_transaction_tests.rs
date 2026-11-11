//! WorDF CLI path transaction, alias isolation, and fault injection integration tests.
//! Tests real file system state, hashes, directory contents, and exit codes.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::Command;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn get_bin_path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // drop test binary name
    if p.ends_with("deps") {
        p.pop(); // drop 'deps'
    }
    p.join("wordf")
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

    assert_eq!(output.status.code(), Some(0), "CLI conversion with default embedded font must succeed");
    assert!(out_pdf.exists(), "Output PDF must exist");
    let pdf_bytes = fs::read(&out_pdf).unwrap();
    assert!(pdf_bytes.starts_with(b"%PDF-"), "Output PDF must have valid header");
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
fn test_case_16_chinese_docx_unsupported_glyph_fails_fast_exit_3() {
    let bin = get_bin_path();
    let dir = setup_test_sandbox("case16");
    let input = dir.join("cjk_unsupported.docx");
    let out_pdf = dir.join("cjk_unsupported_out.pdf");

    let unmapped_char = '\u{2A6D0}';
    fs::write(&input, make_test_docx(&format!("包含生僻字{}测试", unmapped_char))).unwrap();

    let output = Command::new(&bin)
        .arg(&input)
        .arg("-o")
        .arg(&out_pdf)
        .env_remove("DOCXIDE_FONTS")
        .output()
        .expect("Failed to execute wordf binary");

    assert_eq!(output.status.code(), Some(3), "Missing font glyph must fail fast with exit code 3");
    assert!(!out_pdf.exists(), "Output PDF must not be committed on fail-fast error");

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("Font diagnostic failure"));

    let _ = fs::remove_dir_all(&dir);
}
