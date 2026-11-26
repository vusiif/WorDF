//! Path resolution, recursive document discovery, and destination mapping.
//! License: GPL-3.0-or-later

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use wordf::transaction::{check_future_target_alias, is_dir_case_insensitive, is_same_file};

pub const MAX_BATCH_WORKERS: usize = 16;

#[derive(Debug, Clone)]
pub struct BatchItemPlan {
    pub input_path: PathBuf,
    pub output_pdf_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BatchPlan {
    pub items: Vec<BatchItemPlan>,
}

/// Computes destination PDF path for an input document, preserving relative subdirectory structure when present.
pub fn compute_output_pdf_path(
    in_path: &Path,
    input_base: &Path,
    output_base: &Path,
) -> Option<PathBuf> {
    // OsString-based stem keeps non-UTF-8 file names convertible (e.g. GBK names on Linux).
    let stem = in_path.file_stem()?;
    let mut file_name = stem.to_os_string();
    file_name.push(".pdf");
    if let Ok(rel) = in_path.strip_prefix(input_base) {
        if let Some(rel_parent) = rel.parent() {
            if !rel_parent.as_os_str().is_empty() {
                return Some(output_base.join(rel_parent).join(file_name));
            }
        }
    }
    Some(output_base.join(file_name))
}

/// Collects Word document paths from a directory, optionally traversing recursively.
pub fn discover_word_documents_ext(
    dir: &Path,
    recursive: bool,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current_dir) = stack.pop() {
        for entry in fs::read_dir(&current_dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if recursive {
                    stack.push(path);
                }
                continue;
            }

            let file_name = match path.file_name() {
                Some(n) => n,
                None => continue,
            };

            // Skip Office lock/temporary files like '~$example.docx' while preserving non-UTF-8 filenames
            if file_name.to_string_lossy().starts_with("~$") {
                continue;
            }

            let is_word_ext = path
                .extension()
                .and_then(OsStr::to_str)
                .map(|e| e.eq_ignore_ascii_case("doc") || e.eq_ignore_ascii_case("docx"))
                .unwrap_or(false);

            if is_word_ext {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

/// Builds and validates the complete batch plan, checking for alias collisions between
/// input documents, output PDF targets, and the optional JSON report target before execution.
pub fn create_and_validate_batch_plan(
    input_dir: &Path,
    output_dir: &Path,
    report_json: Option<&Path>,
    files: &[PathBuf],
) -> Result<BatchPlan, i32> {
    if let Some(rep_path) = report_json {
        if is_same_file(input_dir, rep_path) {
            eprintln!(
                "Error: JSON report {:?} points to the input directory {:?}.",
                rep_path, input_dir
            );
            return Err(2);
        }
        if is_same_file(output_dir, rep_path) {
            eprintln!(
                "Error: JSON report {:?} points to the output directory {:?}.",
                rep_path, output_dir
            );
            return Err(2);
        }
    }

    let case_insensitive = is_dir_case_insensitive(output_dir);
    let mut target_map: HashMap<OsString, PathBuf> = HashMap::new();
    let mut items = Vec::with_capacity(files.len());

    for in_path in files {
        let target = match compute_output_pdf_path(in_path, input_dir, output_dir) {
            Some(t) => t,
            None => {
                eprintln!("Error: Failed to compute output PDF path for {:?}", in_path);
                return Err(2);
            }
        };

        let target_key: OsString = if case_insensitive {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::{OsStrExt, OsStringExt};
                OsString::from_vec(
                    target
                        .as_os_str()
                        .as_bytes()
                        .iter()
                        .map(|b| b.to_ascii_lowercase())
                        .collect(),
                )
            }
            #[cfg(not(unix))]
            {
                target.to_string_lossy().to_ascii_lowercase().into()
            }
        } else {
            target.as_os_str().to_os_string()
        };

        if let Some(prev) = target_map.insert(target_key, in_path.clone()) {
            eprintln!(
                "Error: Output filename collision detected between {:?} and {:?} (both map to {:?})",
                prev, in_path, target
            );
            return Err(2);
        }

        if let Some(rep_path) = report_json {
            if fs::metadata(rep_path).is_ok() && is_same_file(in_path, rep_path) {
                eprintln!(
                    "Error: JSON report {:?} points to the same file entity as input document {:?}.",
                    rep_path, in_path
                );
                return Err(2);
            }
            check_future_target_alias(rep_path, in_path, "JSON report", "input file")?;

            if fs::metadata(rep_path).is_ok() && is_same_file(&target, rep_path) {
                eprintln!(
                    "Error: Target alias collision between PDF output {:?} and JSON report {:?}.",
                    target, rep_path
                );
                return Err(2);
            }
            check_future_target_alias(&target, rep_path, "PDF output", "JSON report")?;
        }

        items.push(BatchItemPlan {
            input_path: in_path.clone(),
            output_pdf_path: target,
        });
    }

    Ok(BatchPlan { items })
}
