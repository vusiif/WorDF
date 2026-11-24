//! WorDF batch directory conversion engine with scoped std concurrency.
//! Processes DOC/DOCX files in parallel using std::thread::scope with zero external dependencies.
//! License: GPL-3.0-or-later

pub mod paths;
mod report;
pub mod worker;

pub use paths::{create_and_validate_batch_plan, discover_word_documents_ext, MAX_BATCH_WORKERS};

#[cfg(test)]
pub use paths::compute_output_pdf_path;
use report::commit_batch_report;
use worker::{execute_batch_item, BatchTaskResult};

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use wordf::report::BatchItemReport;
use wordf::transaction::validate_output_target;
use wordf::ConversionOptions;

pub struct BatchConfig {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub report_json: Option<PathBuf>,
    pub options: ConversionOptions,
    pub overwrite: bool,
    pub recursive: bool,
    pub strict: bool,
    pub resume: bool,
    pub jobs: Option<usize>,
    pub quiet: bool,
    pub verbose: bool,
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone, PartialEq)]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub elapsed_ms: f64,
}

/// Executes parallel batch conversion of all documents in the input directory.
pub fn run_batch_conversion(config: &BatchConfig) -> Result<BatchSummary, i32> {
    let t_start = Instant::now();

    if !config.output_dir.exists() {
        if let Err(e) = fs::create_dir_all(&config.output_dir) {
            eprintln!(
                "Error: Failed to create output directory {:?}: {}",
                config.output_dir, e
            );
            return Err(2);
        }
    } else if !config.output_dir.is_dir() {
        eprintln!(
            "Error: Target output path {:?} exists but is not a directory.",
            config.output_dir
        );
        return Err(2);
    }

    // Fail fast on an invalid JSON report target before any conversion work.
    if let Some(rep_path) = &config.report_json {
        let allow_rep_overwrite = config.overwrite || config.resume;
        if let Err(msg) = validate_output_target(rep_path, allow_rep_overwrite, "JSON report") {
            eprintln!("Error: {}", msg);
            return Err(4);
        }
    }

    let mut files = match discover_word_documents_ext(&config.input_dir, config.recursive) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Error reading input directory {:?}: {}",
                config.input_dir, e
            );
            return Err(4);
        }
    };

    files.sort_by(|a, b| {
        let size_a = a.metadata().map(|m| m.len()).unwrap_or(0);
        let size_b = b.metadata().map(|m| m.len()).unwrap_or(0);
        size_b.cmp(&size_a).then_with(|| a.cmp(b))
    });

    let total = files.len();
    if total == 0 {
        if !config.quiet {
            eprintln!(
                "WorDF: No '.doc' or '.docx' files found in directory {:?}",
                config.input_dir
            );
        }
        return Ok(BatchSummary {
            total: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            elapsed_ms: t_start.elapsed().as_secs_f64() * 1000.0,
        });
    }

    let plan = create_and_validate_batch_plan(
        &config.input_dir,
        &config.output_dir,
        config.report_json.as_deref(),
        &files,
    )?;

    let worker_count = config.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });
    let worker_count = worker_count.max(1).min(total).min(MAX_BATCH_WORKERS);

    if config.verbose {
        eprintln!(
            "[verbose] Discovered {} documents in {:?}. Launching {} parallel worker(s).",
            total, config.input_dir, worker_count
        );
    }

    let next_index = AtomicUsize::new(0);
    let succeeded_count = AtomicUsize::new(0);
    let failed_count = AtomicUsize::new(0);
    let skipped_count = AtomicUsize::new(0);
    let items: Mutex<Vec<BatchItemReport>> = Mutex::new(Vec::with_capacity(total));
    let plan_items = &plan.items;

    std::thread::scope(|s| {
        for _ in 0..worker_count {
            s.spawn(|| loop {
                let idx = next_index.fetch_add(1, Ordering::Relaxed);
                if idx >= total {
                    break;
                }
                let item_plan = &plan_items[idx];
                let in_path = &item_plan.input_path;
                let out_pdf = item_plan.output_pdf_path.clone();

                match execute_batch_item(in_path, out_pdf, config) {
                    BatchTaskResult::Succeeded {
                        item,
                        total_ms,
                        pages,
                        out_pdf,
                    } => {
                        succeeded_count.fetch_add(1, Ordering::Relaxed);
                        items.lock().unwrap().push(item);
                        if config.verbose {
                            eprintln!(
                                "[verbose] [{}/{}] Converted {:?} -> {:?} ({} pages, {:.2}ms)",
                                idx + 1,
                                total,
                                in_path.file_name().unwrap_or_default(),
                                out_pdf.file_name().unwrap_or_default(),
                                pages,
                                total_ms
                            );
                        }
                    }
                    BatchTaskResult::Skipped { item } => {
                        skipped_count.fetch_add(1, Ordering::Relaxed);
                        if config.verbose {
                            let reason = if config.resume {
                                "target PDF is up-to-date"
                            } else {
                                "output already exists"
                            };
                            eprintln!(
                                "[verbose] [{}/{}] Skipped {:?} ({})",
                                idx + 1,
                                total,
                                in_path.file_name().unwrap_or_default(),
                                reason
                            );
                        }
                        items.lock().unwrap().push(item);
                    }
                    BatchTaskResult::Failed { item } => {
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        items.lock().unwrap().push(item);
                    }
                }
            });
        }
    });

    let succeeded = succeeded_count.load(Ordering::Relaxed);
    let failed = failed_count.load(Ordering::Relaxed);
    let skipped = skipped_count.load(Ordering::Relaxed);
    let elapsed_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    if !config.quiet {
        eprintln!(
            "WorDF: Batch conversion complete. Total: {}, Succeeded: {}, Failed: {}, Skipped: {} (Elapsed: {:.2}ms)",
            total, succeeded, failed, skipped, elapsed_ms
        );
    }

    let summary = BatchSummary {
        total,
        succeeded,
        failed,
        skipped,
        elapsed_ms,
    };

    if let Some(ref rep_path) = config.report_json {
        let collected_items = items.into_inner().unwrap();
        commit_batch_report(rep_path, config, &summary, collected_items)?;
    }

    if failed > 0 {
        Err(1)
    } else {
        Ok(summary)
    }
}

#[cfg(test)]
mod tests;
