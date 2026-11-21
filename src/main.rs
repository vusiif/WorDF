//! WorDF P1 CLI interface
//! License: GPL-3.0-or-later

mod cli;

use std::env;
use std::fs;
use std::io::Write;
use std::process;
use std::time::Instant;

use cli::{
    inspect_fonts, map_conversion_error_to_exit_code, parse_cli_args, run_batch_conversion,
    BatchConfig, TOOL_NAME, TOOL_VERSION,
};
use wordf::report::{format_schema_v1_json, ReportData};
use wordf::transaction::{
    commit_two_phase, create_exclusive_temp_file, validate_and_isolate_targets, TargetPaths,
};
use wordf::{convert, ConversionOptions, Diagnostic};

fn main() {
    let t_start = Instant::now();

    let args = match parse_cli_args() {
        Ok(a) => a,
        Err(code) => process::exit(code),
    };

    let input_path = args.input_path.unwrap();
    let output_path = args.output_pdf.unwrap();

    // Font discovery
    let discovered_fonts = inspect_fonts(args.font_path.as_ref());
    if let Some(ref dir) = discovered_fonts.font_dir {
        if args.font_path.is_some() || env::var("DOCXIDE_FONTS").is_err() {
            env::set_var("DOCXIDE_FONTS", dir);
        }
    }

    let (fallback_name, fallback_bytes, font_policy) = if let Some(ref fb) = discovered_fonts.primary_cjk_fallback {
        let policy = if args.font_path.is_some() {
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

    let mut available_names = discovered_fonts.family_names;
    for name in wordf::embedded_cjk_font_names() {
        if !available_names.contains(&name) {
            available_names.push(name);
        }
    }

    let options = ConversionOptions {
        format: args.format,
        fallback_font_name: fallback_name,
        fallback_font_bytes: fallback_bytes,
        available_font_names: available_names,
    };

    // Route: Directory Batch Conversion
    if input_path.is_dir() {
        let batch_config = BatchConfig {
            input_dir: input_path,
            output_dir: output_path,
            options,
            overwrite: args.overwrite,
            jobs: args.jobs,
            quiet: args.quiet,
            verbose: args.verbose,
        };
        match run_batch_conversion(&batch_config) {
            Ok(_) => process::exit(0),
            Err(code) => process::exit(code),
        }
    }

    // Route: Single File Conversion
    let target_paths = TargetPaths {
        input_path,
        output_pdf_path: output_path,
        report_json_path: args.report_json,
        overwrite: args.overwrite,
    };

    let validated = match validate_and_isolate_targets(target_paths) {
        Ok(v) => v,
        Err(code) => process::exit(code),
    };

    let input_bytes = match fs::read(&validated.input_canonical) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading input file {:?}: {}", validated.input_canonical, e);
            process::exit(4);
        }
    };

    if args.verbose {
        eprintln!(
            "[verbose] Reading input file: {:?} ({} bytes)",
            validated.input_canonical,
            input_bytes.len()
        );
        eprintln!(
            "[verbose] Font discovery policy: {}, fallback: {:?}, available font families: {}",
            font_policy,
            options.fallback_font_name,
            options.available_font_names.len()
        );
    }

    // Check extension mismatch diagnostic
    let mut cli_diagnostics = Vec::new();
    let ext_lower = validated
        .input_canonical
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let t_conv_start = Instant::now();
    let conv_res = convert(&input_bytes, &options);
    let conversion_ms = t_conv_start.elapsed().as_secs_f64() * 1000.0;

    let output = match conv_res {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Error during conversion: {}", e);
            let code = map_conversion_error_to_exit_code(&e.kind);
            process::exit(code);
        }
    };

    if args.verbose {
        eprintln!(
            "[verbose] Converted document in {:.2}ms ({} pages, {} bytes PDF)",
            conversion_ms,
            output.page_count,
            output.pdf.len()
        );
    }

    let is_doc = input_bytes.starts_with(&wordf::format::DOC_MAGIC);
    let is_docx = input_bytes.starts_with(&wordf::format::ZIP_MAGIC);
    if is_docx && ext_lower == "doc" {
        cli_diagnostics.push(Diagnostic {
            code: "input.extension_mismatch".to_string(),
            message: "Input file extension '.doc' does not match verified container format 'docx'".to_string(),
        });
    } else if is_doc && ext_lower == "docx" {
        cli_diagnostics.push(Diagnostic {
            code: "input.extension_mismatch".to_string(),
            message: "Input file extension '.docx' does not match verified container format 'doc'".to_string(),
        });
    }

    let mut all_warnings = output.warnings;
    if !cli_diagnostics.is_empty() {
        cli_diagnostics.extend(all_warnings);
        all_warnings = cli_diagnostics;
    }

    if !args.quiet && !all_warnings.is_empty() {
        eprintln!("Diagnostics & warnings encountered ({}):", all_warnings.len());
        for w in &all_warnings {
            eprintln!("  [{}] {}", w.code, w.message);
        }
    }

    // Two-phase commit
    let mut pdf_guard = match create_exclusive_temp_file(&validated.output_pdf_path, "pdf") {
        Ok(g) => g,
        Err(code) => process::exit(code),
    };

    if let Err(e) = pdf_guard.file.write_all(&output.pdf) {
        eprintln!("Error writing PDF data to temporary file {:?}: {}", pdf_guard.path, e);
        pdf_guard.cleanup();
        process::exit(4);
    }
    if let Err(e) = pdf_guard.file.flush() {
        eprintln!("Error flushing PDF data to temporary file: {}", e);
        pdf_guard.cleanup();
        process::exit(4);
    }

    let mut report_guard = if validated.report_json_path.is_some() {
        let rep_path = validated.report_json_path.as_ref().unwrap();
        match create_exclusive_temp_file(rep_path, "rep") {
            Ok(g) => Some(g),
            Err(code) => {
                pdf_guard.cleanup();
                process::exit(code);
            }
        }
    } else {
        None
    };

    if let Some(ref mut r_guard) = report_guard {
        let pre_report_ms = t_start.elapsed().as_secs_f64() * 1000.0;
        let actual_fmt_str = if is_doc {
            "doc"
        } else if is_docx {
            "docx"
        } else {
            "unknown"
        };
        let (backend_name, backend_ver) = if is_doc {
            ("rwml+docxide-pdf", "0.1.4+0.19.0")
        } else {
            ("docxide-pdf", "0.19.0")
        };
        let fallback_used = all_warnings.iter().any(|w| w.code == "font.fallback")
            || discovered_fonts.primary_cjk_fallback.is_some();

        let rep_data = ReportData {
            tool_name: TOOL_NAME,
            tool_version: TOOL_VERSION,
            input_format: actual_fmt_str,
            input_bytes: input_bytes.len(),
            backend_name,
            backend_version: backend_ver,
            conversion_status: if all_warnings.is_empty() {
                "success"
            } else {
                "success_with_warnings"
            },
            delivery_status: "committed",
            pdf_bytes: output.pdf.len(),
            pdf_page_count: output.page_count,
            warnings: &all_warnings,
            conversion_ms,
            pre_report_ms,
            font_policy,
            fallback_used,
        };

        let json_text = format_schema_v1_json(&rep_data);
        if let Err(e) = r_guard.file.write_all(json_text.as_bytes()) {
            eprintln!("Error writing JSON report to temporary file: {}", e);
            pdf_guard.cleanup();
            r_guard.cleanup();
            process::exit(4);
        }
        if let Err(e) = r_guard.file.flush() {
            eprintln!("Error flushing JSON report to temporary file: {}", e);
            pdf_guard.cleanup();
            r_guard.cleanup();
            process::exit(4);
        }
    }

    let commit_res = commit_two_phase(
        &mut pdf_guard,
        &validated.output_pdf_path,
        report_guard.as_mut(),
        validated.report_json_path.as_deref(),
        validated.overwrite,
    );

    match commit_res {
        Ok(()) => {
            if args.verbose {
                eprintln!(
                    "[verbose] Committed PDF to {:?} (total elapsed: {:.2}ms)",
                    validated.output_pdf_path,
                    t_start.elapsed().as_secs_f64() * 1000.0
                );
            }
            process::exit(0)
        }
        Err(code) => {
            process::exit(code);
        }
    }
}
