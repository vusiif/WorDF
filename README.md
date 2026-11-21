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
# Single document conversion
wordf <input.docx> -o <output.pdf> [OPTIONS]

# Parallel batch directory conversion (auto-detected when input is a directory)
wordf <input_dir> -o <output_dir> [-j 4] [OPTIONS]

wordf --help
wordf --version
```

### Options
- `-o, --output <path>`: Required output PDF file path, or output directory when input is a directory (`-` stdout rejected).
- `--format <auto|doc|docx>`: Explicit format declaration (default: `auto`).
- `--overwrite`: Allow replacing existing regular destination files. Default is race-safe no-clobber.
- `--report-json <path>`: Optional path to write a machine-readable JSON execution report (`-` rejected).
- `--font-path <path>`: Optional local font file or directory for offline font injection.
- `-j, --jobs <N>`: Worker thread count for batch directory conversion (default: logical CPU count, zero external dependencies).
- `-q, --quiet`: Suppress all diagnostic warnings on stderr.
- `-v, --verbose`: Print detailed phase timings, font policy, and progress diagnostics to stderr.
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
   - *Phase 1*: Outputs are staged into unpredictable, exclusive temporary files (`.wordf_tmp_*`) in the target's parent directory with `0600` permissions (or Windows secure exclusive handles).
   - *Phase 2 (Atomic Commit)*:
     - PDF is committed first. Without `--overwrite`, atomic non-clobber commit ensures no existing file is clobbered (using `renameat2(..., RENAME_NOREPLACE)` on Linux and `MoveFileExW` on Windows).
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

### Distributable Offline Embedded CJK Font Scheme
WorDF embeds a legally redistributable, offline-first default CJK TrueType font:
- **Font Name**: LXGW WenKai Lite (霞鹜文楷 轻便版) Regular
- **Version**: v1.530 (Release 2026-10-18)
- **Source**: [lxgw/LxgwWenKai-Lite](https://github.com/lxgw/LxgwWenKai-Lite/releases/tag/v1.530)
- **SHA256**: `f799a538a3355877e597a0ae04050e897932c9ae7689eb9227c2953c5dd549ea`
- **License**: SIL Open Font License 1.1 (OFL-1.1). See [assets/fonts/OFL.txt](assets/fonts/OFL.txt) and [assets/fonts/NOTICE.txt](assets/fonts/NOTICE.txt).
- **Redistribution**: Freely redistributable, commercially usable, and bundled directly within the binary. No user font installation, system font configuration, or network access is required.

### Resolution & Fallback Priority
When converting documents with East Asian / CJK text, font resolution follows a strict three-tier hierarchy:
1. **Explicit Parameter (Highest Priority)**:
   - CLI: `--font-path <path>` (file or directory).
   - Core API: `ConversionOptions::fallback_font_name` and `fallback_font_bytes`.
   - Unconditionally overrides both environment variables and the embedded default font.
2. **Environment Variable**:
   - `DOCXIDE_FONTS`: Process-level path pointing to directories or font files.
   - Used when no explicit CLI `--font-path` is provided.
3. **Embedded Default Font (Fallback)**:
   - Built-in `LXGW WenKai Lite` in memory.
   - Active automatically when neither explicit parameters nor `DOCXIDE_FONTS` are specified, guaranteeing 100% offline self-contained conversion in bare container or server environments.

### Character Coverage & Fail-Fast Guarantee
- **Covered Range**: China National Standard GB 2312-80 (6,763 simplified Chinese characters), General Standard Chinese Characters List (通用规范汉字表 8,105 characters), ASCII, standard punctuation, and Latin symbols.
- **Fail-Fast Diagnostic**: WorDF pre-flights every text run against the font's Unicode CMap (Format 4 and Format 12). If a document contains characters not supported by the active font, WorDF immediately halts with a typed `MissingFont` error (CLI exit code 3) detailing the missing characters, ensuring no characters are silently dropped.

---

## Cargo Feature Flags & Binary Footprint

WorDF is engineered for ultra-lightweight deployments:

| Cargo Feature Flag | Default | Release Size | Description |
|---|:---:|:---:|---|
| `embedded-cjk-font` | **Yes** | **~12.8 MB** | Bundles Deflate-compressed CJK font in binary with lazy runtime decompression. Zero external setup needed. |
| `--no-default-features` | No | **~4.9 MB** | Disables font bundling for cloud, container, and embedded environments where system fonts are already present or for non-CJK workloads. |

---

## Release Packaging & Automation

WorDF provides zero-external-dependency automated packaging scripts for cross-platform releases:

- **Windows PowerShell**:
  ```powershell
  # Package both full and lite zip distributions with SHA256 checksums
  powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -Flavor all
  ```
- **POSIX Bash (Linux / macOS)**:
  ```bash
  ./scripts/release.sh all
  ```

Distribution artifacts, bundled documentation, and cryptographic `SHA256SUMS.txt` are placed in the `dist/` directory.

