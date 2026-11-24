//! Target path validation and isolation logic.
//! License: GPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};

use super::atomic::{is_same_dir, is_same_file};

pub struct TargetPaths {
    pub input_path: PathBuf,
    pub output_pdf_path: PathBuf,
    pub report_json_path: Option<PathBuf>,
    pub overwrite: bool,
}

pub struct ValidatedTargets {
    /// Input path as provided by the caller (kept verbatim, not canonicalized).
    pub input_canonical: PathBuf,
    pub output_pdf_path: PathBuf,
    pub report_json_path: Option<PathBuf>,
    pub overwrite: bool,
}

/// Pre-flight validation of input and output targets according to spec §§4.2–4.4 and plan review §2.2.
/// Returns Ok(ValidatedTargets) or exits with the appropriate exit code:
/// - Exit code 2: Parameter/alias conflict, same file entity, or non-existent target collision.
/// - Exit code 4: File I/O, parent directory missing, symlink target rejected, or output exists without --overwrite.
/// - Exit code 6: Input exceeds the 128 MiB defensible resource budget.
pub fn validate_and_isolate_targets(targets: TargetPaths) -> Result<ValidatedTargets, i32> {
    let input_path = &targets.input_path;
    let is_stdin = input_path.as_os_str() == "-";

    // 1. Input file checks
    if !is_stdin {
        let input_meta = match fs::symlink_metadata(input_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error reading input file {:?}: {}", input_path, e);
                return Err(4);
            }
        };

        if !input_meta.is_file() {
            eprintln!("Error: Input {:?} is not a regular file.", input_path);
            return Err(4);
        }

        if input_meta.len() > crate::format::MAX_INPUT_BYTES as u64 {
            eprintln!(
                "Error: Input file {:?} size ({} bytes) exceeds maximum limit of {} bytes (128 MiB)",
                input_path,
                input_meta.len(),
                crate::format::MAX_INPUT_BYTES
            );
            return Err(6);
        }
    }

    let pdf_path = &targets.output_pdf_path;
    let is_stdout = pdf_path.as_os_str() == "-";

    // 2. Immediate alias checks against input
    if !is_stdin && !is_stdout {
        if fs::metadata(pdf_path).is_ok() && is_same_file(pdf_path, input_path) {
            eprintln!(
                "Error: Output PDF {:?} points to the same file entity as input.",
                pdf_path
            );
            return Err(2);
        }
        check_future_target_alias(pdf_path, input_path, "PDF output", "input file")?;
    }

    // 3. Report alias checks against input & PDF
    if let Some(ref report_path) = targets.report_json_path {
        if report_path.as_os_str() == "-" {
            eprintln!("Error: stdout report '-' is not supported by WorDF contract.");
            return Err(2);
        }
        if !is_stdin {
            if fs::metadata(report_path).is_ok() && is_same_file(report_path, input_path) {
                eprintln!(
                    "Error: JSON report {:?} points to the same file entity as input.",
                    report_path
                );
                return Err(2);
            }
            check_future_target_alias(report_path, input_path, "JSON report", "input file")?;
        }

        if !is_stdout {
            if is_same_file(pdf_path, report_path) {
                eprintln!(
                    "Error: Output PDF and JSON report point to the same existing file entity."
                );
                return Err(2);
            }
            check_future_target_alias(pdf_path, report_path, "PDF output", "JSON report")?;
        }
    }

    // 4. Output PDF structure & existence checks
    if !is_stdout {
        validate_output_target(pdf_path, targets.overwrite, "PDF output").map_err(|m| {
            eprintln!("Error: {}", m);
            4
        })?;
    }

    // 5. Report JSON structure & existence checks (if provided)
    if let Some(ref report_path) = targets.report_json_path {
        validate_output_target(report_path, targets.overwrite, "JSON report").map_err(|m| {
            eprintln!("Error: {}", m);
            4
        })?;
    }

    Ok(ValidatedTargets {
        input_canonical: input_path.clone(),
        output_pdf_path: targets.output_pdf_path,
        report_json_path: targets.report_json_path,
        overwrite: targets.overwrite,
    })
}

/// Structural validation of a single output destination:
/// - parent directory must exist and be a directory;
/// - the destination itself must not be a symlink or an existing directory;
/// - an existing regular file requires `overwrite`.
///
/// Returns a human-readable reason on failure; the caller decides how to log
/// it and which exit code (or per-item failure) to map it to.
pub fn validate_output_target(path: &Path, overwrite: bool, kind: &str) -> Result<(), String> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let parent_meta = fs::metadata(parent).map_err(|e| {
        format!(
            "Parent directory for {} {:?} does not exist or cannot be accessed: {}",
            kind, path, e
        )
    })?;

    if !parent_meta.is_dir() {
        return Err(format!(
            "Parent path {:?} for {} is not a directory.",
            parent, kind
        ));
    }

    if let Ok(m) = fs::symlink_metadata(path) {
        if m.is_symlink() {
            return Err(format!(
                "Refusing to output to symlink {:?} for {}.",
                path, kind
            ));
        }
        if m.is_dir() {
            return Err(format!(
                "Destination {:?} is an existing directory, not a file.",
                path
            ));
        }
        if !overwrite {
            return Err(format!(
                "Destination file {:?} already exists and --overwrite was not specified.",
                path
            ));
        }
    }

    Ok(())
}

fn check_future_target_alias(
    path_a: &Path,
    path_b: &Path,
    name_a: &str,
    name_b: &str,
) -> Result<(), i32> {
    let parent_a = match path_a.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent_b = match path_b.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let meta_a = match fs::metadata(parent_a) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Error: Parent directory for {} {:?} does not exist or cannot be accessed: {}",
                name_a, path_a, e
            );
            return Err(4);
        }
    };
    if !meta_a.is_dir() {
        eprintln!(
            "Error: Parent path {:?} for {} is not a directory.",
            parent_a, name_a
        );
        return Err(4);
    }

    let meta_b = match fs::metadata(parent_b) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Error: Parent directory for {} {:?} does not exist or cannot be accessed: {}",
                name_b, path_b, e
            );
            return Err(4);
        }
    };
    if !meta_b.is_dir() {
        eprintln!(
            "Error: Parent path {:?} for {} is not a directory.",
            parent_b, name_b
        );
        return Err(4);
    }

    let fname_a = path_a.file_name().unwrap_or_default();
    let fname_b = path_b.file_name().unwrap_or_default();

    let same_name = if cfg!(windows) {
        fname_a
            .to_string_lossy()
            .eq_ignore_ascii_case(&fname_b.to_string_lossy())
    } else {
        fname_a == fname_b
    };

    let same_parent = is_same_dir(parent_a, parent_b, &meta_a, &meta_b);

    if same_parent && same_name {
        eprintln!(
            "Error: Target alias collision between {} {:?} and {} {:?} (same parent directory and filename).",
            name_a, path_a, name_b, path_b
        );
        return Err(2);
    }

    Ok(())
}
