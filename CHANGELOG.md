# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-11-26

### Added
- **Agile Encryption & Integrity Verification**:
  - Structured streaming XML parser (`quick-xml`) for `<keyData>`, `<dataIntegrity>`, and `<encryptedKey>` supporting arbitrary formatting, namespaces, and strict rejection of unsupported ciphers/modes.
  - Full MS-OFFCRYPTO `dataIntegrity` HMAC-SHA1/SHA256/SHA512 verification in constant time, strict payload length checks, and `ZeroizeBuffer` RAII wiping for derived keys and password buffers.
  - Zero-heap-allocation fixed stack buffers inside the KDF spin loop (`derive_password_hash`).
- **Secure Password Carriers**:
  - Added `--password-prompt`, `--password-file <path>`, `--password-stdin`, and `--password-env <VAR>` with mutual-exclusion validation (exit code `2`) and `Debug` redaction on `CliArgs` and `ConversionOptions`.
- **Unified `FontResolver` & Cascading Fallback**:
  - Unified font glyph coverage and fallback decisions across `--analyze`, DOCX XML adaptation, and backend rendering, with `Cow<'static, [u8]>` zero-copy embedded font borrowing and multi-directory `DOCXIDE_FONTS` propagation.
- **Versioned `--resume` Fingerprinting**:
  - Replaced mtime-only checks with SHA-256 canonical conversion fingerprints (input hash, engine version, format, font policy, pages, metadata, strict flag, salted password hash) plus `%PDF-` / `%%EOF` output integrity validation.
  - Encrypted inputs never bypass password authentication under `--resume`; single-file skips emit `"conversion_status": "skipped"` JSON reports.

### Changed
- **Single-Pass DOCX Processing**: Merged CJK font adaptation and `docProps/core.xml` metadata injection into a single ZIP pass using `raw_copy_file` for unmodified entries.
- **DOC Analysis Parity**: `analyze_doc_package` now bridges via `rwml` to produce accurate structural statistics and `layout.approximate` diagnostics.
- **Batch Concurrency & Filesystem Safety**:
  - Capped `MAX_BATCH_WORKERS` to `16` to bound peak memory across concurrent 128 MiB tasks.
  - Unified `TaskExecutor::read_bounded_file` across single-file, batch, and `--analyze` routes; eliminated triple-read in batch `--resume` and double-decrypt in single-file mode.
  - Dynamic directory case-sensitivity probing (`is_dir_case_insensitive`), `OsStr`/`OsString` preservation for non-UTF-8 filenames, and empty-directory `--report-json` commit.
  - Hardened DOCX media dimension fallback read with `MAX_DOCX_MEDIA_BYTES`.

### Security & CI
- Added GitHub Actions CI (`ubuntu-latest`, `windows-latest`, `macos-latest`) running `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`, and `cargo audit` (0 blocked vulnerabilities).

## [1.0.0] - 2026-11-21

### Added
- **Core Conversion Engine**:
  - Direct in-memory conversion of OOXML `.docx` documents to PDF (Route C).
  - Word 97–2003 binary `.doc` conversion via CFB parsing and internal DOCX bridge (Route B).
  - Pure-memory public API (`wordf::convert`) with zero filesystem side effects and zero background thread leaks.
- **CJK & Multilingual Typography**:
  - Embedded legally redistributable, SIL OFL-1.1 licensed default CJK TrueType font (`LXGW WenKai Lite` v1.530).
  - Pre-flight Unicode CMap (Format 4 and 12) coverage verification with fail-fast diagnostics on unsupported glyphs.
  - Three-tier offline font resolution: CLI `--font-path` > environment variable `DOCXIDE_FONTS` > built-in CJK font.
  - Full document styles inheritance: `<w:docDefaults>` and Normal style font adaptation with East Asian and Western font bifurcation.
- **Robustness & Two-Phase Filesystem Transactions**:
  - Unpredictable exclusive temporary files (`.wordf_tmp_*`) with restrictive file permissions.
  - Race-safe atomic commit via `MoveFileExW` on Windows and `renameat2(RENAME_NOREPLACE)` on POSIX.
  - Transient conflict retry with exponential backoff on Windows.
  - Strict input/output entity collision prevention.
- **CLI Productivity & Concurrency**:
  - Quiet mode (`-q, --quiet`) to suppress diagnostic warnings on stderr.
  - Verbose mode (`-v, --verbose`) for phase timings, memory statistics, and font policy inspection.
  - Directory batch conversion (`wordf <input_dir> -o <output_dir>`) with zero external concurrency dependencies using scoped `std::thread::scope`.
  - Configurable worker count (`-j, --jobs <N>`) with size-descending job scheduling to eliminate long-tail stalls.
  - Automated filtering of Microsoft Office temporary and lock files (`~$*`).
  - Machine-readable JSON execution reporting (`--report-json`) complying with Schema v1.0.
- **Footprint & Packaging**:
  - Automated multi-target release packager for Windows PowerShell and POSIX Bash.
  - Ultra-compact binary footprint: Full release (~12.8 MB) and Lite release (~4.9 MB binary / 2.29 MB ZIP).
