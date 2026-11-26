//! CLI route for `--analyze` document pre-flight diagnosis.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process;

use super::args::{map_conversion_error_to_exit_code, CliArgs};
use wordf::analyze_document;
use wordf::report::format_analysis_json;
use wordf::transaction::{
    atomic_commit_rename, check_future_target_alias, create_exclusive_temp_file, is_same_file,
    validate_output_target,
};
use wordf::{ConversionOptions, Diagnostic};

pub fn run_analyze_route(args: &CliArgs, input_path: &Path, options: &ConversionOptions) -> ! {
    let is_stdin = input_path.as_os_str() == "-";
    if !is_stdin && !input_path.exists() {
        eprintln!("Error: Input file {:?} does not exist.", input_path);
        process::exit(4);
    }
    if !is_stdin && input_path.is_dir() {
        eprintln!("Error: --analyze mode does not support directory inputs. Please specify a single document.");
        process::exit(2);
    }

    if let Some(ref rep_path) = args.report_json {
        if rep_path.as_os_str() == "-" {
            eprintln!("Error: stdout report '-' is not supported by WorDF contract.");
            process::exit(2);
        }
        if !is_stdin {
            if fs::metadata(rep_path).is_ok() && is_same_file(rep_path, input_path) {
                eprintln!(
                    "Error: JSON report {:?} points to the same file entity as input.",
                    rep_path
                );
                process::exit(2);
            }
            if let Err(code) =
                check_future_target_alias(rep_path, input_path, "JSON report", "input file")
            {
                process::exit(code);
            }
        }
        if let Err(msg) = validate_output_target(rep_path, args.overwrite, "JSON report") {
            eprintln!("Error: {}", msg);
            process::exit(4);
        }
    }

    let input_bytes = if is_stdin {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut handle = std::io::stdin().take(wordf::format::MAX_INPUT_BYTES as u64 + 1);
        if let Err(e) = handle.read_to_end(&mut buf) {
            eprintln!("Error reading input from stdin: {}", e);
            process::exit(4);
        }
        if buf.len() > wordf::format::MAX_INPUT_BYTES {
            eprintln!(
                "Error: Input from stdin exceeds maximum limit of {} bytes (128 MiB)",
                wordf::format::MAX_INPUT_BYTES
            );
            process::exit(6);
        }
        buf
    } else {
        match wordf::transaction::TaskExecutor::read_bounded_file(input_path) {
            Ok(buf) => buf,
            Err(wordf::transaction::TaskError::SizeLimitExceeded(sz)) => {
                eprintln!(
                    "Error: Input file {:?} size ({} bytes) exceeds maximum limit of {} bytes (128 MiB)",
                    input_path,
                    sz,
                    wordf::format::MAX_INPUT_BYTES
                );
                process::exit(6);
            }
            Err(e) => {
                eprintln!("Error reading input file {:?}: {}", input_path, e);
                process::exit(4);
            }
        }
    };

    let mut analysis = match analyze_document(&input_bytes, options) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error during document analysis: {}", e);
            process::exit(map_conversion_error_to_exit_code(&e.kind));
        }
    };

    // Container extension mismatch check
    let ext_lower = input_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let is_doc = input_bytes.starts_with(&wordf::format::DOC_MAGIC);
    let is_docx = input_bytes.starts_with(&wordf::format::ZIP_MAGIC);
    if is_docx && ext_lower == "doc" {
        analysis.warnings.push(Diagnostic {
            code: "input.extension_mismatch".to_string(),
            message: "Input file extension '.doc' does not match verified container format 'docx'"
                .to_string(),
        });
    } else if is_doc && ext_lower == "docx" {
        analysis.warnings.push(Diagnostic {
            code: "input.extension_mismatch".to_string(),
            message: "Input file extension '.docx' does not match verified container format 'doc'"
                .to_string(),
        });
    }

    if !args.quiet {
        print!(
            "{}",
            analysis.render_console_summary(input_path.to_string_lossy().as_ref())
        );
    }

    if let Some(ref rep_path) = args.report_json {
        let json_str = format_analysis_json(&analysis);
        let mut rep_guard = match create_exclusive_temp_file(rep_path, "rep") {
            Ok(g) => g,
            Err(code) => process::exit(code),
        };
        if let Err(e) = rep_guard.file.write_all(json_str.as_bytes()) {
            rep_guard.cleanup();
            eprintln!("Error writing analysis JSON report: {}", e);
            process::exit(4);
        }
        if let Err(e) = rep_guard.file.flush() {
            rep_guard.cleanup();
            eprintln!("Error flushing analysis JSON report: {}", e);
            process::exit(4);
        }
        if let Err(e) = atomic_commit_rename(&rep_guard.path, rep_path, args.overwrite) {
            rep_guard.cleanup();
            eprintln!("Error committing analysis JSON report: {}", e);
            process::exit(4);
        }
        rep_guard.committed = true;
    }

    if args.strict && !analysis.warnings.is_empty() {
        eprintln!(
            "Strict mode failure: {} warning(s) detected during document analysis.",
            analysis.warnings.len()
        );
        process::exit(3);
    }

    process::exit(0);
}
