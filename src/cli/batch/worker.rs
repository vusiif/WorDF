//! Worker execution logic for individual batch conversion tasks.
//! License: GPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};

use super::BatchConfig;
use wordf::report::BatchItemReport;
use wordf::transaction::{
    is_same_file, validate_output_target, TaskError, TaskExecutor, TaskFileConfig,
};

/// Build a failed batch item and surface the reason on stderr.
fn failed_item(in_path: &Path, out_pdf: &Path, msg: String) -> BatchTaskResult {
    eprintln!("Error: {}", msg);
    BatchTaskResult::Failed {
        item: BatchItemReport {
            input_path: in_path.to_string_lossy().to_string(),
            output_path: out_pdf.to_string_lossy().to_string(),
            status: "failed".to_string(),
            page_count: 0,
            elapsed_ms: 0.0,
            error: Some(msg),
        },
    }
}

pub enum BatchTaskResult {
    Succeeded {
        item: BatchItemReport,
        total_ms: f64,
        pages: u32,
        out_pdf: PathBuf,
    },
    Skipped {
        item: BatchItemReport,
    },
    Failed {
        item: BatchItemReport,
    },
}

pub fn execute_batch_item(
    in_path: &Path,
    out_pdf: PathBuf,
    config: &BatchConfig,
) -> BatchTaskResult {
    if let Some(parent) = out_pdf.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }

    let in_meta = match in_path.metadata() {
        Ok(m) => m,
        Err(e) => return failed_item(in_path, &out_pdf, format!("Metadata error: {}", e)),
    };

    // Destination safety aligned with single-file mode: reject symlink and
    // directory destinations as well as output/input entity aliases before any
    // conversion work happens.
    if let Err(msg) = validate_output_target(&out_pdf, true, "PDF output") {
        return failed_item(in_path, &out_pdf, msg);
    }
    if is_same_file(&out_pdf, in_path) {
        return failed_item(
            in_path,
            &out_pdf,
            format!(
                "Output PDF {:?} points to the same file entity as input {:?}",
                out_pdf, in_path
            ),
        );
    }

    let should_skip = if config.resume {
        if out_pdf.exists() {
            let in_mtime = in_meta.modified().ok();
            let out_mtime = out_pdf.metadata().and_then(|m| m.modified()).ok();
            matches!((in_mtime, out_mtime), (Some(in_t), Some(out_t)) if out_t >= in_t)
        } else {
            false
        }
    } else {
        !config.overwrite && out_pdf.exists()
    };

    if should_skip {
        return BatchTaskResult::Skipped {
            item: BatchItemReport {
                input_path: in_path.to_string_lossy().to_string(),
                output_path: out_pdf.to_string_lossy().to_string(),
                status: "skipped".to_string(),
                page_count: 0,
                elapsed_ms: 0.0,
                error: None,
            },
        };
    }

    let task_cfg = TaskFileConfig {
        input_path: in_path,
        output_pdf_path: &out_pdf,
        options: &config.options,
        strict: config.strict,
        overwrite: config.overwrite || config.resume,
    };

    match TaskExecutor::execute_file_to_file(&task_cfg) {
        Ok(res) => BatchTaskResult::Succeeded {
            item: BatchItemReport {
                input_path: in_path.to_string_lossy().to_string(),
                output_path: out_pdf.to_string_lossy().to_string(),
                status: "succeeded".to_string(),
                page_count: res.output.page_count,
                elapsed_ms: res.total_ms,
                error: None,
            },
            total_ms: res.total_ms,
            pages: res.output.page_count,
            out_pdf,
        },
        Err(err) => {
            eprintln!("Error converting {:?}: {}", in_path, err);
            let status = match err {
                TaskError::StrictRejected(_) => "failed_strict".to_string(),
                _ => "failed".to_string(),
            };
            BatchTaskResult::Failed {
                item: BatchItemReport {
                    input_path: in_path.to_string_lossy().to_string(),
                    output_path: out_pdf.to_string_lossy().to_string(),
                    status,
                    page_count: 0,
                    elapsed_ms: 0.0,
                    error: Some(err.to_string()),
                },
            }
        }
    }
}
