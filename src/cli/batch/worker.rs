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

    if !config.resume && !config.overwrite && out_pdf.exists() {
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

    let in_bytes = match TaskExecutor::read_bounded_file(in_path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("Error converting {:?}: {}", in_path, err);
            return failed_item(in_path, &out_pdf, err.to_string());
        }
    };

    if config.resume
        && out_pdf.exists()
        && wordf::crypto::decrypt_office_document(&in_bytes, config.options.password.as_deref())
            .is_ok()
        && crate::cli::resume::check_resume_match(
            &in_bytes,
            &out_pdf,
            &config.options,
            config.strict,
        )
    {
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

    match TaskExecutor::execute_bytes_to_file(&in_bytes, &task_cfg) {
        Ok(res) => {
            let fp = crate::cli::resume::compute_conversion_fingerprint(
                &in_bytes,
                &config.options,
                config.strict,
            );
            let _ = crate::cli::resume::write_resume_fingerprint(&out_pdf, &fp);
            BatchTaskResult::Succeeded {
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
            }
        }
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
