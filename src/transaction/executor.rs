//! Single task execution engine with atomic transactional isolation and quality gating.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

use crate::format::MAX_INPUT_BYTES;
use crate::transaction::{atomic_commit_rename, create_exclusive_temp_file};
use crate::{convert, ConversionError, ConversionOptions, ConversionOutput, Diagnostic};

/// Execution parameters for a single file-to-file conversion task.
#[derive(Clone, Debug)]
pub struct TaskFileConfig<'a> {
    pub input_path: &'a Path,
    pub output_pdf_path: &'a Path,
    pub options: &'a ConversionOptions,
    pub strict: bool,
    pub overwrite: bool,
}

/// Execution output of a successful conversion task.
#[derive(Clone, Debug)]
pub struct TaskResult {
    pub output: ConversionOutput,
    pub conversion_ms: f64,
    pub total_ms: f64,
}

/// Strongly typed task execution failure.
#[derive(Debug)]
pub enum TaskError {
    /// Input exceeds defended size limit (128 MiB).
    SizeLimitExceeded(usize),
    /// I/O read failure.
    IoRead(std::io::Error),
    /// Conversion engine failure.
    Conversion(ConversionError),
    /// Strict quality mode rejected document due to warnings.
    StrictRejected(Vec<Diagnostic>),
    /// Temp file creation failure (exit code).
    TempFile(i32),
    /// I/O write failure.
    IoWrite(std::io::Error),
    /// Atomic commit rename failure (exit code).
    Commit(i32),
}

impl TaskError {
    /// Get the corresponding CLI process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::SizeLimitExceeded(_) => 6,
            Self::IoRead(_) | Self::IoWrite(_) => 4,
            Self::Conversion(e) => e.kind.exit_code(),
            Self::StrictRejected(_) => 3,
            Self::TempFile(c) | Self::Commit(c) => *c,
        }
    }
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeLimitExceeded(len) => {
                write!(
                    f,
                    "Input file size ({} bytes) exceeds 128 MiB maximum limit",
                    len
                )
            }
            Self::IoRead(e) => write!(f, "I/O read error: {}", e),
            Self::Conversion(e) => write!(f, "Conversion error: {}", e),
            Self::StrictRejected(warns) => {
                write!(
                    f,
                    "Strict mode rejected document: {} warning(s) present",
                    warns.len()
                )
            }
            Self::TempFile(c) => write!(f, "Exclusive temp file creation failed (exit code {})", c),
            Self::IoWrite(e) => write!(f, "I/O write error: {}", e),
            Self::Commit(c) => write!(f, "Atomic commit rename failed (exit code {})", c),
        }
    }
}

impl std::error::Error for TaskError {}

/// Unified execution pipeline for isolated conversion tasks.
pub struct TaskExecutor;

impl TaskExecutor {
    /// Execute a file-to-file conversion task with atomic commit and strict checking.
    pub fn execute_file_to_file(config: &TaskFileConfig<'_>) -> Result<TaskResult, TaskError> {
        let t_start = Instant::now();

        // 1. Guard against OOM before reading file
        if let Ok(meta) = config.input_path.metadata() {
            let len = meta.len() as usize;
            if len > MAX_INPUT_BYTES {
                return Err(TaskError::SizeLimitExceeded(len));
            }
        }

        // 2. Read input file with a hard cap so a file growing between the
        //    metadata check and the read cannot exhaust memory.
        let mut bytes = Vec::new();
        let file = fs::File::open(config.input_path).map_err(TaskError::IoRead)?;
        let mut handle = file.take(MAX_INPUT_BYTES as u64 + 1);
        handle.read_to_end(&mut bytes).map_err(TaskError::IoRead)?;
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(TaskError::SizeLimitExceeded(bytes.len()));
        }

        // 3. Execute conversion with timing
        let t_conv = Instant::now();
        let mut output = convert(&bytes, config.options).map_err(TaskError::Conversion)?;
        let conversion_ms = t_conv.elapsed().as_secs_f64() * 1000.0;

        // 4. Extension mismatch diagnostic
        Self::append_extension_diagnostics(config.input_path, &bytes, &mut output.warnings);

        // 5. Strict quality gating
        if config.strict && !output.warnings.is_empty() {
            return Err(TaskError::StrictRejected(output.warnings));
        }

        // 6. Two-phase exclusive temp file + atomic rename commit
        let mut pdf_guard = create_exclusive_temp_file(config.output_pdf_path, "pdf")
            .map_err(TaskError::TempFile)?;

        if let Err(e) = pdf_guard.file.write_all(&output.pdf) {
            pdf_guard.cleanup();
            return Err(TaskError::IoWrite(e));
        }

        if let Err(e) = pdf_guard.file.flush() {
            pdf_guard.cleanup();
            return Err(TaskError::IoWrite(e));
        }

        atomic_commit_rename(&pdf_guard.path, config.output_pdf_path, config.overwrite).map_err(
            |_| {
                pdf_guard.cleanup();
                TaskError::Commit(4)
            },
        )?;
        pdf_guard.committed = true;

        Ok(TaskResult {
            output,
            conversion_ms,
            total_ms: t_start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Check if container magic matches file extension, appending diagnostic if mismatched.
    pub fn append_extension_diagnostics(path: &Path, bytes: &[u8], warnings: &mut Vec<Diagnostic>) {
        let ext_lower = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let is_doc = bytes.starts_with(&crate::format::DOC_MAGIC);
        let is_docx = bytes.starts_with(&crate::format::ZIP_MAGIC);

        if is_docx && ext_lower == "doc" {
            warnings.insert(
                0,
                Diagnostic {
                    code: "input.extension_mismatch".to_string(),
                    message: "Input file extension '.doc' does not match verified container format 'docx'".to_string(),
                },
            );
        } else if is_doc && ext_lower == "docx" {
            warnings.insert(
                0,
                Diagnostic {
                    code: "input.extension_mismatch".to_string(),
                    message: "Input file extension '.docx' does not match verified container format 'doc'".to_string(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_error_exit_codes() {
        assert_eq!(TaskError::SizeLimitExceeded(100).exit_code(), 6);
        assert_eq!(
            TaskError::IoRead(std::io::Error::new(std::io::ErrorKind::NotFound, "err")).exit_code(),
            4
        );
        assert_eq!(TaskError::StrictRejected(Vec::new()).exit_code(), 3);
        assert_eq!(TaskError::TempFile(2).exit_code(), 2);
        assert_eq!(TaskError::Commit(4).exit_code(), 4);
    }

    #[test]
    fn test_append_extension_diagnostics() {
        let mut warns = Vec::new();
        // Matching extension -> no warnings
        TaskExecutor::append_extension_diagnostics(
            Path::new("test.docx"),
            &crate::format::ZIP_MAGIC,
            &mut warns,
        );
        assert!(warns.is_empty());

        // Mismatched extension -> warning inserted
        TaskExecutor::append_extension_diagnostics(
            Path::new("test.doc"),
            &crate::format::ZIP_MAGIC,
            &mut warns,
        );
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].code, "input.extension_mismatch");
    }
}
