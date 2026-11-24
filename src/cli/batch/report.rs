//! Batch JSON execution report staging and atomic commit.
//! License: GPL-3.0-or-later

use std::io::Write;
use std::path::Path;

use wordf::report::{format_batch_report_json, BatchItemReport, BatchReportData};
use wordf::transaction::{atomic_commit_rename, create_exclusive_temp_file};

use crate::cli::args::TOOL_VERSION;

use super::{BatchConfig, BatchSummary};

/// Stage, write and atomically commit the batch JSON report.
///
/// Per-file PDFs are committed before this runs: a report failure must exit 9
/// (README contract) unless nothing was ever committed (then exit 4).
pub(super) fn commit_batch_report(
    rep_path: &Path,
    config: &BatchConfig,
    summary: &BatchSummary,
    mut items: Vec<BatchItemReport>,
) -> Result<(), i32> {
    let committed_any = summary.succeeded > 0 || summary.skipped > 0;
    let report_fail_code = if committed_any { 9 } else { 4 };

    items.sort_by(|a, b| a.input_path.cmp(&b.input_path));

    let report_data = BatchReportData {
        tool_name: "wordf",
        tool_version: TOOL_VERSION,
        total: summary.total,
        succeeded: summary.succeeded,
        failed: summary.failed,
        skipped: summary.skipped,
        elapsed_ms: summary.elapsed_ms,
        items: &items,
    };
    let report_json_str = format_batch_report_json(&report_data);

    let mut rep_guard = match create_exclusive_temp_file(rep_path, "rep") {
        Ok(g) => g,
        Err(code) => {
            eprintln!(
                "output.report_commit_failed: Failed to create temp file for batch report JSON {:?} (exit code {}). PDF outputs committed: {}.",
                rep_path, code, committed_any
            );
            return Err(report_fail_code);
        }
    };

    if let Err(e) = rep_guard.file.write_all(report_json_str.as_bytes()) {
        rep_guard.cleanup();
        eprintln!(
            "output.report_commit_failed: Failed to write batch report JSON {:?}: {}. PDF outputs committed: {}.",
            rep_path, e, committed_any
        );
        return Err(report_fail_code);
    }
    if let Err(e) = rep_guard.file.flush() {
        rep_guard.cleanup();
        eprintln!(
            "output.report_commit_failed: Failed to flush batch report JSON {:?}: {}. PDF outputs committed: {}.",
            rep_path, e, committed_any
        );
        return Err(report_fail_code);
    }

    let allow_rep_overwrite = config.overwrite || config.resume;
    if let Err(e) = atomic_commit_rename(&rep_guard.path, rep_path, allow_rep_overwrite) {
        rep_guard.cleanup();
        eprintln!(
            "output.report_commit_failed: Failed to commit batch report JSON {:?}: {}. PDF outputs committed: {}.",
            rep_path, e, committed_any
        );
        return Err(report_fail_code);
    }
    rep_guard.committed = true;
    Ok(())
}
