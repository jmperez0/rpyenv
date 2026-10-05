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
    if !target.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "junction target must be absolute",
        ));
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
}
