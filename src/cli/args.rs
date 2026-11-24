//! CLI argument parsing, flags, help, and error code mapping.
//! License: GPL-3.0-or-later

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process;

use wordf::{ConversionErrorKind, InputFormat};

pub use crate::cli::help::{print_help, print_version, TOOL_NAME, TOOL_VERSION};
use crate::cli::password::PasswordOptions;

#[derive(Default, Clone, Debug)]
pub struct CliArgs {
    pub input_path: Option<PathBuf>,
    pub output_pdf: Option<PathBuf>,
    pub format: InputFormat,
    pub overwrite: bool,
    pub report_json: Option<PathBuf>,
    pub font_path: Option<String>,
    pub font_paths: Vec<String>,
    pub jobs: Option<usize>,
    pub recursive: bool,
    pub strict: bool,
    pub analyze: bool,
    pub resume: bool,
    pub pages: Option<Vec<usize>>,
    pub password: Option<String>,
    pub metadata: wordf::DocumentMetadata,
    pub quiet: bool,
    pub verbose: bool,
}

pub fn parse_cli_args() -> Result<CliArgs, i32> {
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
    let mut pwd_opts = PasswordOptions::default();
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
                    eprintln!(
                        "Error: Unknown format '{}'. Expected 'auto', 'doc', or 'docx'",
                        other
                    );
                    return Err(2);
                }
            }
        } else if arg_str == "--overwrite" {
            parsed.overwrite = true;
        } else if arg_str == "--strict" {
            parsed.strict = true;
        } else if arg_str == "--analyze" {
            parsed.analyze = true;
        } else if arg_str == "--resume" {
            parsed.resume = true;
        } else if arg_str == "--pages" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--pages'");
                return Err(2);
            }
            let raw = args[i].to_string_lossy();
            let parsed_range = match wordf::pages::PageRange::parse(&raw) {
                Ok(pr) => pr,
                Err(err_msg) => {
                    eprintln!("Error: Invalid --pages specification: {}", err_msg);
                    return Err(2);
                }
            };
            parsed.pages = Some(parsed_range.pages);
        } else if arg_str == "--password" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--password'");
                return Err(2);
            }
            pwd_opts.raw = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--password-prompt" {
            pwd_opts.prompt = true;
        } else if arg_str == "--password-file" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--password-file'");
                return Err(2);
            }
            pwd_opts.file = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--password-stdin" {
            pwd_opts.stdin = true;
        } else if arg_str == "--password-env" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--password-env'");
                return Err(2);
            }
            pwd_opts.env_var = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--title" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--title'");
                return Err(2);
            }
            parsed.metadata.title = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--author" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--author'");
                return Err(2);
            }
            parsed.metadata.author = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--subject" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--subject'");
                return Err(2);
            }
            parsed.metadata.subject = Some(args[i].to_string_lossy().to_string());
        } else if arg_str == "--keywords" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '--keywords'");
                return Err(2);
            }
            parsed.metadata.keywords = Some(args[i].to_string_lossy().to_string());
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
            let raw = args[i].to_string_lossy().to_string();
            for part in raw.split(',') {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    parsed.font_paths.push(trimmed.to_string());
                }
            }
            if parsed.font_path.is_none() {
                parsed.font_path = parsed.font_paths.first().cloned();
            }
        } else if arg_str == "-j" || arg_str == "--jobs" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: Missing argument for '{}'", arg_str);
                return Err(2);
            }
            let jobs_val = match args[i].to_string_lossy().parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => {
                    eprintln!(
                        "Error: Invalid jobs count '{}'. Expected positive integer.",
                        args[i].to_string_lossy()
                    );
                    return Err(2);
                }
            };
            parsed.jobs = Some(jobs_val);
        } else if arg_str == "-r" || arg_str == "--recursive" {
            parsed.recursive = true;
        } else if arg_str == "-q" || arg_str == "--quiet" {
            parsed.quiet = true;
        } else if arg_str == "-v" || arg_str == "--verbose" {
            parsed.verbose = true;
        } else if arg_str.starts_with('-') && arg_str != "-" {
            eprintln!("Error: Unknown or unsupported option '{}'", arg_str);
            return Err(2);
        } else {
            if let Some(ref existing) = parsed.input_path {
                eprintln!("Error: Multiple inputs provided ('{:?}' and '{:?}'). WorDF processes exactly one target file or directory.", existing, arg_str);
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

    if parsed.output_pdf.is_none() && !parsed.analyze {
        eprintln!("Error: Mandatory output path '-o <output.pdf>' is missing.");
        return Err(2);
    }

    if parsed.quiet && parsed.verbose {
        eprintln!("Error: Cannot specify both '--quiet' and '--verbose'.");
        return Err(2);
    }

    parsed.password = pwd_opts.validate_and_resolve()?;

    Ok(parsed)
}

pub fn map_conversion_error_to_exit_code(err: &ConversionErrorKind) -> i32 {
    err.exit_code()
}
