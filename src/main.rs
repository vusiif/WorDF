//! WorDF P1 CLI interface
//! License: GPL-3.0-or-later

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use wordf::font::{extract_font_family_names, is_cjk_char};
use wordf::report::{format_schema_v1_json, ReportData};
use wordf::transaction::{
    commit_two_phase, create_exclusive_temp_file, validate_and_isolate_targets, TargetPaths,
};
use wordf::{convert, ConversionErrorKind, ConversionOptions, Diagnostic, InputFormat};

const TOOL_NAME: &str = "wordf";
const TOOL_VERSION: &str = "0.1.0";

fn print_help() {
    println!("WorDF: Independent DOC / DOCX to PDF converter in Rust");
    println!("License: GPL-3.0-or-later");
    println!();
    println!("Usage:");
    println!("  wordf <input> -o <output.pdf> [OPTIONS]");
    println!("  wordf --help");
    println!("  wordf --version");
    println!();
    println!("Options:");
    println!("  -o, --output <path>       Output PDF file path (mandatory, '-' stdout rejected)");
    println!("      --format <FORMAT>     Expected input format: auto (default), doc, or docx");
    println!("      --overwrite           Allow overwriting existing ordinary output files");
    println!("      --report-json <path>  Emit machine-readable JSON execution report");
    println!("      --font-path <path>    Local font file or directory for offline CJK injection");
    println!("  -h, --help                Show this help message and exit");
    println!("  -V, --version             Show version and exit");
}

fn print_version() {
    println!("{} {}", TOOL_NAME, TOOL_VERSION);
}

#[derive(Default)]
struct CliArgs {
    input_path: Option<PathBuf>,
    output_pdf: Option<PathBuf>,
    format: InputFormat,
    overwrite: bool,
    report_json: Option<PathBuf>,
    font_path: Option<String>,
}

fn parse_cli_args() -> Result<CliArgs, i32> {
    let args: Vec<OsString> = env::args_os().collect();
    if args.len() <= 1 {
        print_help();
        return Err(2);
    }

    // P0 positional route rejection
    let first_arg = args[1].to_string_lossy();
    if first_arg == "a" || first_arg == "b" || first_arg == "c" {
        eprintln!(
            "Error: P0 positional route syntax 'wordf <a|b|c> ...' has been deprecated and replaced by the P1 CLI contract."
        );
        eprintln!(
            "Usage: wordf <input> -o <output.pdf> [--format auto|doc|docx] [--overwrite] [--report-json <report.json>]"
        );
        return Err(2);
    }

    let mut parsed = CliArgs::default();
    let mut i = 1;

    while i < args.len() {
        let arg_str = args[i].to_string_lossy();
        if arg_str == "--help" || arg_str == "-h" {
            print_help();
            process::exit(0);
        } else if arg_str == "--version" || arg_str == "-V" {
            print_version();
            process::exit(0);
        } else if arg_str == "-o" || arg_str == "--output" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '{}'", arg_str);
                return Err(2);
            }
            parsed.output_pdf = Some(PathBuf::from(&args[i]));
        } else if arg_str == "--format" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--format'");
                return Err(2);
            }
            let fmt_str = args[i].to_string_lossy().to_lowercase();
            match fmt_str.as_str() {
                "auto" => parsed.format = InputFormat::Auto,
                "doc" => parsed.format = InputFormat::Doc,
                "docx" => parsed.format = InputFormat::Docx,
                other => {
                    eprintln!("Error: Unknown format '{}'. Expected 'auto', 'doc', or 'docx'", other);
                    return Err(2);
                }
            }
        } else if arg_str == "--overwrite" {
            parsed.overwrite = true;
        } else if arg_str == "--report-json" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--report-json'");
                return Err(2);
            }
            parsed.report_json = Some(PathBuf::from(&args[i]));
        } else if arg_str == "--font-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--font-path'");
                return Err(2);
            }
            parsed.font_path = Some(args[i].to_string_lossy().to_string());
        } else if arg_str.starts_with('-') {
            eprintln!("Error: Unknown or unsupported option '{}'", arg_str);
            return Err(2);
        } else {
            if parsed.input_path.is_some() {
                eprintln!("Error: Multiple input files provided ('{:?}' and '{:?}'). WorDF P1 processes exactly one regular file.", parsed.input_path.unwrap(), arg_str);
                return Err(2);
            }
            parsed.input_path = Some(PathBuf::from(&args[i]));
        }
        i += 1;
    }

    if parsed.input_path.is_none() {
        eprintln!("Error: Mandatory input file path is missing.");
        return Err(2);
    }

    if parsed.output_pdf.is_none() {
        eprintln!("Error: Mandatory output path '-o <output.pdf>' is missing.");
        return Err(2);
    }

    Ok(parsed)
}

struct DiscoveredFonts {
    font_bytes: Vec<Vec<u8>>,
    font_dir: Option<PathBuf>,
    family_names: Vec<String>,
    primary_cjk_fallback: Option<String>,
    primary_cjk_font_bytes: Option<Vec<u8>>,
}

fn normalize_parent_dir(parent: Option<&Path>) -> PathBuf {
    match parent {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

fn inspect_fonts(path_str: Option<&String>) -> DiscoveredFonts {
    let mut df = DiscoveredFonts {
        font_bytes: Vec::new(),
        font_dir: None,
        family_names: Vec::new(),
        primary_cjk_fallback: None,
        primary_cjk_font_bytes: None,
    };

    let font_path_to_use = path_str.cloned().or_else(|| env::var("DOCXIDE_FONTS").ok());

    let Some(fp) = font_path_to_use else {
        return df;
    };

    let p = Path::new(&fp);
    let mut font_files = Vec::new();

    if p.is_file() {
        font_files.push(p.to_path_buf());
        df.font_dir = Some(normalize_parent_dir(p.parent()));
    } else if p.is_dir() {
        let dir = if p.as_os_str().is_empty() {
            env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            p.to_path_buf()
        };
        df.font_dir = Some(dir);
        if let Ok(entries) = fs::read_dir(p) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if ext_lower == "ttf" || ext_lower == "otf" || ext_lower == "ttc" {
                        font_files.push(path);
                    }
                }
            }
        }
    }

    let mut font_entries = Vec::new();
    for ff in font_files {
        if let Ok(bytes) = fs::read(&ff) {
            let extracted = extract_font_family_names(&bytes);
            for name in &extracted {
                if !df.family_names.contains(name) {
                    df.family_names.push(name.clone());
                }
            }
            font_entries.push((bytes, extracted));
        }
    }

    for (bytes, names) in &font_entries {
        if let Some(cjk_name) = names.iter().find(|n| n.chars().any(is_cjk_char)) {
            df.primary_cjk_fallback = Some(cjk_name.clone());
            df.primary_cjk_font_bytes = Some(bytes.clone());
            break;
        }
    }

    if df.primary_cjk_fallback.is_none() {
        if let Some((bytes, names)) = font_entries.first() {
            df.primary_cjk_fallback = names.first().cloned();
            df.primary_cjk_font_bytes = Some(bytes.clone());
        }
    }

    df.font_bytes = font_entries.into_iter().map(|(b, _)| b).collect();
    df
}

fn map_conversion_error_to_exit_code(err: &ConversionErrorKind) -> i32 {
    match err {
        ConversionErrorKind::UnsupportedFormat => 3,
        ConversionErrorKind::UnsupportedVersion => 3,
        ConversionErrorKind::Encrypted => 3,
        ConversionErrorKind::MissingFont => 3,
        ConversionErrorKind::CorruptInput => 5,
        ConversionErrorKind::ResourceLimit => 6,
        ConversionErrorKind::BackendFailure => 8,
        ConversionErrorKind::UnsupportedCapability => 3,
        ConversionErrorKind::Internal => 8,
    }
}

fn main() {
    let t_start = Instant::now();

    let args = match parse_cli_args() {
        Ok(a) => a,
        Err(code) => process::exit(code),
    };

    let target_paths = TargetPaths {
        input_path: args.input_path.unwrap(),
        output_pdf_path: args.output_pdf.unwrap(),
        report_json_path: args.report_json,
        overwrite: args.overwrite,
    };

    let validated = match validate_and_isolate_targets(target_paths) {
        Ok(v) => v,
        Err(code) => process::exit(code),
    };

    // Font discovery
    let discovered_fonts = inspect_fonts(args.font_path.as_ref());
    if let Some(ref dir) = discovered_fonts.font_dir {
        if args.font_path.is_some() || env::var("DOCXIDE_FONTS").is_err() {
            env::set_var("DOCXIDE_FONTS", dir);
        }
    }

    let options = ConversionOptions {
        format: args.format,
        fallback_font_name: discovered_fonts.primary_cjk_fallback.clone(),
        fallback_font_bytes: discovered_fonts.primary_cjk_font_bytes.clone(),
        available_font_names: discovered_fonts.family_names,
    };

    // Read input snapshot
    let input_bytes = match fs::read(&validated.input_canonical) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading input file {:?}: {}", validated.input_canonical, e);
            process::exit(4);
        }
    };

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

    // Check extension mismatch vs actual format
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

    if !all_warnings.is_empty() {
        eprintln!("Diagnostics & warnings encountered ({}):", all_warnings.len());
        for w in &all_warnings {
            eprintln!("  [{}] {}", w.code, w.message);
        }
    }

    // Pre-report timing measurement
    let pre_report_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    let (backend_name, backend_version, actual_input_format) = if is_doc {
        ("rwml_bridge_docxide", "rwml=0.1.4,docxide-pdf=0.19.0", "doc")
    } else {
        ("docxide_pdf", "docxide-pdf=0.19.0", "docx")
    };

    let report_data = ReportData {
        tool_name: TOOL_NAME,
        tool_version: TOOL_VERSION,
        input_format: actual_input_format,
        input_bytes: input_bytes.len(),
        backend_name,
        backend_version,
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
        font_policy: "system-offline",
        fallback_used: discovered_fonts.primary_cjk_fallback.is_some(),
    };

    let report_json_str = if validated.report_json_path.is_some() {
        Some(format_schema_v1_json(&report_data))
    } else {
        None
    };

    // Phase 1: Create exclusive temporary files and write data
    let mut pdf_temp = match create_exclusive_temp_file(&validated.output_pdf_path, "pdf") {
        Ok(t) => t,
        Err(code) => process::exit(code),
    };

    let mut report_temp = match validated.report_json_path {
        Some(ref p) => match create_exclusive_temp_file(p, "report") {
            Ok(t) => Some(t),
            Err(code) => {
                pdf_temp.cleanup();
                process::exit(code);
            }
        },
        None => None,
    };

    // Write PDF directly via retained exclusive handle
    if let Err(e) = (|| -> Result<(), std::io::Error> {
        pdf_temp.file.write_all(&output.pdf)?;
        pdf_temp.file.flush()?;
        pdf_temp.file.sync_all()?;
        Ok(())
    })() {
        eprintln!("Error writing PDF to temporary file {:?}: {}", pdf_temp.path, e);
        pdf_temp.cleanup();
        if let Some(ref mut r) = report_temp {
            r.cleanup();
        }
        process::exit(4);
    }

    // Write Report directly via retained exclusive handle
    if let (Some(ref mut r_temp), Some(ref r_content)) = (&mut report_temp, &report_json_str) {
        if let Err(e) = (|| -> Result<(), std::io::Error> {
            r_temp.file.write_all(r_content.as_bytes())?;
            r_temp.file.flush()?;
            r_temp.file.sync_all()?;
            Ok(())
        })() {
            eprintln!("Error writing report to temporary file {:?}: {}", r_temp.path, e);
            pdf_temp.cleanup();
            if let Some(ref mut r) = report_temp {
                r.cleanup();
            }
            process::exit(4);
        }
    }

    // Phase 2: Two-phase commit
    let commit_res = commit_two_phase(
        &mut pdf_temp,
        &validated.output_pdf_path,
        report_temp.as_mut(),
        validated.report_json_path.as_deref(),
        validated.overwrite,
    );

    match commit_res {
        Ok(()) => process::exit(0),
        Err(code) => {
            pdf_temp.cleanup();
            if let Some(ref mut r) = report_temp {
                r.cleanup();
            }
            process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_ttc(family_names: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ttcf");
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        let num_fonts = family_names.len() as u32;
        buf.extend_from_slice(&num_fonts.to_be_bytes());

        let offsets_start = buf.len();
        buf.resize(offsets_start + (num_fonts as usize) * 4, 0);

        for (idx, name) in family_names.iter().enumerate() {
            let font_offset = buf.len() as u32;
            let off_idx = offsets_start + idx * 4;
            buf[off_idx..off_idx + 4].copy_from_slice(&font_offset.to_be_bytes());

            buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
            buf.extend_from_slice(&1u16.to_be_bytes());
            buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

            let table_rec_pos = buf.len();
            buf.extend_from_slice(b"name");
            buf.extend_from_slice(&[0, 0, 0, 0]);
            buf.extend_from_slice(&[0, 0, 0, 0]);
            buf.extend_from_slice(&[0, 0, 0, 0]);

            let name_table_pos = buf.len() as u32;
            buf[table_rec_pos + 8..table_rec_pos + 12].copy_from_slice(&name_table_pos.to_be_bytes());

            let utf16_bytes: Vec<u8> = name.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
            let string_offset = 6 + 12;
            buf.extend_from_slice(&0u16.to_be_bytes());
            buf.extend_from_slice(&1u16.to_be_bytes());
            buf.extend_from_slice(&(string_offset as u16).to_be_bytes());

            buf.extend_from_slice(&3u16.to_be_bytes());
            buf.extend_from_slice(&1u16.to_be_bytes());
            buf.extend_from_slice(&0x0409u16.to_be_bytes());
            buf.extend_from_slice(&1u16.to_be_bytes());
            buf.extend_from_slice(&(utf16_bytes.len() as u16).to_be_bytes());
            buf.extend_from_slice(&0u16.to_be_bytes());
            buf.extend_from_slice(&utf16_bytes);

            let table_len = (buf.len() as u32) - name_table_pos;
            buf[table_rec_pos + 12..table_rec_pos + 16].copy_from_slice(&table_len.to_be_bytes());
        }

        buf
    }

    #[test]
    fn test_extract_font_family_names_ttc() {
        let ttc_bytes = build_minimal_ttc(&["Microsoft YaHei", "Microsoft YaHei UI"]);
        let names = extract_font_family_names(&ttc_bytes);
        assert_eq!(names, vec!["Microsoft YaHei", "Microsoft YaHei UI"]);
    }

    #[test]
    fn test_extract_font_family_names_single_font() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

        let table_rec_pos = buf.len();
        buf.extend_from_slice(b"name");
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&[0, 0, 0, 0]);

        let name_table_pos = buf.len() as u32;
        buf[table_rec_pos + 8..table_rec_pos + 12].copy_from_slice(&name_table_pos.to_be_bytes());

        let utf16_bytes: Vec<u8> = "SingleFontFamily"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        let string_offset = 6 + 12;
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&(string_offset as u16).to_be_bytes());

        buf.extend_from_slice(&3u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&0x0409u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&(utf16_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&utf16_bytes);

        let table_len = (buf.len() as u32) - name_table_pos;
        buf[table_rec_pos + 12..table_rec_pos + 16].copy_from_slice(&table_len.to_be_bytes());

        let names = extract_font_family_names(&buf);
        assert_eq!(names, vec!["SingleFontFamily"]);
    }

    #[test]
    fn test_normalize_parent_dir_bare_filename() {
        let bare = Path::new("bare_font.ttf");
        assert_eq!(bare.parent(), Some(Path::new("")));
        let normalized = normalize_parent_dir(bare.parent());
        assert!(!normalized.as_os_str().is_empty(), "Normalized path must not be empty");
        assert_ne!(normalized, PathBuf::from(""));

        let with_dir = Path::new("fonts/bare_font.ttf");
        assert_eq!(normalize_parent_dir(with_dir.parent()), PathBuf::from("fonts"));

        let abs_path = Path::new("/usr/share/fonts/bare_font.ttf");
        assert_eq!(normalize_parent_dir(abs_path.parent()), PathBuf::from("/usr/share/fonts"));
    }

    #[test]
    fn test_inspect_fonts_with_bare_file() {
        let temp_dir = std::env::temp_dir();
        let font_path = temp_dir.join("temp_wordf_test_font.ttf");
        fs::write(&font_path, b"dummy").unwrap();

        let fonts = inspect_fonts(Some(&font_path.to_string_lossy().to_string()));
        assert!(fonts.font_dir.is_some());
        let dir = fonts.font_dir.unwrap();
        assert!(!dir.as_os_str().is_empty(), "font_dir must never be empty string");

        let _ = fs::remove_file(&font_path);
    }

    #[test]
    fn test_font_path_overrides_preset_env() {
        let temp_dir = std::env::temp_dir();
        let font_path = temp_dir.join("test_override_font.ttf");
        fs::write(&font_path, b"dummy").unwrap();

        let old_env = env::var("DOCXIDE_FONTS").ok();
        env::set_var("DOCXIDE_FONTS", "/preset/old/path");

        let cli_font_arg = Some(font_path.to_string_lossy().to_string());
        let discovered = inspect_fonts(cli_font_arg.as_ref());
        if let Some(ref dir) = discovered.font_dir {
            if cli_font_arg.is_some() || env::var("DOCXIDE_FONTS").is_err() {
                env::set_var("DOCXIDE_FONTS", dir);
            }
        }

        let current_env = env::var("DOCXIDE_FONTS").unwrap();
        assert_ne!(current_env, "/preset/old/path");
        assert_eq!(
            current_env,
            normalize_parent_dir(font_path.parent())
                .to_string_lossy()
                .to_string()
        );

        match old_env {
            Some(v) => env::set_var("DOCXIDE_FONTS", v),
            None => env::remove_var("DOCXIDE_FONTS"),
        }
        let _ = fs::remove_file(&font_path);
    }
}
