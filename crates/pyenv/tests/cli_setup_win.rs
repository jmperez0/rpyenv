//! `pyenv setup` (rpyenv-only, spec §9.4; plan M6a, Task 3). The fixture's user and
//! machine environments are `HKCU\<f.test_key>\user` and `\machine` (plan R1); tests seed
//! and read them with `reg`.
#![cfg(windows)]

mod common;
use common::Fixture;

fn reg_set(f: &Fixture, scope: &str, name: &str, kind: &str, value: &str) {
    let out = std::process::Command::new("reg")
        .args([
            "add",
            &format!("HKCU\\{}\\{scope}", f.test_key),
            "/v",
            name,
            "/t",
            kind,
            "/d",
            value,
            "/f",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
}

/// `<type> <data>` as stored, e.g. `REG_EXPAND_SZ C:\x;C:\y`, or "" when absent. Read with
/// the API: `reg query` prints in the OEM code page, which loses the fixture's `ñ`.
fn reg_get(f: &Fixture, scope: &str, name: &str) -> String {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, REG_EXPAND_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ,
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (sub, name) = (wide(&format!("{}\\{scope}", f.test_key)), wide(name));
    let mut buf = vec![0u16; 32 * 1024];
    let (mut kind, mut bytes) = (0u32, (buf.len() * 2) as u32);
    // SAFETY: reads one value into a local buffer of the size given.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if rc != 0 {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let kind = if kind == REG_EXPAND_SZ {
        "REG_EXPAND_SZ"
    } else {
        "REG_SZ"
    };
    format!("{kind} {}", String::from_utf16_lossy(&buf[..len]))
}

#[test]
fn setup_puts_shims_first_adds_the_profile_line_and_marks_the_root() {
    let f = Fixture::new();
    reg_set(&f, "user", "Path", "REG_EXPAND_SZ", r"C:\Tools");
    let r = f.pyenv(&["setup"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let shims = f.root.join("shims");
    assert!(
        reg_get(&f, "user", "Path").contains(&format!("{};C:\\Tools", shims.display())),
        "{}",
        reg_get(&f, "user", "Path")
    );
    assert!(f.root.join(".rpyenv-setup").is_file());
    let p7 = f
        .base
        .join("Documents")
        .join("PowerShell")
        .join("Microsoft.PowerShell_profile.ps1");
    assert!(std::fs::read_to_string(p7)
        .unwrap()
        .contains("pyenv init - pwsh"));
    assert!(r.stdout.contains("open a new terminal"), "{}", r.stdout);
}

/// Review focus 1 and 2: a REG_EXPAND_SZ value keeps its type, the entry isn't duplicated,
/// and a second run changes nothing.
#[test]
fn setup_keeps_expand_sz_and_does_not_duplicate() {
    let f = Fixture::new();
    let shims = f.root.join("shims");
    let spelled = format!("{}\\", shims.display()).to_uppercase();
    reg_set(
        &f,
        "user",
        "Path",
        "REG_EXPAND_SZ",
        &format!(r"C:\Tools;{spelled}"),
    );
    assert_eq!(f.pyenv(&["setup"]).code, 0);
    let after = reg_get(&f, "user", "Path");
    assert!(after.contains("REG_EXPAND_SZ"), "{after}");
    assert_eq!(
        after
            .to_lowercase()
            .matches(&shims.display().to_string().to_lowercase())
            .count(),
        1,
        "{after}"
    );
    let again = f.pyenv(&["setup"]);
    assert!(again.stdout.contains("already first"), "{}", again.stdout);
    assert_eq!(reg_get(&f, "user", "Path"), after);
}

/// Spec §9.4 "PATH order": a python.exe on the machine PATH is named.
#[test]
fn setup_warns_about_a_python_on_the_machine_path() {
    let f = Fixture::new();
    let sys_py = f.base.join("SysPython");
    std::fs::create_dir_all(&sys_py).unwrap();
    std::fs::write(sys_py.join("python.exe"), b"").unwrap();
    reg_set(
        &f,
        "machine",
        "Path",
        "REG_SZ",
        &sys_py.display().to_string(),
    );
    let r = f.pyenv(&["setup"]);
    assert!(
        r.stderr
            .contains(&sys_py.join("python.exe").display().to_string()),
        "{}",
        r.stderr
    );
}

/// Spec §9.4: pyenv-win's own `bin` in this root gets a hint to migrate.
#[test]
fn setup_hints_at_migrate_when_pyenv_win_is_there() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("bin")).unwrap();
    std::fs::write(f.root.join("bin").join("pyenv.ps1"), b"").unwrap();
    let r = f.pyenv(&["setup"]);
    assert!(r.stderr.contains("pyenv migrate"), "{}", r.stderr);
}

/// Spec §9.4: an all-users install (pyenv.exe under Program Files) sets each user up on
/// their first `pyenv` command; a per-user install doesn't.
#[test]
fn an_all_users_install_sets_the_user_up_on_first_use() {
    let f = Fixture::new();
    let pf_bin = f.base.join("Program Files").join("rpyenv").join("bin");
    std::fs::create_dir_all(&pf_bin).unwrap();
    for exe in ["pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"] {
        let src = std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")).with_file_name(exe);
        std::fs::copy(&src, pf_bin.join(exe)).unwrap();
    }
    let out = f
        .command(&pf_bin.join("pyenv.exe"), &f.work, &[])
        .arg("root")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("set up for this user"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(f.root.join(".rpyenv-setup").is_file());
    let again = f
        .command(&pf_bin.join("pyenv.exe"), &f.work, &[])
        .arg("root")
        .output()
        .unwrap();
    assert!(
        again.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
}

#[test]
fn a_per_user_install_doesnt_set_up_on_its_own() {
    let f = Fixture::new();
    assert_eq!(f.pyenv(&["root"]).code, 0);
    assert!(!f.root.join(".rpyenv-setup").exists());
}

fn reg_type(f: &Fixture, scope: &str, name: &str) -> String {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            &format!("HKCU\\{}\\{scope}", f.test_key),
            "/v",
            name,
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Final review I5 and M1: a user Path setup can't read is left as it is (not replaced by
/// the shims alone), setup fails, and it doesn't mark the root as set up.
#[test]
fn setup_leaves_an_unreadable_path_alone_and_fails() {
    let f = Fixture::new();
    reg_set(&f, "user", "Path", "REG_DWORD", "1");
    let r = f.pyenv(&["setup"]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("PATH"), "{}", r.stderr);
    assert!(
        reg_type(&f, "user", "Path").contains("REG_DWORD"),
        "{}",
        reg_type(&f, "user", "Path")
    );
    assert!(!f.root.join(".rpyenv-setup").exists());
}

fn all_users_bin(f: &Fixture) -> std::path::PathBuf {
    let pf_bin = f.base.join("Program Files").join("rpyenv").join("bin");
    std::fs::create_dir_all(&pf_bin).unwrap();
    for exe in ["pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"] {
        let src = std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")).with_file_name(exe);
        std::fs::copy(&src, pf_bin.join(exe)).unwrap();
    }
    pf_bin
}

/// Final review M5: Program Files matches in any case, as Windows paths do.
#[test]
fn first_run_matches_program_files_in_any_case() {
    let f = Fixture::new();
    let pf_bin = all_users_bin(&f);
    let upper = f
        .base
        .join("Program Files")
        .display()
        .to_string()
        .to_uppercase();
    let out = f
        .command(
            &pf_bin.join("pyenv.exe"),
            &f.work,
            &[("RPYENV_TEST_PROGRAM_FILES", upper.as_str())],
        )
        .arg("root")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("set up for this user"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Final review M5: the first-run line prints before `pyenv exec` runs its command.
#[test]
fn first_run_prints_before_exec_runs_the_command() {
    let f = Fixture::new();
    let pf_bin = all_users_bin(&f);
    f.version("3.12.1");
    let cmd = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe");
    std::fs::copy(cmd, f.root.join("versions").join("3.12.1").join("tool.exe")).unwrap();
    let out = f
        .command(
            &pf_bin.join("pyenv.exe"),
            &f.work,
            &[("PYENV_VERSION", "3.12.1")],
        )
        .args(["exec", "tool", "/d", "/c", "echo from-the-child 1>&2"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    let (setup, child) = (err.find("set up for this user"), err.find("from-the-child"));
    assert!(setup.is_some() && child.is_some() && setup < child, "{err}");
}

/// Re-review 6: a first run that fails says so once, not on every command (it retries a
/// day later; `pyenv setup` retries at once).
#[test]
fn a_failed_first_run_is_not_repeated_on_every_command() {
    let f = Fixture::new();
    let pf_bin = all_users_bin(&f);
    reg_set(&f, "user", "Path", "REG_DWORD", "1");
    let run = || {
        let out = f
            .command(&pf_bin.join("pyenv.exe"), &f.work, &[])
            .arg("root")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    assert!(run().contains("set up for this user"));
    let second = run();
    assert!(second.is_empty(), "{second}");
}
