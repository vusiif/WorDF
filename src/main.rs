//! WorDF P1 CLI interface
//! License: GPL-3.0-or-later

mod cli;

use std::env;
use std::fs;
use std::io::Write;
use std::process;
use std::time::Instant;

use cli::{
    inspect_font_chain, map_conversion_error_to_exit_code, parse_cli_args, run_batch_conversion,
    run_single_conversion, BatchConfig, SingleConfig,
};
use wordf::analyze_document;
use wordf::report::format_analysis_json;
use wordf::transaction::{
    atomic_commit_rename, create_exclusive_temp_file, validate_and_isolate_targets, TargetPaths,
};
use wordf::{ConversionOptions, Diagnostic};

fn main() {
    let t_start = Instant::now();

    let args = match parse_cli_args() {
        Ok(a) => a,
        Err(code) => process::exit(code),
    };

    let input_path = args.input_path.unwrap();

    // Font discovery & multi-tiered fallback chain
    let discovered_fonts = match inspect_font_chain(&args.font_paths) {
        Ok(df) => df,
        Err(code) => process::exit(code),
    };
    if let Some(ref dir) = discovered_fonts.font_dir {
        if !args.font_paths.is_empty() || env::var("DOCXIDE_FONTS").is_err() {
            env::set_var("DOCXIDE_FONTS", dir);
        }
    }

    let (fallback_name, fallback_bytes, font_policy) = if let Some(ref fb) = discovered_fonts.primary_cjk_fallback {
        let policy = if !args.font_paths.is_empty() {
            "explicit-font-path"
        } else {
            "env-docxide-fonts"
        };
        (Some(fb.clone()), discovered_fonts.primary_cjk_font_bytes.clone(), policy)
    } else if wordf::font::has_embedded_cjk_font() {
        (
            Some(wordf::EMBEDDED_CJK_FONT_NAME.to_string()),
            None,
            "system-offline",
        )
    } else {
        (None, None, "system-offline-no-embedded")
    };

    let mut available_names = discovered_fonts.family_names.clone();
    for name in wordf::embedded_cjk_font_names() {
        if !available_names.contains(&name) {
            available_names.push(name);
        }
    }

    let mut fallback_chain = discovered_fonts.fallback_chain.clone();
    if fallback_chain.is_empty() && args.font_paths.is_empty() && wordf::font::has_embedded_cjk_font() {
        let b = wordf::font::embedded_cjk_font_bytes();
        if let Some(cmap) = wordf::font::get_or_init_embedded_font_cmap(b).cloned() {
            fallback_chain.push_with_cmap(
                wordf::EMBEDDED_CJK_FONT_NAME.to_string(),
                b.to_vec(),
                cmap,
            );
        }
    }

    let options = ConversionOptions {
        format: args.format,
        fallback_font_name: fallback_name,
        fallback_font_bytes: fallback_bytes,
        available_font_names: available_names,
        fallback_chain: (!fallback_chain.is_empty()).then_some(fallback_chain),
        pages: args.pages.clone(),
        password: args.password.clone(),
        metadata: (!args.metadata.is_empty()).then_some(args.metadata.clone()),
    };

    // Route: Document Analysis Pre-flight Mode
    if args.analyze {
        let is_stdin = input_path.as_os_str() == "-";
        if !is_stdin && !input_path.exists() {
            eprintln!("Error: Input file {:?} does not exist.", input_path);
            process::exit(4);
        }
        if !is_stdin && input_path.is_dir() {
            eprintln!("Error: --analyze mode does not support directory inputs. Please specify a single document.");
            process::exit(2);
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
            let in_meta = match input_path.metadata() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Error reading metadata for {:?}: {}", input_path, e);
                    process::exit(4);
                }
            };
            if in_meta.len() > wordf::format::MAX_INPUT_BYTES as u64 {
                eprintln!(
                    "Error: Input file {:?} size ({} bytes) exceeds maximum limit of {} bytes (128 MiB)",
                    input_path,
                    in_meta.len(),
                    wordf::format::MAX_INPUT_BYTES
                );
                process::exit(6);
            }
            match fs::read(&input_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Error reading input file {:?}: {}", input_path, e);
                    process::exit(4);
                }
            }
        };

        let mut analysis = match analyze_document(&input_bytes, &options) {
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
                message: "Input file extension '.doc' does not match verified container format 'docx'".to_string(),
            });
        } else if is_doc && ext_lower == "docx" {
            analysis.warnings.push(Diagnostic {
                code: "input.extension_mismatch".to_string(),
                message: "Input file extension '.docx' does not match verified container format 'doc'".to_string(),
            });
        }

        if !args.quiet {
            print!("{}", analysis.render_console_summary(input_path.to_string_lossy().as_ref()));
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

    let output_path = match args.output_pdf {
        Some(p) => p,
        None => {
            eprintln!("Error: Output path is required (e.g. -o output.pdf)");
            process::exit(2);
        }
    };

    // Route: Directory Batch Conversion
    if input_path.is_dir() {
        if output_path.as_os_str() == "-" {
            eprintln!("Error: Cannot output to stdout '-' in batch directory mode.");
            process::exit(2);
        }
        let batch_config = BatchConfig {
            input_dir: input_path,
            output_dir: output_path,
            report_json: args.report_json,
            options,
            overwrite: args.overwrite,
            recursive: args.recursive,
            strict: args.strict,
            resume: args.resume,
            jobs: args.jobs,
            quiet: args.quiet,
            verbose: args.verbose,
        };
        match run_batch_conversion(&batch_config) {
            Ok(_) => process::exit(0),
            Err(code) => process::exit(code),
        }
    }

    let is_streaming = input_path.as_os_str() == "-" || output_path.as_os_str() == "-";
    let mut effective_overwrite = args.overwrite;
    if args.resume && !is_streaming && output_path.exists() {
        let in_mtime = input_path.metadata().and_then(|m| m.modified()).ok();
        let out_mtime = output_path.metadata().and_then(|m| m.modified()).ok();
        if matches!((in_mtime, out_mtime), (Some(in_t), Some(out_t)) if out_t >= in_t) {
            if !args.quiet {
                eprintln!(
                    "WorDF: Output {:?} is up-to-date with input {:?}. Skipping conversion (--resume).",
                    output_path, input_path
                );
            }
            process::exit(0);
        } else {
            effective_overwrite = true;
        }
    }

    // Route: Single File Conversion
    let target_paths = TargetPaths {
        input_path,
        output_pdf_path: output_path,
        report_json_path: args.report_json,
        overwrite: effective_overwrite,
    };

    let validated = match validate_and_isolate_targets(target_paths) {
        Ok(v) => v,
        Err(code) => process::exit(code),
    };

    let single_config = SingleConfig {
        validated: &validated,
        options: &options,
        discovered_fonts: &discovered_fonts,
        font_policy,
        strict: args.strict,
        quiet: args.quiet,
        verbose: args.verbose,
        t_start,
    };

    run_single_conversion(&single_config);
}
