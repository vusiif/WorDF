//! CLI help text, tool banners, and version information.
//! License: GPL-3.0-or-later

pub const TOOL_NAME: &str = "wordf";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn print_help() {
    println!("WorDF: Independent DOC / DOCX to PDF converter in Rust");
    println!("License: GPL-3.0-or-later");
    println!();
    println!("Usage:");
    println!("  wordf <input> -o <output> [OPTIONS]");
    println!("  wordf --help");
    println!("  wordf --version");
    println!();
    println!("Options:");
    println!(
        "  -o, --output <path>       Output PDF path (or directory when input is a directory)"
    );
    println!("      --format <FORMAT>     Expected input format: auto (default), doc, or docx");
    println!("      --overwrite           Allow overwriting existing ordinary output files");
    println!("      --strict              Fail with non-zero exit code on any diagnostic warning");
    println!("      --analyze             Inspect and diagnose document compatibility without generating PDF");
    println!(
        "      --pages <RANGE>       Export specific page ranges (e.g. '1-3', '2', '1-3,5,8-10')"
    );
    println!("      --password <string>   Decryption password for password-protected documents");
    println!(
        "      --password-prompt     Prompt interactively for decryption password without echo"
    );
    println!("      --password-file <path> Read decryption password from specified file");
    println!("      --password-stdin      Read decryption password from standard input");
    println!("      --password-env <VAR>  Read decryption password from environment variable");
    println!(
        "      --title <string>      Set or override document Title metadata in generated PDF"
    );
    println!("      --author <string>     Set or override document Author/Creator metadata in generated PDF");
    println!(
        "      --subject <string>    Set or override document Subject metadata in generated PDF"
    );
    println!(
        "      --keywords <string>   Set or override document Keywords metadata in generated PDF"
    );
    println!("      --resume              Skip conversion if target PDF already exists and is newer than input");
    println!("      --report-json <path>  Emit machine-readable JSON execution report");
    println!("      --font-path <path>    Local font file or directory for offline CJK injection");
    println!(
        "  -j, --jobs <N>            Worker threads for batch conversion (default: logical CPUs)"
    );
    println!("  -r, --recursive           Recursively scan subdirectories in batch conversion");
    println!("  -q, --quiet               Suppress all diagnostic warnings on stderr");
    println!("  -v, --verbose             Print detailed phase timings and diagnostics to stderr");
    println!("  -h, --help                Show this help message and exit");
    println!("  -V, --version             Show version and exit");
}

pub fn print_version() {
    println!("{} {}", TOOL_NAME, TOOL_VERSION);
}
