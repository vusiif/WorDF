//! Exclusive unpredictable temporary file creation and lifecycle guard.
//! License: GPL-3.0-or-later

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

/// Fill buffer with cryptographically secure random bytes from the OS.
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
                break;
            }
        }
        if filled == buf.len() {
            return Ok(());
        }
    }

    #[cfg(unix)]
    {
        use std::io::Read;
        let mut file = fs::File::open("/dev/urandom")?;
        file.read_exact(buf)?;
        return Ok(());
    }

    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(
                hAlgorithm: *mut std::ffi::c_void,
                pbBuffer: *mut u8,
                cbBuffer: u32,
                dwFlags: u32,
            ) -> i32;
        }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            return Ok(());
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("BCryptGenRandom failed with NTSTATUS 0x{:08X}", status),
            ));
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported platform for secure OS random bytes",
        ))
    }
}

/// Generate an unpredictable temporary filename component using 16 bytes (128 bits) of OS randomness.
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

        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::io::FromRawFd;

            let os_path = match CString::new(temp_path.as_os_str().as_bytes()) {
                Ok(cs) => cs,
                Err(_) => return Err(4),
            };

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

        #[cfg(windows)]
        {
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create_new(true);
            match opts.open(&temp_path) {
                Ok(file) => {
                    return Ok(TempFileGuard {
                        path: temp_path,
                        file,
                        committed: false,
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(err) => {
                    eprintln!(
                        "Error: Failed to create exclusive temporary file {:?}: {}",
                        temp_path, err
                    );
                    return Err(4);
                }
            }
        }
    }

    eprintln!(
        "Error: Failed to create exclusive temporary file in parent directory {:?} after 100 attempts",
        parent
    );
    Err(4)
}
