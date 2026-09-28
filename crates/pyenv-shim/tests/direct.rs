/// Run under its own name, the shim binary is not a command.
#[test]
fn running_the_shim_binary_directly_is_an_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv-shim"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("run this through a shim"));
}
