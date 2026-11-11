# WorDF

WorDF is an independent, offline-first DOC / DOCX to PDF converter written in Rust.

## License

WorDF is licensed under the GNU General Public License v3.0 or later (GPL-3.0-or-later). See [LICENSE](LICENSE) for the full text.

---

## Architecture & Core Conversion Routes

WorDF performs conversion without depending on LibreOffice, Microsoft Office, or external cloud services:
- **Binary DOC (Word 97–2003)**: Parsed via `rwml` 0.1.4, bridged into OOXML DOCX, and rendered to PDF via `docxide-pdf` (Route B, preview-grade layout).
- **OOXML DOCX**: Rendered directly to PDF via vendor-patched `docxide-pdf` 0.19.0 (Route C).

---

## Rust Core Library Public API

The core library provides a pure-memory byte-level API that does not touch the filesystem, does not spawn background processes, and produces no standard I/O side effects:

```rust
use wordf::{convert, ConversionOptions, InputFormat, ConversionErrorKind};

let input_docx_bytes: &[u8] = ...;
let options = ConversionOptions {
    format: InputFormat::Auto,
    fallback_font_name: Some("SimSun".to_string()),
    ..Default::default()
};

match convert(input_docx_bytes, &options) {
    Ok(output) => {
        println!("Generated PDF bytes: {}", output.pdf.len());
        println!("Real positive page count: {}", output.page_count);
        for warning in output.warnings {
            eprintln!("Warning [{}]: {}", warning.code, warning.message);
        }
    }
    Err(e) => {
        eprintln!("Conversion error [{}]: {}", e.kind, e.message);
    }
}
```

### Typed Error Classifications (`ConversionErrorKind`)
- `UnsupportedFormat`: Non-Word inputs (RTF, HTML, plain text, spreadsheets).
- `UnsupportedVersion`: Legacy or unsupported Word versions.
- `Encrypted`: Password-protected or encrypted documents.
- `CorruptInput`: Damaged ZIP/CFB containers or invalid FIB blocks.
- `ResourceLimit`: Defensible resource limits exceeded (size, entries, decompression ratio, image resolution).
- `MissingFont`: Font lacks required CJK glyphs (fail-fast diagnostic).
- `BackendFailure`: Low-level rendering failure or 0-page PDF output.
- `Internal`: Unrecoverable internal error.

---

## Command Line Interface (CLI)

### Usage

```bash
wordf <input> -o <output.pdf> [OPTIONS]
wordf --help
wordf --version
```

### Options
- `-o, --output <path>`: Required output PDF path (`-` stdout rejected).
- `--format <auto|doc|docx>`: Explicit format declaration (default: `auto`).
- `--overwrite`: Allow replacing existing regular destination files. Default is race-safe no-clobber.
- `--report-json <path>`: Optional path to write a machine-readable JSON execution report (`-` rejected).
- `--font-path <path>`: Optional local font file or directory for offline font injection.
- `-h, --help`: Display help and exit (written to `stdout`).
- `-V, --version`: Display version information and exit (written to `stdout`).

### Standard Streams Contract
- During normal file conversion, **`stdout` remains completely empty (0 bytes written)**.
- All diagnostics, errors, and warning logs are written strictly to **`stderr`**.

### Stable Exit Codes
| Exit Code | Meaning | Destination PDF State |
|:---:|---|:---:|
| **0** | Conversion and all requested outputs committed successfully | Committed |
| **2** | CLI argument errors, alias conflicts, stdin/stdout `-`, deprecated P0 syntax | Not committed |
| **3** | Unsupported format, explicit format mismatch, encrypted file, missing font glyph | Not committed |
| **4** | File I/O, parent directory missing, existing file without `--overwrite`, symlink target | Not committed |
| **5** | Corrupt or invalid Word container / stream | Not committed |
| **6** | Defensible resource budget exceeded (size, entries, ratio, pixels) | Not committed |
| **7** | User cancellation (reserved) | Not committed |
| **8** | Internal backend failure, zero-page PDF output | Not committed |
| **9** | PDF committed successfully, but requested JSON report commit failed | **Committed** |

---

## Path Isolation & Two-Phase Output Transactions

WorDF enforces filesystem boundaries to prevent accidental data loss:
1. **Physical Entity Isolation**: Input file, output PDF, and JSON report must be distinct file entities. Hard links, symlinks, and aliases pointing to the same file entity are rejected before conversion.
2. **Non-existent Target Collision**: Distinct non-existent output paths resolving to the same parent directory and filename are rejected with exit code 2.
3. **Symlink Rejection**: Destination PDF and report paths must not be symlinks; symlinks are rejected with exit code 4.
4. **Parent Directory Existence**: Output parent directories must exist; WorDF does not create directories implicitly.
5. **Two-Phase Commit**:
   - *Phase 1*: Outputs are staged into unpredictable, exclusive temporary files (`.wordf_tmp_*`) in the target's parent directory with `0600` permissions.
   - *Phase 2 (Atomic Commit)*:
     - PDF is committed first. Without `--overwrite`, atomic `renameat2(..., RENAME_NOREPLACE)` ensures no existing file is clobbered.
     - JSON report is committed second. If report commit fails, **the successfully committed PDF is preserved**, and WorDF exits with exit code 9 (`output.report_commit_failed`).

---

## Defensible Resource Limits & Defenses

WorDF enforces defensive bounds against denial-of-service and malicious inputs:
- **Maximum Input Size**: 128 MiB (`134,217,728` bytes).
- **DOCX ZIP Entries**: Maximum 1,000 entries.
- **DOCX Cumulative Decompression**: Maximum 64 MiB (`67,108,864` bytes).
- **DOCX Main Document XML**: Maximum 16 MiB (`16,777,216` bytes).
- **DOCX Auxiliary XML / Relations**: Maximum 2 MiB per part.
- **DOCX Media Items**: Maximum 20 MiB per item.
- **Compression Ratio**: Capped at 100 : 1 for parts expanding beyond 1 MiB (ZIP bomb defense).
- **Image Resolution**: Maximum 8,192 px per side, maximum 16,777,216 total pixels.
- **DOC CFB Streams**: Maximum 1,000 streams.
- **DOC WordDocument Stream**: Maximum 64 MiB.

---

## Font Architecture & Fallback Disclosure

- **Underlying Font Resolution**: The underlying `docxide-pdf` layout engine resolves fonts using the process-level `DOCXIDE_FONTS` environment variable and system font directories. Callers injecting CJK fallback fonts via the CLI or public API should ensure that the directory containing the font is accessible via `DOCXIDE_FONTS` or system font directories. When `--font-path` is explicitly specified via the CLI, it unconditionally overrides any pre-set `DOCXIDE_FONTS` environment variable.
- **Distribution Notice**: Testing fonts used during validation are private local test files and are not embedded or bundled into the distributable source code.
