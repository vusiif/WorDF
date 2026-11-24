//! Path resolution, recursive document discovery, and destination mapping.
//! License: GPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_BATCH_WORKERS: usize = 64;

/// Computes destination PDF path for an input document, preserving relative subdirectory structure when present.
pub fn compute_output_pdf_path(
    in_path: &Path,
    input_base: &Path,
    output_base: &Path,
) -> Option<PathBuf> {
    let stem = in_path.file_stem()?.to_str()?;
    if let Ok(rel) = in_path.strip_prefix(input_base) {
        if let Some(rel_parent) = rel.parent() {
            if !rel_parent.as_os_str().is_empty() {
                return Some(output_base.join(rel_parent).join(format!("{}.pdf", stem)));
            }
        }
    }
    Some(output_base.join(format!("{}.pdf", stem)))
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
    }

    files.sort();
    Ok(files)
}
