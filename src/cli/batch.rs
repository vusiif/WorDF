//! WorDF batch directory conversion engine with scoped std concurrency.
//! Processes DOC/DOCX files in parallel using std::thread::scope with zero external dependencies.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use wordf::transaction::{commit_two_phase, create_exclusive_temp_file};
use wordf::{convert, ConversionOptions};

pub struct BatchConfig {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub options: ConversionOptions,
    pub overwrite: bool,
    pub jobs: Option<usize>,
    pub quiet: bool,
    pub verbose: bool,
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub elapsed_ms: f64,
}

/// Collects valid Word document paths from a directory, ignoring Office lock/temp files (`~$*`).
pub fn discover_word_documents(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        // Skip Office lock/temporary files like '~$example.docx'
        if file_name.starts_with("~$") {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if ext == "doc" || ext == "docx" {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
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

    let mut files = match discover_word_documents(&config.input_dir) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Error reading input directory {:?}: {}",
                config.input_dir, e
            );
            return Err(4);
        }
    };

    // Sort files by size in descending order so largest documents start first,
    // maximizing worker saturation and preventing long-tail stall at the end.
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

    let worker_count = config
        .jobs
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        })
        .max(1)
        .min(total);

    if config.verbose {
        eprintln!(
            "[verbose] Discovered {} documents in {:?}. Spawning {} worker threads.",
            total, config.input_dir, worker_count
        );
    }

    let next_index = AtomicUsize::new(0);
    let succeeded_count = AtomicUsize::new(0);
    let failed_count = AtomicUsize::new(0);
    let skipped_count = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let idx = next_index.fetch_add(1, Ordering::Relaxed);
                    if idx >= total {
                        break;
                    }

                    let in_path = &files[idx];
                    let stem = match in_path.file_stem().and_then(|s| s.to_str()) {
                        Some(s) => s,
                        None => {
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let out_pdf = config.output_dir.join(format!("{}.pdf", stem));

                    if !config.overwrite && out_pdf.exists() {
                        skipped_count.fetch_add(1, Ordering::Relaxed);
                        if config.verbose {
                            eprintln!(
                                "[verbose] [{}/{}] Skipped {:?} (output {:?} already exists)",
                                idx + 1,
                                total,
                                in_path.file_name().unwrap_or_default(),
                                out_pdf.file_name().unwrap_or_default()
                            );
                        }
                        continue;
                    }

                    let t_file_start = Instant::now();
                    let file_bytes = match fs::read(in_path) {
                        Ok(b) => b,
                        Err(e) => {
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            eprintln!("Error reading input file {:?}: {}", in_path, e);
                            continue;
                        }
                    };

                    let converted = match convert(&file_bytes, &config.options) {
                        Ok(c) => c,
                        Err(e) => {
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            eprintln!("Error converting {:?}: {}", in_path, e);
                            continue;
                        }
                    };

                    let mut pdf_guard = match create_exclusive_temp_file(&out_pdf, "pdf") {
                        Ok(g) => g,
                        Err(code) => {
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            eprintln!(
                                "Error creating temp file for {:?} (code {})",
                                out_pdf, code
                            );
                            continue;
                        }
                    };

                    if let Err(e) = pdf_guard.file.write_all(&converted.pdf) {
                        pdf_guard.cleanup();
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        eprintln!("Error writing PDF for {:?}: {}", out_pdf, e);
                        continue;
                    }

                    if let Err(e) = pdf_guard.file.flush() {
                        pdf_guard.cleanup();
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        eprintln!("Error flushing PDF for {:?}: {}", out_pdf, e);
                        continue;
                    }

                    if let Err(code) = commit_two_phase(
                        &mut pdf_guard,
                        &out_pdf,
                        None,
                        None,
                        config.overwrite,
                    ) {
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        eprintln!(
                            "Error committing atomic PDF for {:?} (code {})",
                            out_pdf, code
                        );
                        continue;
                    }

                    succeeded_count.fetch_add(1, Ordering::Relaxed);
                    if config.verbose {
                        eprintln!(
                            "[verbose] [{}/{}] Converted {:?} -> {:?} ({} pages, {:.2}ms)",
                            idx + 1,
                            total,
                            in_path.file_name().unwrap_or_default(),
                            out_pdf.file_name().unwrap_or_default(),
                            converted.page_count,
                            t_file_start.elapsed().as_secs_f64() * 1000.0
                        );
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

    if failed > 0 {
        Err(1)
    } else {
        Ok(summary)
    }
}
