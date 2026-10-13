//! The execution-policy reader against Windows PowerShell itself, on the real registry
//! (read only). Its own test binary: the unit tests set `RPYENV_TEST_ENV_KEY` process-wide.
#![cfg(windows)]

#[test]
fn the_reader_agrees_with_windows_powershell() {
    assert!(std::env::var_os("RPYENV_TEST_ENV_KEY").is_none());
    let ps = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let out = std::process::Command::new(ps)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-ExecutionPolicy",
        ])
        .env_remove("PSExecutionPolicyPreference")
        // PowerShell 7's module path, inherited, makes 5.1 load 7's Security module.
        .env_remove("PSModulePath")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let want = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(rpyenv_core::winenv::windows_powershell_policy().name, want);
}
