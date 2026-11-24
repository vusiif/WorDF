//! Single-file conversion orchestrator with atomic transactions and reporting.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use wordf::format::MAX_INPUT_BYTES;
use wordf::report::{format_schema_v1_json, ReportData};
use wordf::transaction::{
    atomic_commit_rename, commit_two_phase, create_exclusive_temp_file, TaskExecutor,
    ValidatedTargets,
};
use wordf::{convert, ConversionOptions, Diagnostic};

use crate::cli::args::{map_conversion_error_to_exit_code, TOOL_NAME, TOOL_VERSION};
use crate::cli::discovery::DiscoveredFonts;

pub struct SingleConfig<'a> {
    pub validated: &'a ValidatedTargets,
    pub options: &'a ConversionOptions,
    pub discovered_fonts: &'a DiscoveredFonts,
    pub font_policy: &'static str,
    pub strict: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub t_start: Instant,
}

struct ReportMeta<'a> {
    pub input_bytes: &'a [u8],
    pub warnings: &'a [Diagnostic],
    pub pdf_bytes_len: usize,
    pub page_count: u32,
    pub conversion_status: &'static str,
    pub delivery_status: &'static str,
    pub conv_ms: f64,
}

pub fn run_single_conversion(config: &SingleConfig<'_>) -> ! {
    let input_bytes = read_input_bytes(&config.validated.input_canonical);

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

    let t_conv = Instant::now();
    let mut output = match convert(&input_bytes, config.options) {
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
        &input_bytes,
        &mut output.warnings,
    );

    if !config.quiet && !output.warnings.is_empty() {
        eprintln!("Diagnostics & warnings encountered ({}):", output.warnings.len());
        for w in &output.warnings {
            eprintln!("  [{}] {}", w.code, w.message);
        }
    }

    let status = if output.warnings.is_empty() { "success" } else { "success_with_warnings" };

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
            write_report_file(rep_path, config, &meta);
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
    if path.as_os_str() == "-" {
        let mut buf = Vec::new();
        let mut handle = std::io::stdin().take(MAX_INPUT_BYTES as u64 + 1);
        if handle.read_to_end(&mut buf).is_err() {
            eprintln!("Error reading input from stdin");
            process::exit(4);
        }
        if buf.len() > MAX_INPUT_BYTES {
            eprintln!("Error: Stdin exceeds limit of {} bytes", MAX_INPUT_BYTES);
            process::exit(1);
        }
        buf
    } else {
        fs::read(path).unwrap_or_else(|e| {
            eprintln!("Error reading input file {:?}: {}", path, e);
            process::exit(4);
        })
    }
}

fn deliver_stdout(config: &SingleConfig<'_>, pdf_bytes: &[u8], meta: &ReportMeta<'_>) -> ! {
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(pdf_bytes).is_err() || stdout.flush().is_err() {
        process::exit(4);
    }
    if let Some(ref rep_path) = config.validated.report_json_path {
        write_report_file(rep_path, config, meta);
    }
    if config.verbose {
        eprintln!(
            "[verbose] Committed PDF to {:?} (total elapsed: {:.2}ms)",
            config.validated.output_pdf_path,
            config.t_start.elapsed().as_secs_f64() * 1000.0
        );
    }

    process::exit(0);
}

fn build_report_json(config: &SingleConfig<'_>, meta: &ReportMeta<'_>) -> String {
    let is_doc = meta.input_bytes.starts_with(&wordf::format::DOC_MAGIC);
    let is_docx = meta.input_bytes.starts_with(&wordf::format::ZIP_MAGIC);
    let (fmt, b_name, b_ver) = if is_doc {
        ("doc", "rwml+docxide-pdf", "0.1.4+0.19.0")
    } else if is_docx {
        ("docx", "docxide-pdf", "0.19.0")
    } else {
        ("unknown", "unknown", "unknown")
    };
    let fb_used = meta.warnings.iter().any(|w| w.code == "font.fallback")
        || config.discovered_fonts.primary_cjk_fallback.is_some();

    let rep = ReportData {
        tool_name: TOOL_NAME,
        tool_version: TOOL_VERSION,
        input_format: fmt,
        input_bytes: meta.input_bytes.len(),
        backend_name: b_name,
        backend_version: b_ver,
        conversion_status: meta.conversion_status,
        delivery_status: meta.delivery_status,
        pdf_bytes: meta.pdf_bytes_len,
        pdf_page_count: meta.page_count,
        warnings: meta.warnings,
        conversion_ms: meta.conv_ms,
        pre_report_ms: config.t_start.elapsed().as_secs_f64() * 1000.0,
        font_policy: config.font_policy,
        fallback_used: fb_used,
    };
    format_schema_v1_json(&rep)
}

fn write_report_file(rep_path: &Path, config: &SingleConfig<'_>, meta: &ReportMeta<'_>) {
    let json = build_report_json(config, meta);
    if let Ok(mut g) = create_exclusive_temp_file(rep_path, "rep") {
        if g.file.write_all(json.as_bytes()).is_ok() && g.file.flush().is_ok() {
            if atomic_commit_rename(&g.path, rep_path, config.validated.overwrite).is_ok() {
                g.committed = true;
            } else {
                g.cleanup();
            }
        } else {
            g.cleanup();
        }
    }
}
