//! argv-echo built for the Windows GUI subsystem, as `pythonw` is, for the GUI-shim tests.
#![windows_subsystem = "windows"]

#[path = "../echo.rs"]
mod echo;

fn main() {
    echo::main()
}
