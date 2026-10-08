//! `pyenv migrate` / `--restore` (rpyenv-only, spec §9.4; plan M6a, Task 5). The fixture's
//! user environment is `HKCU\<f.test_key>\user` (plan R1).
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

/// `<type> <data>` as stored, or "" when absent. Read with the API: `reg query` prints in
/// the OEM code page, which loses the fixture's `ñ`.
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

/// A pyenv-win install in the fixture's root: launchers in `bin`, `bin` and `shims` on the
/// user PATH, one version.
fn pyenv_win(f: &Fixture) {
    let bin = f.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for n in ["pyenv.ps1", "pyenv.bat", "pyenv"] {
        std::fs::write(bin.join(n), format!("{n} from pyenv-win")).unwrap();
    }
    f.version("3.12.1");
    let path = format!(
        "{};{};C:\\Tools",
        bin.display(),
        f.root.join("shims").display()
    );
    reg_set(f, "user", "Path", "REG_SZ", &path);
}

fn venv_env(env: &std::path::Path, home: &std::path::Path) {
    std::fs::create_dir_all(env.join("Scripts")).unwrap();
    std::fs::write(
        env.join("pyvenv.cfg"),
        format!("home = {}\r\n", home.display()),
    )
    .unwrap();
}

#[test]
fn migrate_takes_over_and_restore_gives_back() {
    let f = Fixture::new();
    pyenv_win(&f);
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(!f.root.join("bin").join("pyenv.ps1").exists());
    assert!(f
        .root
        .join(".rpyenv-migrate")
        .join("bin")
        .join("pyenv.ps1")
        .is_file());
    let path = reg_get(&f, "user", "Path");
    assert!(
        !path
            .to_lowercase()
            .contains(&f.root.join("bin").display().to_string().to_lowercase()),
        "{path}"
    );
    assert!(
        path.contains(&f.root.join("shims").display().to_string()),
        "{path}"
    );
    let p5 = f
        .base
        .join("Documents")
        .join("WindowsPowerShell")
        .join("Microsoft.PowerShell_profile.ps1");
    assert!(std::fs::read_to_string(&p5)
        .unwrap()
        .contains("pyenv init - pwsh"));

    // Review focus 2: a second run changes nothing.
    let again = f.pyenv(&["migrate"]);
    assert!(
        again.stdout.contains("already migrated"),
        "{}",
        again.stdout
    );

    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}{}", back.stdout, back.stderr);
    assert_eq!(
        std::fs::read_to_string(f.root.join("bin").join("pyenv.ps1")).unwrap(),
        "pyenv.ps1 from pyenv-win"
    );
    assert!(reg_get(&f, "user", "Path").contains(&f.root.join("bin").display().to_string()));
    assert!(!std::fs::read_to_string(&p5)
        .unwrap()
        .contains("pyenv init - pwsh"));
    assert!(!f.root.join(".rpyenv-migrate").exists());
}

#[test]
fn migrate_without_pyenv_win_says_so() {
    let f = Fixture::new();
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("no pyenv-win"), "{}", r.stderr);
}

/// pyenv-win-venv envs are linked in (user decision 2026-10-08), with Review focus 4: a
/// name taken by a version, or an env whose base isn't installed, is skipped.
#[test]
fn migrate_links_pyenv_win_venv_envs() {
    let f = Fixture::new();
    pyenv_win(&f);
    let envs = f.base.join(".pyenv-win-venv").join("envs");
    let base = f.root.join("versions").join("3.12.1");
    for (name, home) in [
        ("work", base.clone()),
        ("3.12.1", base.clone()),
        ("orphan", f.root.join("versions").join("3.9.9")),
    ] {
        venv_env(&envs.join(name), &home);
    }
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(f
        .root
        .join("versions")
        .join("work")
        .join("pyvenv.cfg")
        .is_file());
    assert!(base.join("envs").join("work").join("pyvenv.cfg").is_file());
    assert!(
        r.stderr.contains("3.12.1") && r.stderr.contains("orphan"),
        "{}",
        r.stderr
    );
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}", back.stderr);
    assert!(!f.root.join("versions").join("work").exists());
    assert!(
        envs.join("work").join("pyvenv.cfg").is_file(),
        "the env's files stay"
    );
    // Final review M2: the `envs` folder migrate created goes too.
    assert!(!base.join("envs").exists());
}

/// Review focus 3: `--restore` touches only what migrate did.
#[test]
fn restore_touches_only_what_migrate_did() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    std::fs::write(f.root.join("bin").join("pyenv.ps1"), "a newer pyenv.ps1").unwrap();
    let p7 = f
        .base
        .join("Documents")
        .join("PowerShell")
        .join("Microsoft.PowerShell_profile.ps1");
    let mut text = std::fs::read_to_string(&p7).unwrap();
    text.push_str("Set-Alias ll ls\r\n");
    std::fs::write(&p7, &text).unwrap();
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(
        std::fs::read_to_string(f.root.join("bin").join("pyenv.ps1")).unwrap(),
        "a newer pyenv.ps1"
    );
    assert!(back.stderr.contains("pyenv.ps1"), "{}", back.stderr);
    assert_eq!(std::fs::read_to_string(&p7).unwrap(), "Set-Alias ll ls\r\n");
}

/// Review focus 5: uninstalling the base removes the junction, not the env's files.
#[test]
fn uninstalling_the_base_keeps_a_linked_venv_env() {
    let f = Fixture::new();
    pyenv_win(&f);
    let env = f.base.join(".pyenv-win-venv").join("envs").join("work");
    venv_env(&env, &f.root.join("versions").join("3.12.1"));
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let r = f.pyenv(&["uninstall", "-f", "3.12.1"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(env.join("pyvenv.cfg").is_file(), "the env's files stay");
    // Final review M7: the top-level link goes with the base.
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("work")).is_err());
}

/// Global Constraints (side-agent note): `virtualenv-delete` of a linked pyenv-win-venv env
/// removes the links, never the env's files.
#[test]
fn virtualenv_delete_keeps_a_linked_venv_envs_files() {
    let f = Fixture::new();
    pyenv_win(&f);
    let env = f.base.join(".pyenv-win-venv").join("envs").join("work");
    venv_env(&env, &f.root.join("versions").join("3.12.1"));
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let r = f.pyenv(&["virtualenv-delete", "-f", "work"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(
        !f.root.join("versions").join("work").exists(),
        "the link is gone"
    );
    assert!(env.join("pyvenv.cfg").is_file(), "the env's files stay");
    // --restore copes with links that are already gone.
    assert_eq!(f.pyenv(&["migrate", "--restore"]).code, 0);
}

/// Final review I5: a user Path migrate can't read stops it before anything moves.
#[test]
fn migrate_stops_on_an_unreadable_path() {
    let f = Fixture::new();
    pyenv_win(&f);
    reg_set(&f, "user", "Path", "REG_DWORD", "1");
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(f.root.join("bin").join("pyenv.ps1").is_file());
    assert!(!f.root.join(".rpyenv-migrate").exists());
}

fn profiles(f: &Fixture) -> [std::path::PathBuf; 2] {
    let docs = f.base.join("Documents");
    [
        docs.join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        docs.join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
    ]
}

/// Final review I2: after `pyenv setup` (which hints at migrate, and which an all-users
/// first run does before anything), `--restore` still takes the line out: under pyenv-win
/// it would fail in every new PowerShell.
#[test]
fn restore_removes_the_line_setup_added_too() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["setup"]).code, 0);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}{}", back.stdout, back.stderr);
    for p in profiles(&f) {
        assert!(
            !std::fs::read_to_string(&p).unwrap().contains("pyenv init"),
            "{}",
            p.display()
        );
    }
}

/// Final review I3: a restore step that fails is reported, fails the command, and stays in
/// the manifest, so running `--restore` again finishes the job.
#[test]
fn a_failed_restore_keeps_what_is_left_to_retry() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let [_, p7] = profiles(&f);
    let mut perms = std::fs::metadata(&p7).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&p7, perms.clone()).unwrap();
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 1, "{}{}", back.stdout, back.stderr);
    assert!(
        back.stderr.contains(&p7.display().to_string()),
        "{}",
        back.stderr
    );
    assert!(f
        .root
        .join(".rpyenv-migrate")
        .join("manifest.txt")
        .is_file());
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&p7, perms).unwrap();
    let again = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
    assert!(!std::fs::read_to_string(&p7).unwrap().contains("pyenv init"));
    assert!(!f.root.join(".rpyenv-migrate").exists());
}

/// Final review I4: a launcher kept because it exists again is final, not a reason to
/// keep the whole manifest: the restore finishes, a second one has nothing to do, and the
/// old launcher stays in the backup.
#[test]
fn a_kept_launcher_doesnt_hold_the_restore_open() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    std::fs::write(f.root.join("bin").join("pyenv.ps1"), "a newer pyenv.ps1").unwrap();
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}{}", back.stdout, back.stderr);
    assert!(back.stderr.contains("kept"), "{}", back.stderr);
    let again = f.pyenv(&["migrate", "--restore"]);
    assert!(
        again.stderr.contains("nothing to restore"),
        "{}",
        again.stderr
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join(".rpyenv-migrate").join("bin").join("pyenv.ps1"))
            .unwrap(),
        "pyenv.ps1 from pyenv-win"
    );
}

/// Final review M2: a dangling `versions\<name>` link counts as taken (nothing half-linked),
/// a `home` that isn't a version folder is refused, and an env with no `home` is named.
#[test]
fn odd_venv_envs_are_named_and_not_linked() {
    let f = Fixture::new();
    pyenv_win(&f);
    let envs = f.base.join(".pyenv-win-venv").join("envs");
    let base = f.root.join("versions").join("3.12.1");
    venv_env(&envs.join("work"), &base);
    venv_env(&envs.join("odd"), &f.root.join("versions").join(".."));
    let nohome = envs.join("nohome");
    std::fs::create_dir_all(&nohome).unwrap();
    std::fs::write(nohome.join("pyvenv.cfg"), "version = 3.12.1\r\n").unwrap();
    let gone = f.base.join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    rpyenv_core::junction::create(&f.root.join("versions").join("work"), &gone).unwrap();
    std::fs::remove_dir(&gone).unwrap();
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    for name in ["work", "odd", "nohome"] {
        assert!(
            r.stderr.contains(&envs.join(name).display().to_string()),
            "{name}: {}",
            r.stderr
        );
    }
    assert!(std::fs::symlink_metadata(base.join("envs").join("work")).is_err());
    assert!(!f.root.join("envs").exists());
}

/// Final review M4: a link rpyenv made later under the same name (the base reinstalled and
/// the env re-created) isn't migrate's to remove.
#[test]
fn restore_keeps_a_link_made_after_migrate() {
    let f = Fixture::new();
    pyenv_win(&f);
    let env = f.base.join(".pyenv-win-venv").join("envs").join("work");
    let base = f.root.join("versions").join("3.12.1");
    venv_env(&env, &base);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let (top, in_base) = (
        f.root.join("versions").join("work"),
        base.join("envs").join("work"),
    );
    std::fs::remove_dir(&top).unwrap();
    std::fs::remove_dir(&in_base).unwrap();
    venv_env(&in_base, &base);
    rpyenv_core::junction::create(&top, &in_base).unwrap();
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}{}", back.stdout, back.stderr);
    assert!(top.join("pyvenv.cfg").is_file(), "{}", back.stderr);
}

/// Final review M3: restore removes only empty folders: a file in the backup that the
/// manifest doesn't list stays.
#[test]
fn restore_never_deletes_an_unlisted_backup_file() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let extra = f.root.join(".rpyenv-migrate").join("bin").join("extra.ps1");
    std::fs::write(&extra, "x").unwrap();
    assert_eq!(f.pyenv(&["migrate", "--restore"]).code, 0);
    assert!(extra.is_file());
}
