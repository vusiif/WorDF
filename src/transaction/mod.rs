//! WorDF file system isolation and two-phase output transactions.
//! License: GPL-3.0-or-later

pub mod atomic;
pub mod temp_file;
pub mod validate;

pub use atomic::{atomic_commit_rename, commit_two_phase, is_same_file};
pub use temp_file::{
    create_exclusive_temp_file, generate_unpredictable_temp_name, os_random_bytes, TempFileGuard,
};
pub use validate::{validate_and_isolate_targets, TargetPaths, ValidatedTargets};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process;

    #[test]
    fn test_os_random_bytes_and_unpredictable_name() {
        let mut buf1 = [0u8; 16];
        let mut buf2 = [0u8; 16];
        os_random_bytes(&mut buf1).expect("os_random_bytes must succeed");
        os_random_bytes(&mut buf2).expect("os_random_bytes must succeed");
        assert_ne!(buf1, [0u8; 16], "OS random bytes must not be all zeros");
        assert_ne!(buf1, buf2, "Consecutive OS random byte reads must be independent");

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

        assert_eq!(guard.path.parent().unwrap(), temp_dir.as_path());

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = fs::metadata(&guard.path).expect("File metadata must exist");
            assert_eq!(meta.mode() & 0o777, 0o600, "Temporary file must have 0600 permissions");
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            use std::os::unix::io::AsRawFd;
            let meta = fs::metadata(&guard.path).expect("File metadata must exist");
            let fd = guard.file.as_raw_fd();
            let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
            let ret = unsafe { libc::fstat(fd, &mut stat_buf) };
            assert_eq!(ret, 0, "fstat on retained handle must succeed");
            assert_eq!(meta.ino(), stat_buf.st_ino as u64, "Retained file handle must match on-disk inode");
            assert_eq!(meta.dev(), stat_buf.st_dev as u64, "Retained file handle must match on-disk device");
        }

        let payload = b"Direct write test via retained handle without reopening";
        guard.file.write_all(payload).expect("Write to retained handle must succeed");
        guard.file.flush().unwrap();
        guard.file.sync_all().unwrap();

        let disk_bytes = fs::read(&guard.path).expect("Read from path must succeed");
        assert_eq!(disk_bytes, payload, "File content written via handle must match on-disk bytes");

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

        let commit_res = commit_two_phase(&mut guard, &target, None, None, false);
        assert_eq!(commit_res, Err(4), "Commit without overwrite against existing file must fail");

        assert_eq!(fs::read(&target).unwrap(), existing_data, "Existing target content must be untouched");

        assert!(!temp_path.exists(), "Failed commit must clean up temp file without leftovers");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
