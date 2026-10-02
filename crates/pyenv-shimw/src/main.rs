//! The GUI-subsystem shim (spec §3): rehash links GUI programs such as `pythonw` to this,
//! so starting one opens no console window.
#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(rpyenv_core::shim::main_gui());
}
