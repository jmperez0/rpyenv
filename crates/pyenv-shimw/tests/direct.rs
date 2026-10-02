/// Run under its own name, the GUI shim is not a command. It has a stderr to write to
/// here, so it says so there rather than in a message box.
#[test]
fn running_the_gui_shim_directly_is_an_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv-shimw"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("run this through a shim"));
}
