# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **Exit-code contract unification**: oversized inputs (> 128 MiB) now consistently exit `6` on every route (single-file previously exited `4`, stdin `1`); batch partial failure exit `1` and the corrected meaning of exit `2` are documented in the README exit-code table.
- **Silent JSON report failures**: the stdout delivery route and strict-mode route now surface report write/commit errors on stderr; a stdout-route report failure exits `9` per contract instead of silently exiting `0`.
- **Batch destination safety parity**: batch items now reject symlink destinations, directory destinations, and output/input entity aliases with the same rules as single-file mode (a symlink output is rejected, never silently replaced).
- **Batch report failure exit code**: batch JSON report failures now exit `9` when PDF outputs were already committed (previously `4`), matching the README contract; report targets are validated up front so an invalid `--report-json` path fails before any conversion work.
- **Case-variant collision detection**: batch output collision detection folds case only on case-insensitive filesystems (mirroring the single-file alias checks), so case-sensitive filesystems no longer reject genuinely distinct outputs.
- **Non-UTF-8 file names**: batch conversion no longer fails documents whose file names are not valid UTF-8 (e.g. GBK names on Linux); the output PDF preserves the original stem byte-for-byte.
- **Bounded input reads**: single-file and batch input reads are hard-capped at 128 MiB + 1 byte, closing the gap between the metadata size check and the actual read.
- **README accuracy**: stdout `-` streaming support, filesystem access during font resolution, batch directory creation behavior, previously undocumented CLI options, and exit-code semantics corrected to match actual behavior.

### Changed
- Repository-wide `cargo fmt` normalization; `cargo fmt --check` now passes with zero diffs.
- Added GitHub Actions CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets` on Windows, Linux and macOS, plus a `cargo audit` security job (expected to stay red until the zip advisories below are upgraded away).
- Restored the project's ≤ 292 total line limit across all 47 source files after `cargo fmt` pushed six over it: split `crypto::agile` (parser / test builders / tests), `analyze` (engine / console rendering), `cli::single` (orchestrator / JSON report), batch report staging into `cli::batch::report`, the `--analyze` CLI route into `cli::analyze_route`, and moved the embedded CMap cache into `font`.

### Security
- `cargo audit` baseline recorded: 14 advisories across `zip` 2.6.1 (6) and `zip` 8.6.0 (8, including one 9.1 critical) plus 2 unmaintained warnings (`rustybuzz`, `ttf-parser`). All flagged code paths (`ZipArchive::extract`, `ZipStreamReader`, AES entries, legacy shrink/reduce/implode decoders) are either feature-disabled (`default-features = false, features = ["deflate"]`) or never called by WorDF — reachability verified by grep; upgrades tracked in `.agents/HANDOFF.md`.

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
