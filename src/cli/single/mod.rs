//! Single-file conversion orchestrator with atomic transactions and reporting.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use wordf::format::MAX_INPUT_BYTES;
use wordf::transaction::{
    commit_two_phase, create_exclusive_temp_file, TaskExecutor, ValidatedTargets,
};
use wordf::{convert, ConversionOptions};

use crate::cli::args::map_conversion_error_to_exit_code;
use crate::cli::discovery::DiscoveredFonts;

mod report;
use self::report::{build_report_json, write_report_file, ReportMeta};

pub struct SingleConfig<'a> {
    pub validated: &'a ValidatedTargets,
    pub options: &'a ConversionOptions,
    pub discovered_fonts: &'a DiscoveredFonts,
    pub font_policy: &'static str,
    pub strict: bool,
    pub resume: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub t_start: Instant,
}

pub fn run_single_conversion(config: &SingleConfig<'_>) -> ! {
    let input_bytes = read_input_bytes(&config.validated.input_canonical);

    // Validate encryption and password fail-fast before attempting resume or conversion
    let decrypted_holder = match wordf::crypto::decrypt_office_document(
        &input_bytes,
        config.options.password.as_deref(),
    ) {
        Ok(holder) => holder,
        Err(e) => {
            eprintln!("Error during conversion: {}", e);
            process::exit(map_conversion_error_to_exit_code(&e.kind));
        }
    };

    let is_streaming = config.validated.input_canonical.as_os_str() == "-"
        || config.validated.output_pdf_path.as_os_str() == "-";

    if config.resume
        && !is_streaming
        && config.validated.output_pdf_path.exists()
        && crate::cli::resume::check_resume_match(
            &input_bytes,
            &config.validated.output_pdf_path,
            config.options,
            config.strict,
        )
    {
        if let Some(ref rep_path) = config.validated.report_json_path {
            let meta = ReportMeta {
                input_bytes: &input_bytes,
                warnings: &[],
                pdf_bytes_len: fs::metadata(&config.validated.output_pdf_path)
                    .map(|m| m.len() as usize)
                    .unwrap_or(0),
                page_count: 0,
                conversion_status: "skipped",
                delivery_status: "skipped",
                conv_ms: 0.0,
            };
            if let Err(e) = write_report_file(rep_path, config, &meta) {
                eprintln!("Error writing JSON report {:?}: {}", rep_path, e);
                process::exit(4);
            }
        }
        if !config.quiet {
            eprintln!(
                "WorDF: Output {:?} is up-to-date with input {:?}. Skipping conversion (--resume).",
                config.validated.output_pdf_path, config.validated.input_canonical
            );
        }
        process::exit(0);
    }

    if config.verbose {
        eprintln!(
            "[verbose] Reading input file: {:?} ({} bytes)",
            config.validated.input_canonical,
            input_bytes.len()
        );
        eprintln!(
            "[verbose] Font discovery policy: {}, fallback: {:?}, available font families: {}",
            config.font_policy,
            config.options.fallback_font_name,
            config.options.available_font_names.len()
        );
    }

    let effective_bytes = decrypted_holder.as_deref().unwrap_or(&input_bytes);
    let t_conv = Instant::now();
    let mut output = match convert(effective_bytes, config.options) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Error during conversion: {}", e);
            process::exit(map_conversion_error_to_exit_code(&e.kind));
        }
    };
    let conv_ms = t_conv.elapsed().as_secs_f64() * 1000.0;

    if config.verbose {
        eprintln!(
            "[verbose] Converted document in {:.2}ms ({} pages, {} bytes PDF)",
            conv_ms,
            output.page_count,
            output.pdf.len()
        );
    }

    TaskExecutor::append_extension_diagnostics(
        &config.validated.input_canonical,
        effective_bytes,
        &mut output.warnings,
    );

    if !config.quiet && !output.warnings.is_empty() {
        eprintln!(
            "Diagnostics & warnings encountered ({}):",
            output.warnings.len()
        );
        for w in &output.warnings {
            eprintln!("  [{}] {}", w.code, w.message);
        }
    }

    let status = if output.warnings.is_empty() {
        "success"
    } else {
        "success_with_warnings"
    };

    // Strict quality gate: abort delivery if any warning occurs
    if config.strict && !output.warnings.is_empty() {
        eprintln!(
            "Strict mode failure: {} warning(s) encountered. PDF delivery aborted.",
            output.warnings.len()
        );
        if let Some(ref rep_path) = config.validated.report_json_path {
            let meta = ReportMeta {
                input_bytes: &input_bytes,
                warnings: &output.warnings,
                pdf_bytes_len: 0,
                page_count: 0,
                conversion_status: "failure_strict",
                delivery_status: "aborted",
                conv_ms,
            };
            if let Err(e) = write_report_file(rep_path, config, &meta) {
                eprintln!("Error writing JSON report {:?}: {}", rep_path, e);
            }
        }
        process::exit(3);
    }

    // Stream output route: stdout
    if config.validated.output_pdf_path.as_os_str() == "-" {
        let meta = ReportMeta {
            input_bytes: &input_bytes,
            warnings: &output.warnings,
            pdf_bytes_len: output.pdf.len(),
            page_count: output.page_count,
            conversion_status: status,
            delivery_status: "committed",
            conv_ms,
        };
        deliver_stdout(config, &output.pdf, &meta);
    }

    // Two-phase commit: PDF temp + optional Report temp
    let mut pdf_guard = match create_exclusive_temp_file(&config.validated.output_pdf_path, "pdf") {
        Ok(g) => g,
        Err(code) => process::exit(code),
    };

    if pdf_guard.file.write_all(&output.pdf).is_err() || pdf_guard.file.flush().is_err() {
        pdf_guard.cleanup();
        process::exit(4);
    }

    let mut report_guard = config.validated.report_json_path.as_ref().map(|rep_path| {
        let mut r_guard = create_exclusive_temp_file(rep_path, "rep").unwrap_or_else(|c| {
            pdf_guard.cleanup();
            process::exit(c);
        });
        let meta = ReportMeta {
            input_bytes: &input_bytes,
            warnings: &output.warnings,
            pdf_bytes_len: output.pdf.len(),
            page_count: output.page_count,
            conversion_status: status,
            delivery_status: "committed",
            conv_ms,
        };
        let json = build_report_json(config, &meta);
        if r_guard.file.write_all(json.as_bytes()).is_err() || r_guard.file.flush().is_err() {
            pdf_guard.cleanup();
            r_guard.cleanup();
            process::exit(4);
        }
        r_guard
    });

    if let Err(code) = commit_two_phase(
        &mut pdf_guard,
        &config.validated.output_pdf_path,
        report_guard.as_mut(),
        config.validated.report_json_path.as_deref(),
        config.validated.overwrite,
    ) {
        process::exit(code);
    }

    let fp = crate::cli::resume::compute_conversion_fingerprint(
        &input_bytes,
        config.options,
        config.strict,
    );
    let _ = crate::cli::resume::write_resume_fingerprint(&config.validated.output_pdf_path, &fp);
    exit_committed(config);
}

fn exit_committed(config: &SingleConfig<'_>) -> ! {
    if config.verbose {
        eprintln!(
            "[verbose] Committed PDF to {:?} (total elapsed: {:.2}ms)",
            config.validated.output_pdf_path,
            config.t_start.elapsed().as_secs_f64() * 1000.0
        );
    }
    process::exit(0);
}

fn read_input_bytes(path: &Path) -> Vec<u8> {
    if path.as_os_str() != "-" {
        return TaskExecutor::read_bounded_file(path).unwrap_or_else(|err| {
            eprintln!("Error reading input file {:?}: {}", path, err);
            let code = match err {
                wordf::transaction::TaskError::SizeLimitExceeded(_) => 6,
                _ => 4,
            };
            process::exit(code);
        });
    }
    let mut buf = Vec::new();
    if std::io::stdin()
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .is_err()
    {
        eprintln!("Error reading input from stdin");
        process::exit(4);
    }
    if buf.len() > MAX_INPUT_BYTES {
        eprintln!(
            "Error: Input size exceeds {} bytes (128 MiB)",
            MAX_INPUT_BYTES
        );
        process::exit(6);
    }
    buf
}

fn deliver_stdout(config: &SingleConfig<'_>, pdf_bytes: &[u8], meta: &ReportMeta<'_>) -> ! {
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(pdf_bytes).is_err() || stdout.flush().is_err() {
        process::exit(4);
    }
    if let Some(ref rep_path) = config.validated.report_json_path {
        if let Err(e) = write_report_file(rep_path, config, meta) {
            eprintln!(
                "output.report_commit_failed: Failed to write JSON report {:?}: {}. PDF was already delivered to stdout.",
                rep_path, e
            );
            process::exit(9);
        }
    }
    exit_committed(config);
}
