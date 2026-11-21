# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
