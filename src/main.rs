//! WorDF P1 CLI interface
//! License: GPL-3.0-or-later

mod cli;

use std::env;
use std::process;
use std::time::Instant;

use cli::{
    analyze_route::run_analyze_route, inspect_font_chain, parse_cli_args, run_batch_conversion,
    run_single_conversion, BatchConfig, SingleConfig,
};
use wordf::transaction::{validate_and_isolate_targets, TargetPaths};
use wordf::ConversionOptions;

fn main() {
    let t_start = Instant::now();

    let mut args = match parse_cli_args() {
        Ok(a) => a,
        Err(code) => process::exit(code),
    };

    let input_path = args.input_path.take().unwrap();

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

    let (fallback_name, fallback_bytes, font_policy) =
        if let Some(ref fb) = discovered_fonts.primary_cjk_fallback {
            let policy = if !args.font_paths.is_empty() {
                "explicit-font-path"
            } else {
                "env-docxide-fonts"
            };
            (
                Some(fb.clone()),
                discovered_fonts.primary_cjk_font_bytes.clone(),
                policy,
            )
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
    if fallback_chain.is_empty()
        && args.font_paths.is_empty()
        && wordf::font::has_embedded_cjk_font()
    {
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
        run_analyze_route(&args, &input_path, &options);
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
