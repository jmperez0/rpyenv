//! Directory junctions for virtualenv links on Windows (spec §10, plan M4b Decision 4).
//! A junction needs no privilege; its target must be absolute. Remove one with
//! `std::fs::remove_dir`, never `remove_file` (Access is denied) or `remove_dir_all`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().collect()
}

/// Creates the folder `link` as a junction to the absolute folder `target`.
pub fn create(link: &Path, target: &Path) -> std::io::Result<()> {
    let invalid = |why: &str| {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            why.to_string(),
        ))
    };
    if !target.is_absolute() {
        return invalid("junction target must be absolute");
    }
    // A drive path only: a verbatim (`\\?\`) or UNC target would make a malformed `\??\` name.
    if target.as_os_str().to_string_lossy().starts_with(r"\\") {
        return invalid("junction target must be a drive path");
    }
    // The whole buffer (8-byte header, 8 bytes of offsets, both NUL-terminated names) must
    // fit the kernel's 16 KiB MAXIMUM_REPARSE_DATA_BUFFER_SIZE, and so the u16 lengths too.
    if 8 + 8 + (wide(target.as_os_str()).len() * 2 + 6) * 2 > 16 * 1024 {
        return invalid("junction target is too long");
    }
    std::fs::create_dir(link)?;
    let result = (|| {
        let substitute: Vec<u16> = wide(std::ffi::OsStr::new("\\??\\"))
            .into_iter()
            .chain(wide(target.as_os_str()))
            .collect();
        let print = wide(target.as_os_str());
        // REPARSE_DATA_BUFFER, MountPointReparseBuffer: tag, data length, reserved, then
        // substitute offset/length, print offset/length, then both names with NULs.
        let names_len = (substitute.len() + 1 + print.len() + 1) * 2;
        let data_len = 8 + names_len;
        let mut buf: Vec<u8> = Vec::with_capacity(8 + data_len);
        buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend_from_slice(&(data_len as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        for u in substitute
            .iter()
            .chain([0u16].iter())
            .chain(print.iter())
            .chain([0u16].iter())
        {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        let path: Vec<u16> = wide(link.as_os_str()).into_iter().chain(Some(0)).collect();
        // SAFETY: path is NUL-terminated; the handle is closed below.
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let mut returned = 0u32;
        // SAFETY: buf outlives the call; h is a valid handle.
        let ok = unsafe {
            DeviceIoControl(
                h,
                FSCTL_SET_REPARSE_POINT,
                buf.as_ptr().cast(),
                buf.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        let err = std::io::Error::last_os_error();
        // SAFETY: h came from CreateFileW above.
        unsafe { CloseHandle(h) };
        if ok == 0 {
            Err(err)
        } else {
            Ok(())
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_junction_reads_back_and_removes_without_its_target() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("target");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        let link = t.path().join("link");
        super::create(&link, &target).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert!(link.join("inside").is_dir());
        std::fs::remove_dir(&link).unwrap();
        assert!(target.join("inside").is_dir());
    }

    /// Final review M5: verbatim or UNC targets, and names too long for the reparse buffer, are
    /// refused before anything is made.
    #[test]
    fn odd_targets_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let link = t.path().join("link");
        for target in [r"\\?\C:\x", r"\\server\share\x"] {
            let e = super::create(&link, std::path::Path::new(target)).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{target}");
        }
        let long = format!(r"C:\{}", "a".repeat(40_000));
        assert_eq!(
            super::create(&link, std::path::Path::new(&long))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(!link.exists());
    }

    /// Re-review M5: the kernel's 16 KiB reparse buffer bounds the target, checked up front.
    #[test]
    fn a_target_over_the_kernel_limit_is_refused_up_front() {
        let t = tempfile::tempdir().unwrap();
        let link = t.path().join("link");
        let long = format!(r"C:\{}", "a".repeat(5_000));
        let e = super::create(&link, std::path::Path::new(&long)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!link.exists());
    }
}
