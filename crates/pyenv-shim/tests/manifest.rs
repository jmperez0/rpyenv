//! The console shim carries `consoleAllocationPolicy=detached` (spec §5.3, M5a); the GUI
//! shim doesn't need it and doesn't get it.
#![cfg(windows)]

use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
    LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
};

/// The text of `exe`'s embedded manifest (resource type 24, ID 1), or empty.
fn manifest(exe: &str) -> String {
    let wide: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
    // SAFETY: maps the file as data only and reads one resource's bytes, whose size
    // SizeofResource gives; the module is never freed, which only leaks in a test.
    unsafe {
        let module = LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        );
        assert!(!module.is_null(), "cannot map {exe}");
        let res = FindResourceW(module, 1 as _, 24 as _);
        if res.is_null() {
            return String::new();
        }
        let size = SizeofResource(module, res) as usize;
        let data = LockResource(LoadResource(module, res)) as *const u8;
        String::from_utf8_lossy(std::slice::from_raw_parts(data, size)).into_owned()
    }
}

#[test]
fn the_console_shim_asks_for_no_console_at_startup() {
    let text = manifest(env!("CARGO_BIN_EXE_pyenv-shim"));
    assert!(text.contains("consoleAllocationPolicy"), "{text}");
    assert!(text.contains(">detached<"), "{text}");
}
