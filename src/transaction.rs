//! WorDF file system isolation and two-phase output transactions.
//! License: GPL-3.0-or-later

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::process;

pub struct TargetPaths {
    pub input_path: PathBuf,
    pub output_pdf_path: PathBuf,
    pub report_json_path: Option<PathBuf>,
    pub overwrite: bool,
}

pub struct ValidatedTargets {
    pub input_canonical: PathBuf,
    pub output_pdf_path: PathBuf,
    pub report_json_path: Option<PathBuf>,
    pub overwrite: bool,
}

/// Pre-flight validation of input and output targets according to spec §§4.2–4.4 and plan review §2.2.
/// Returns Ok(ValidatedTargets) or exits with the appropriate exit code:
/// - Exit code 2: Parameter/alias conflict, same file entity, or non-existent target collision.
/// - Exit code 4: File I/O, parent directory missing, symlink target rejected, or output exists without --overwrite.
pub fn validate_and_isolate_targets(targets: TargetPaths) -> Result<ValidatedTargets, i32> {
    // 1. Input file checks
    let input_path = &targets.input_path;
    if input_path.as_os_str() == "-" {
        eprintln!("Error: stdin input '-' is not supported by WorDF P1 contract.");
        return Err(2);
    }

    let input_meta = match fs::symlink_metadata(input_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error reading input file {:?}: {}", input_path, e);
            return Err(4);
        }
    };

    if !input_meta.is_file() {
        eprintln!("Error: Input {:?} is not a regular file.", input_path);
        return Err(4);
    }

    let input_dev = input_meta.dev();
    let input_ino = input_meta.ino();

    let pdf_path = &targets.output_pdf_path;

    // 2. Immediate alias checks against input
    if let Ok(m) = fs::metadata(pdf_path) {
        if m.dev() == input_dev && m.ino() == input_ino {
            eprintln!("Error: Output PDF {:?} points to the same file entity as input.", pdf_path);
            return Err(2);
        }
    }
    check_future_target_alias(pdf_path, input_path, "PDF output", "input file")?;

    // 3. Report alias checks against input & PDF
    if let Some(ref report_path) = targets.report_json_path {
        if let Ok(m) = fs::metadata(report_path) {
            if m.dev() == input_dev && m.ino() == input_ino {
                eprintln!("Error: JSON report {:?} points to the same file entity as input.", report_path);
                return Err(2);
            }
        }
        check_future_target_alias(report_path, input_path, "JSON report", "input file")?;

        if let (Ok(m_pdf), Ok(m_rep)) = (fs::metadata(pdf_path), fs::metadata(report_path)) {
            if m_pdf.dev() == m_rep.dev() && m_pdf.ino() == m_rep.ino() {
                eprintln!("Error: Output PDF and JSON report point to the same existing file entity.");
                return Err(2);
            }
        }
        check_future_target_alias(pdf_path, report_path, "PDF output", "JSON report")?;
    }

    // 4. Output PDF structure & existence checks
    if pdf_path.as_os_str() == "-" {
        eprintln!("Error: stdout output '-' is not supported by WorDF P1 contract.");
        return Err(2);
    }
    validate_single_output_target(pdf_path, targets.overwrite, "PDF output")?;

    // 5. Report JSON structure & existence checks (if provided)
    if let Some(ref report_path) = targets.report_json_path {
        if report_path.as_os_str() == "-" {
            eprintln!("Error: stdout report '-' is not supported by WorDF P1 contract.");
            return Err(2);
        }
        validate_single_output_target(report_path, targets.overwrite, "JSON report")?;
    }

    Ok(ValidatedTargets {
        input_canonical: input_path.clone(),
        output_pdf_path: targets.output_pdf_path,
        report_json_path: targets.report_json_path,
        overwrite: targets.overwrite,
    })
}

fn validate_single_output_target(path: &Path, overwrite: bool, kind: &str) -> Result<(), i32> {
    // Parent directory check
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let parent_meta = match fs::metadata(parent) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: Parent directory for {} {:?} does not exist or cannot be accessed: {}", kind, path, e);
            return Err(4);
        }
    };

    if !parent_meta.is_dir() {
        eprintln!("Error: Parent path {:?} for {} is not a directory.", parent, kind);
        return Err(4);
    }

    // Existing target check
    if let Ok(m) = fs::symlink_metadata(path) {
        if m.is_symlink() {
            eprintln!("Error: Refusing to output to symlink {:?} for {}.", path, kind);
            return Err(4);
        }
        if m.is_dir() {
            eprintln!("Error: Destination {:?} is an existing directory, not a file.", path);
            return Err(4);
        }
        if !overwrite {
            eprintln!("Error: Destination file {:?} already exists and --overwrite was not specified.", path);
            return Err(4);
        }
    }

    Ok(())
}

fn check_future_target_alias(path_a: &Path, path_b: &Path, name_a: &str, name_b: &str) -> Result<(), i32> {
    let parent_a = match path_a.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent_b = match path_b.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let meta_a = match fs::metadata(parent_a) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Error: Parent directory for {} {:?} does not exist or cannot be accessed: {}",
                name_a, path_a, e
            );
            return Err(4);
        }
    };
    if !meta_a.is_dir() {
        eprintln!("Error: Parent path {:?} for {} is not a directory.", parent_a, name_a);
        return Err(4);
    }

    let meta_b = match fs::metadata(parent_b) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Error: Parent directory for {} {:?} does not exist or cannot be accessed: {}",
                name_b, path_b, e
            );
            return Err(4);
        }
    };
    if !meta_b.is_dir() {
        eprintln!("Error: Parent path {:?} for {} is not a directory.", parent_b, name_b);
        return Err(4);
    }

    let fname_a = path_a.file_name().unwrap_or_default();
    let fname_b = path_b.file_name().unwrap_or_default();

    if meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino() && fname_a == fname_b {
        eprintln!(
            "Error: Target alias collision between {} {:?} and {} {:?} (same parent directory and filename).",
            name_a, path_a, name_b, path_b
        );
        return Err(2);
    }

    Ok(())
}

/// Fill buffer with cryptographically secure random bytes from the OS.
/// Primary mechanism on Linux: libc::getrandom syscall (reads directly from the kernel CSPRNG / ChaCha20 DRNG).
/// Standard fallback: /dev/urandom standard interface (kernel CSPRNG character device).
/// Unpredictability rationale: Both mechanisms are backed directly by the Linux kernel's
/// cryptographically secure pseudorandom number generator (CSPRNG), whose internal state is
/// forward-secret, backtrack-resistant, and seeded from hardware/system entropy pools.
pub fn os_random_bytes(buf: &mut [u8]) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let mut filled = 0;
        while filled < buf.len() {
            let ret = unsafe {
                libc::getrandom(
                    buf[filled..].as_mut_ptr() as *mut libc::c_void,
                    buf.len() - filled,
                    0,
                )
            };
            if ret > 0 {
                filled += ret as usize;
            } else if ret == 0 {
                break;
            } else {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                // Fall back to /dev/urandom on ENOSYS or other errors
                break;
            }
        }
        if filled == buf.len() {
            return Ok(());
        }
    }

    use std::io::Read;
    let mut file = fs::File::open("/dev/urandom")?;
    file.read_exact(buf)?;
    Ok(())
}

/// Generate an unpredictable temporary filename component using 16 bytes (128 bits) of OS randomness.
/// Unpredictability rationale:
/// 16 bytes (128 bits) of OS CSPRNG entropy yields 2^128 possible values. The probability of an
/// adversary predicting the filename prior to exclusive creation is <= 2^-128 (negligible).
pub fn generate_unpredictable_temp_name(kind: &str) -> Result<String, io::Error> {
    let mut rand_bytes = [0u8; 16];
    os_random_bytes(&mut rand_bytes)?;
    let rand_hex: String = rand_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let pid = process::id();
    Ok(format!(".wordf_tmp_{}_{}_{}.tmp", kind, pid, rand_hex))
}

pub struct TempFileGuard {
    pub path: PathBuf,
    pub file: fs::File,
    pub committed: bool,
}

impl TempFileGuard {
    pub fn cleanup(&mut self) {
        if !self.committed && self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub fn create_exclusive_temp_file(target_path: &Path, kind: &str) -> Result<TempFileGuard, i32> {
    let parent = match target_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    for _attempt in 0..100 {
        let temp_name = match generate_unpredictable_temp_name(kind) {
            Ok(name) => name,
            Err(e) => {
                eprintln!("Error acquiring OS random bytes for temporary file: {}", e);
                return Err(4);
            }
        };
        let temp_path = parent.join(temp_name);

        let os_path = match CString::new(temp_path.as_os_str().as_bytes()) {
            Ok(cs) => cs,
            Err(_) => return Err(4),
        };

        // O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC with 0600 mode
        let fd = unsafe {
            libc::open(
                os_path.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_CLOEXEC,
                0o600,
            )
        };

        if fd >= 0 {
            let file = unsafe { fs::File::from_raw_fd(fd) };
            return Ok(TempFileGuard {
                path: temp_path,
                file,
                committed: false,
            });
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EEXIST) {
            eprintln!(
                "Error: Failed to create exclusive temporary file {:?}: {}",
                temp_path, err
            );
            return Err(4);
        }
    }

    eprintln!(
        "Error: Failed to create exclusive temporary file in parent directory {:?} after 100 attempts",
        parent
    );
    Err(4)
}

/// Atomically rename `old_path` to `new_path`.
/// If `overwrite` is false, use RENAME_NOREPLACE to prevent clobbering existing files.
pub fn atomic_commit_rename(old_path: &Path, new_path: &Path, overwrite: bool) -> Result<(), io::Error> {
    let old_c = CString::new(old_path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let new_c = CString::new(new_path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    if overwrite {
        // Standard atomic overwrite rename
        let ret = unsafe { libc::rename(old_c.as_ptr(), new_c.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    } else {
        // Race-safe no-clobber rename via renameat2
        let ret = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                old_c.as_ptr(),
                libc::AT_FDCWD,
                new_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// Execute two-phase commit:
/// Step 1: Commit PDF
/// Step 2: Commit Report (if requested).
/// If Step 2 fails: PDF is PRESERVED, returns exit code 9.
pub fn commit_two_phase(
    pdf_temp: &mut TempFileGuard,
    pdf_target: &Path,
    mut report_temp: Option<&mut TempFileGuard>,
    report_target: Option<&Path>,
    overwrite: bool,
) -> Result<(), i32> {
    // Step 1: Commit PDF
    if let Err(e) = atomic_commit_rename(&pdf_temp.path, pdf_target, overwrite) {
        eprintln!("Error committing PDF output to {:?}: {}", pdf_target, e);
        pdf_temp.cleanup();
        if let Some(ref mut r_guard) = report_temp {
            r_guard.cleanup();
        }
        return Err(4);
    }
    pdf_temp.committed = true;

    // Step 2: Commit JSON Report (if requested)
    if let (Some(r_guard), Some(r_target)) = (report_temp.as_mut(), report_target) {
        // Check internal test failure injection switch
        let inject_failure = std::env::var("_WORDF_TEST_FAIL_REPORT_COMMIT").as_deref() == Ok("1");

        let commit_res = if inject_failure {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Simulated report commit failure via _WORDF_TEST_FAIL_REPORT_COMMIT",
            ))
        } else {
            atomic_commit_rename(&r_guard.path, r_target, overwrite)
        };

        if let Err(e) = commit_res {
            eprintln!(
                "output.report_commit_failed: Failed to commit JSON report to {:?}: {}. PDF was successfully saved to {:?}.",
                r_target, e, pdf_target
            );
            // Explicitly clean up failed report temporary file; PDF is preserved
            r_guard.cleanup();
            return Err(9);
        }
        r_guard.committed = true;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;

    #[test]
    fn test_os_random_bytes_and_unpredictable_name() {
        // 1. Verify OS random bytes generates non-zero random output
        let mut buf1 = [0u8; 16];
        let mut buf2 = [0u8; 16];
        os_random_bytes(&mut buf1).expect("os_random_bytes must succeed");
        os_random_bytes(&mut buf2).expect("os_random_bytes must succeed");
        assert_ne!(buf1, [0u8; 16], "OS random bytes must not be all zeros");
        assert_ne!(buf1, buf2, "Consecutive OS random byte reads must be independent");

        // 2. Verify unpredictable temp name format and uniqueness
        let name1 = generate_unpredictable_temp_name("pdf").expect("Must generate name");
        let name2 = generate_unpredictable_temp_name("pdf").expect("Must generate name");
        assert!(name1.starts_with(".wordf_tmp_pdf_"));
        assert!(name1.ends_with(".tmp"));
        assert_ne!(name1, name2);
    }

    #[test]
    fn test_create_exclusive_temp_file_identity_and_permissions() {
        let temp_dir = std::env::temp_dir().join(format!("wordf_test_tx_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let target = temp_dir.join("target.pdf");
        let mut guard = create_exclusive_temp_file(&target, "pdf")
            .expect("Exclusive temp file creation must succeed");

        // 1. Verify target parent directory is respected
        assert_eq!(guard.path.parent().unwrap(), temp_dir.as_path());

        // 2. Verify 0600 mode permissions
        let meta = fs::metadata(&guard.path).expect("File metadata must exist");
        assert_eq!(meta.mode() & 0o777, 0o600, "Temporary file must have 0600 permissions");

        // 3. Verify handle preservation: fd inode matches path inode (same file identity)
        let fd = guard.file.as_raw_fd();
        let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::fstat(fd, &mut stat_buf) };
        assert_eq!(ret, 0, "fstat on retained handle must succeed");
        assert_eq!(meta.ino(), stat_buf.st_ino as u64, "Retained file handle must match on-disk inode");
        assert_eq!(meta.dev(), stat_buf.st_dev as u64, "Retained file handle must match on-disk device");

        // 4. Verify direct write via retained handle updates the exact file
        let payload = b"Direct write test via retained handle without reopening";
        guard.file.write_all(payload).expect("Write to retained handle must succeed");
        guard.file.flush().unwrap();
        guard.file.sync_all().unwrap();

        let disk_bytes = fs::read(&guard.path).expect("Read from path must succeed");
        assert_eq!(disk_bytes, payload, "File content written via handle must match on-disk bytes");

        // 5. Verify cleanup on drop without commit
        let temp_path = guard.path.clone();
        assert!(temp_path.exists());
        drop(guard);
        assert!(!temp_path.exists(), "Uncommitted temp file must be cleaned up on drop");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_atomic_commit_no_clobber_and_failure_cleanup() {
        let temp_dir = std::env::temp_dir().join(format!("wordf_test_tx_noclobber_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let target = temp_dir.join("existing_target.pdf");
        let existing_data = b"Pre-existing original content";
        fs::write(&target, existing_data).unwrap();

        let mut guard = create_exclusive_temp_file(&target, "pdf").unwrap();
        guard.file.write_all(b"New attempted content").unwrap();
        guard.file.flush().unwrap();
        guard.file.sync_all().unwrap();

        let temp_path = guard.path.clone();

        // Attempt commit with overwrite = false -> must fail (no-clobber)
        let commit_res = commit_two_phase(&mut guard, &target, None, None, false);
        assert_eq!(commit_res, Err(4), "Commit without overwrite against existing file must fail");

        // Target must remain untouched
        assert_eq!(fs::read(&target).unwrap(), existing_data, "Existing target content must be untouched");

        // Temp file must be cleaned up without leftovers
        assert!(!temp_path.exists(), "Failed commit must clean up temp file without leftovers");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
