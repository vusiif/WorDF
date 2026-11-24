//! Atomic commit and platform-specific file identity / rename logic.
//! License: GPL-3.0-or-later

use std::fs;
use std::io;
use std::path::Path;

use super::temp_file::TempFileGuard;

#[cfg(windows)]
pub fn get_file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    #[allow(non_snake_case)]
    #[repr(C)]
    struct BY_HANDLE_FILE_INFORMATION {
        dwFileAttributes: u32,
        ftCreationTime: [u32; 2],
        ftLastAccessTime: [u32; 2],
        ftLastWriteTime: [u32; 2],
        dwVolumeSerialNumber: u32,
        nFileSizeHigh: u32,
        nFileSizeLow: u32,
        nNumberOfLinks: u32,
        nFileIndexHigh: u32,
        nFileIndexLow: u32,
    }
    extern "system" {
        fn GetFileInformationByHandle(
            hFile: *mut std::ffi::c_void,
            lpFileInformation: *mut BY_HANDLE_FILE_INFORMATION,
        ) -> i32;
    }

    let file = fs::File::open(path).ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ret = unsafe { GetFileInformationByHandle(file.as_raw_handle() as *mut _, &mut info) };
    if ret != 0 {
        let index = ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64);
        Some((info.dwVolumeSerialNumber, index))
    } else {
        None
    }
}

/// Check if two paths physically point to the same file entity (hard links, same inode/file_index).
pub fn is_same_file(path_a: &Path, path_b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (fs::metadata(path_a), fs::metadata(path_b)) {
            ma.dev() == mb.dev() && ma.ino() == mb.ino()
        } else {
            false
        }
    }
    #[cfg(windows)]
    {
        if let (Some(id_a), Some(id_b)) = (get_file_identity(path_a), get_file_identity(path_b)) {
            id_a == id_b
        } else if let (Ok(ca), Ok(cb)) = (fs::canonicalize(path_a), fs::canonicalize(path_b)) {
            ca == cb
        } else {
            false
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

pub fn is_same_dir(p_a: &Path, p_b: &Path, meta_a: &fs::Metadata, meta_b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (p_a, p_b);
        meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino()
    }
    #[cfg(windows)]
    {
        let _ = (meta_a, meta_b);
        match (fs::canonicalize(p_a), fs::canonicalize(p_b)) {
            (Ok(ca), Ok(cb)) => ca == cb,
            _ => p_a == p_b,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (meta_a, meta_b);
        p_a == p_b
    }
}

/// Atomically rename `old_path` to `new_path`.
/// If `overwrite` is false, use RENAME_NOREPLACE (or Windows MoveFileExW without MOVEFILE_REPLACE_EXISTING)
/// to prevent clobbering existing files.
pub fn atomic_commit_rename(old_path: &Path, new_path: &Path, overwrite: bool) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let old_c = CString::new(old_path.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let new_c = CString::new(new_path.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        if overwrite {
            let ret = unsafe { libc::rename(old_c.as_ptr(), new_c.as_ptr()) };
            if ret == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        } else {
            #[cfg(target_os = "linux")]
            let ret = unsafe {
                libc::renameat2(
                    libc::AT_FDCWD,
                    old_c.as_ptr(),
                    libc::AT_FDCWD,
                    new_c.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };

            #[cfg(target_os = "macos")]
            let ret = unsafe {
                libc::renamex_np(
                    old_c.as_ptr(),
                    new_c.as_ptr(),
                    libc::RENAME_EXCL,
                )
            };

            #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
            let ret = {
                if new_path.exists() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "destination already exists",
                    ));
                }
                unsafe { libc::rename(old_c.as_ptr(), new_c.as_ptr()) }
            };

            if ret == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let old_w: Vec<u16> = old_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let new_w: Vec<u16> = new_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

        extern "system" {
            fn MoveFileExW(
                lpExistingFileName: *const u16,
                lpNewFileName: *const u16,
                dwFlags: u32,
            ) -> i32;
        }

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x00000001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x00000008;

        let flags = if overwrite {
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
        } else {
            MOVEFILE_WRITE_THROUGH
        };

        let mut last_err = None;
        for attempt in 0..3 {
            let ret = unsafe { MoveFileExW(old_w.as_ptr(), new_w.as_ptr(), flags) };
            if ret != 0 {
                return Ok(());
            }
            let err = io::Error::last_os_error();
            let raw_code = err.raw_os_error().unwrap_or(0);
            // Retry on ERROR_SHARING_VIOLATION (32) or transient ERROR_ACCESS_DENIED (5)
            if raw_code == 32 || (overwrite && raw_code == 5) {
                std::thread::sleep(std::time::Duration::from_millis(2 * (attempt + 1)));
                last_err = Some(err);
                continue;
            } else {
                return Err(err);
            }
        }
        Err(last_err.unwrap_or_else(io::Error::last_os_error))
    }

    #[cfg(not(any(unix, windows)))]
    {
        if !overwrite && new_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination file already exists",
            ));
        }
        fs::rename(old_path, new_path)
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
            r_guard.cleanup();
            return Err(9);
        }
        r_guard.committed = true;
    }

    Ok(())
}
