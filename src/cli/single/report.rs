//! JSON execution report construction for the single-file route.
//! License: GPL-3.0-or-later

use std::io::Write;
use std::path::Path;

use super::SingleConfig;
use crate::cli::args::{TOOL_NAME, TOOL_VERSION};
use wordf::report::{format_schema_v1_json, ReportData};
use wordf::transaction::{atomic_commit_rename, create_exclusive_temp_file};
use wordf::Diagnostic;

pub(super) struct ReportMeta<'a> {
    pub input_bytes: &'a [u8],
    pub warnings: &'a [Diagnostic],
    pub pdf_bytes_len: usize,
    pub page_count: u32,
    pub conversion_status: &'static str,
    pub delivery_status: &'static str,
    pub conv_ms: f64,
}

pub(super) fn build_report_json(config: &SingleConfig<'_>, meta: &ReportMeta<'_>) -> String {
    let is_doc = meta.input_bytes.starts_with(&wordf::format::DOC_MAGIC);
    let is_docx = meta.input_bytes.starts_with(&wordf::format::ZIP_MAGIC);
    let (fmt, b_name, b_ver) = if is_doc {
        ("doc", "rwml+docxide-pdf", "0.1.4+0.19.0")
    } else if is_docx {
        ("docx", "docxide-pdf", "0.19.0")
    } else {
        ("unknown", "unknown", "unknown")
    };
    let fb_used = meta.warnings.iter().any(|w| w.code == "font.fallback")
        || config.discovered_fonts.primary_cjk_fallback.is_some();

    let rep = ReportData {
        tool_name: TOOL_NAME,
        tool_version: TOOL_VERSION,
        input_format: fmt,
        input_bytes: meta.input_bytes.len(),
        backend_name: b_name,
        backend_version: b_ver,
        conversion_status: meta.conversion_status,
        delivery_status: meta.delivery_status,
        pdf_bytes: meta.pdf_bytes_len,
        pdf_page_count: meta.page_count,
        warnings: meta.warnings,
        conversion_ms: meta.conv_ms,
        pre_report_ms: config.t_start.elapsed().as_secs_f64() * 1000.0,
        font_policy: config.font_policy,
        fallback_used: fb_used,
    };
    format_schema_v1_json(&rep)
}

pub(super) fn write_report_file(
    rep_path: &Path,
    config: &SingleConfig<'_>,
    meta: &ReportMeta<'_>,
) -> Result<(), String> {
    let json = build_report_json(config, meta);
    let mut g = create_exclusive_temp_file(rep_path, "rep")
        .map_err(|code| format!("exclusive temp file creation failed (exit code {})", code))?;
    if let Err(e) = g
        .file
        .write_all(json.as_bytes())
        .and_then(|_| g.file.flush())
    {
        g.cleanup();
        return Err(format!("write failed: {}", e));
    }
    match atomic_commit_rename(&g.path, rep_path, config.validated.overwrite) {
        Ok(()) => {
            g.committed = true;
            Ok(())
        }
        Err(e) => {
            g.cleanup();
            Err(format!("atomic commit failed: {}", e))
        }
    }
}
