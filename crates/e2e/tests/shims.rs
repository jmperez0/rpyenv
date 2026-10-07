mod common;
use common::*;
use std::ffi::{OsStr, OsString};
use std::process::Stdio;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

fn v(s: &str) -> &OsStr {
    OsStr::new(s)
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[cfg(unix)]
#[test]
fn shim_passes_arguments_byte_for_byte() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    let python = f.install("3.12.10/bin/python");
    f.rehash();
    let mut args: Vec<OsString> = [
        "a b",
        "",
        "x^y",
        "100%",
        "%USERNAME%",
        "a&b",
        "say \"hi\"",
        "ñ",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push(OsString::from_vec(b"\xff\xfe".to_vec()));
    let out = f.run_shim(
        "python",
        &args,
        &[("PYENV_VERSION", v("3.12.10")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    let mut expected = vec![line("argv0", &python)];
    expected.extend(args.iter().map(|a| line("arg", a)));
    let text = stdout(&out);
    assert_eq!(
        text.lines().take(expected.len()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(out.status.code(), Some(7));
}

#[cfg(unix)]
#[test]
fn shim_and_exec_export_the_upstream_environment() {
    let f = Fixture::new();
    let python = f.install("3.12.10/bin/python");
    f.rehash();
    let env = [
        ("PYENV_VERSION", v("3.12")),
        ("ARGV_ECHO_ENV", v("PYENV_VERSION,PYENV_ROOT,PATH")),
    ];
    let mut path = python.parent().unwrap().as_os_str().to_os_string();
    path.push(":");
    path.push(std::env::join_paths([f.root.join("shims"), f.syspath.clone()]).unwrap());
    let expected = [
        line("env PYENV_VERSION", "3.12.10"),
        line("env PYENV_ROOT", &f.root),
        line("env PATH", &path),
    ];
    for out in [
        f.run_shim("python", &[], &env),
        f.pyenv(&["exec", "python"], &env),
    ] {
        let text = stdout(&out);
        for e in &expected {
            assert!(text.lines().any(|l| l == e), "missing {e} in:\n{text}");
        }
    }
}

/// Fix I-1: a shim baked with no `PYENV_ROOT` in its own environment still finds the root
/// it lives in, rather than falling back to `HOME`/`.pyenv`.
#[cfg(unix)]
#[test]
fn shim_uses_the_root_it_lives_in() {
    let f = Fixture::new();
    let python = f.install("3.12.10/bin/python");
    f.rehash();
    let out = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("ARGV_ECHO_ENV", v("PYENV_ROOT")),
            ],
        )
        .env_remove("PYENV_ROOT")
        .output()
        .unwrap();
    let text = stdout(&out);
    assert_eq!(text.lines().next(), Some(line("argv0", &python).as_str()));
    assert!(
        text.lines().any(|l| l == line("env PYENV_ROOT", &f.root)),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn not_found_exits_127_listing_the_versions_that_have_it() {
    let f = Fixture::new();
    f.install("3.11.9/bin/tool");
    f.install("3.12.10/bin/python");
    f.rehash();
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.12.10"))]);
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "pyenv: tool: command not found\n\n\
         The `tool' command exists in these Python versions:\n  \
         3.11.9\n\n\
         Note: See 'pyenv help global' for tips on allowing multiple\n      \
         Python versions to be found at the same time.\n"
    );
    assert_eq!(out.status.code(), Some(127));
}

#[cfg(unix)]
#[test]
fn pip_shim_rehashes_before_returning() {
    let f = Fixture::new();
    f.install("3.12.10/bin/pip");
    f.rehash();
    let script = f.root.join("versions/3.12.10/bin/black");
    let out = f.run_shim(
        "pip",
        &["install".into(), "black".into()],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("ARGV_ECHO_TOUCH", script.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(f.shim("black").exists(), "the exit check did not rehash");
}

/// The pip wait path keeps signals the caller ignored, as a shell does, so
/// `nohup pip install …` survives a logout (Task 7 review finding).
#[cfg(target_os = "linux")]
#[test]
fn pip_shim_keeps_signals_the_caller_ignored() {
    let f = Fixture::new();
    f.install("3.12.10/bin/pip");
    f.rehash();
    let sigign = |script: &str| -> u64 {
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", script, "sh"])
            .arg(f.shim("pip"))
            .current_dir(&f.work)
            .env_clear()
            .env("PYENV_ROOT", &f.root)
            .env("PYENV_VERSION", "3.12.10")
            .env("ARGV_ECHO_SIGIGN", "1")
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let hex = text
            .lines()
            .find_map(|l| l.strip_prefix("sigign="))
            .expect("argv-echo printed no sigign line");
        u64::from_str_radix(hex, 16).unwrap()
    };
    // Bits: SIGHUP 0x1, SIGINT 0x2, SIGQUIT 0x4.
    assert_eq!(sigign(r#"trap '' HUP INT QUIT; exec "$1""#) & 0x7, 0x7);
    assert_eq!(sigign(r#"exec "$1""#) & 0x7, 0);
}

#[cfg(unix)]
#[test]
fn sigterm_reaches_the_child_and_the_shim_dies_the_same_way() {
    use std::os::unix::process::ExitStatusExt;
    let f = Fixture::new();
    f.install("3.12.10/bin/pip");
    f.rehash();
    let start = Instant::now();
    let after = f.base.join("after");
    let mut shim = f
        .shim_command(
            "pip",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
                ("ARGV_ECHO_AFTER", after.as_os_str()),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &shim.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = shim.wait().unwrap();
    assert_eq!(status.signal(), Some(15));
    let elapsed = start.elapsed();
    if elapsed < Duration::from_secs(4) {
        std::thread::sleep(Duration::from_secs(4) - elapsed);
    }
    assert!(
        !after.exists(),
        "the child kept running: SIGTERM was not passed on"
    );
}

#[cfg(unix)]
#[test]
fn stdin_reaches_the_program() {
    use std::io::Write;
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let mut child = f
        .shim_command(
            "python",
            &[("PYENV_VERSION", v("3.12.10")), ("ARGV_ECHO_STDIN", v("1"))],
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(stdout(&out).lines().any(|l| l == "stdin=\"hello\\n\""));
}

/// Review M-4 and review focus 3.
#[cfg(unix)]
#[test]
fn works_in_a_deleted_directory() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let doomed = f.base.join("doomed");
    std::fs::create_dir_all(&doomed).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            r#"cd "$1" && export PWD && /bin/rmdir "$1" && exec "$2""#,
            "sh",
        ])
        .arg(&doomed)
        .arg(f.shim("python"))
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .env("PYENV_VERSION", "3.12.10")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Review focus 2, with the real binaries: a write into one shim breaks only the per-user
/// template, never the installed `pyenv-shim`, and `pyenv rehash` makes the shims run again.
#[cfg(unix)]
#[test]
fn linux_writing_into_a_shim_leaves_the_installed_binary_alone() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let installed = std::fs::read(built("pyenv-shim")).unwrap();
    std::fs::write(f.shim("python"), b"2\n").unwrap();
    assert_eq!(std::fs::read(built("pyenv-shim")).unwrap(), installed);
    let r = f.pyenv(&["rehash"], &[]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = f.run_shim("python", &["x".into()], &[("PYENV_VERSION", v("3.12.10"))]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(arg_lines(&out), [line("arg", "x")]);
}

/// Review focus 1, through the automatic rehash: a root left from rpyenv 0.1 (symlink shims,
/// no template yet) is converted by a shim's exit check, seeded from the running shim's binary.
#[cfg(unix)]
#[test]
fn linux_exit_check_converts_a_0_1_root_of_symlink_shims() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.install("3.12.10/bin/pip");
    let installed = std::fs::read(built("pyenv-shim")).unwrap();
    let shims = f.root.join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    for n in ["pip", "python"] {
        std::os::unix::fs::symlink(built("pyenv-shim"), shims.join(n)).unwrap();
    }
    let out = f.run_shim("pip", &["x".into()], &[("PYENV_VERSION", v("3.12.10"))]);
    assert_eq!(out.status.code(), Some(0));
    let t = std::fs::metadata(shims.join(".template/pyenv-shim")).unwrap();
    for n in ["pip", "python"] {
        let m = std::fs::symlink_metadata(shims.join(n)).unwrap();
        assert!(m.is_file(), "{n} should be a hardlink, not a symlink");
        assert_eq!((m.dev(), m.ino()), (t.dev(), t.ino()), "{n}");
    }
    assert_eq!(std::fs::read(built("pyenv-shim")).unwrap(), installed);
}

/// Review focus 1: a symlink to a shim elsewhere on PATH must not make `system` recurse:
/// the hit that resolves into the shims folder is passed over (allowlist D-30).
#[cfg(unix)]
#[test]
fn system_command_never_finds_the_shim() {
    let f = Fixture::new();
    f.install("3.12.10/bin/tool");
    f.rehash();
    let links = f.base.join("links");
    std::fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(f.shim("tool"), links.join("tool")).unwrap();
    let real = f.syspath.join("tool");
    std::fs::copy(built("argv-echo"), &real).unwrap();
    let path = std::env::join_paths([links, f.root.join("shims"), f.syspath.clone()]).unwrap();
    let mut child = f
        .shim_command(
            "tool",
            &[("PYENV_VERSION", v("system")), ("PATH", path.as_os_str())],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // A shim that finds itself runs itself forever: stop it rather than hang the suite.
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the shim was still running after 20 s: it found itself through the symlink");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        stdout(&out).lines().next(),
        Some(line("argv0", &real).as_str())
    );
}

#[cfg(windows)]
#[test]
fn win_shim_passes_plain_arguments_and_the_exit_code() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let args: Vec<OsString> = ["a b", "ñ", "plain"].iter().map(OsString::from).collect();
    let out = f.run_shim(
        "python",
        &args,
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    let text = stdout(&out);
    for a in &args {
        assert!(text.lines().any(|l| l == line("arg", a)), "{text}");
    }
    assert_eq!(out.status.code(), Some(7));
}

/// Fix I-1: a shim baked with no `PYENV_ROOT` in its own environment still finds the root
/// it lives in, rather than falling back to `USERPROFILE`/`.pyenv/pyenv-win`.
#[cfg(windows)]
#[test]
fn win_shim_uses_the_root_it_lives_in() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let out = f
        .shim_command(
            "python",
            &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v("7"))],
        )
        .env_remove("PYENV_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
}

/// A command the selected version lacks is looked up only in the installed versions:
/// `which`'s message on stdout, listing the versions that have it, exit 127 (allowlist D-42).
#[cfg(windows)]
#[test]
fn win_not_found_exits_127() {
    let f = Fixture::new();
    f.install("3.8.2/tool.exe");
    f.install("3.9.1/python.exe");
    f.rehash();
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(
        stdout(&out),
        "pyenv: tool: command not found\r\n\r\nThe 'tool' command exists in these Python versions:\r\n  3.8.2\r\n  \r\n"
    );
    assert_eq!(out.status.code(), Some(127));
}

/// Every shim waits for its child, then runs the stored-state check (allowlist D-39).
#[cfg(windows)]
#[test]
fn win_every_shim_runs_the_rehash_check() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("black.exe");
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("ARGV_ECHO_TOUCH", script.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(f.shim("black").is_file(), "the exit check did not rehash");
}

/// A batch tool in `Scripts` gets the console exe shim, which runs it in a child cmd
/// (allowlist D-47).
#[cfg(windows)]
#[test]
fn win_batch_target_runs_through_its_exe_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("hello.bat"), "@echo hello %1\r\n").unwrap();
    f.rehash();
    let out = f.run_shim("hello", &["world".into()], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(stdout(&out), "hello world\r\n");
}

/// Review focus 5: a running shim must not make rehash fail. Windows either deletes it,
/// or refuses and rehash renames it to `.tool.exe.old`; either way no stale shim remains
/// once it exits.
#[cfg(windows)]
#[test]
fn win_rehash_does_not_trip_over_a_running_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let bat = scripts.join("tool.bat");
    // The shim stays running while cmd runs this for about 3 s.
    std::fs::write(
        &bat,
        "@\"%SystemRoot%\\System32\\ping.exe\" -n 4 127.0.0.1 > NUL\r\n",
    )
    .unwrap();
    f.rehash();
    let mut running = f
        .shim_command("tool", &[("PYENV_VERSION", v("3.9.1"))])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    std::fs::remove_file(&bat).unwrap();
    f.rehash();
    let shims = f.root.join("shims");
    assert!(!shims.join("tool.exe").exists());
    let _ = running.wait();
    f.rehash();
    assert!(!shims.join("tool.exe").exists());
    assert!(!shims.join(".tool.exe.old").exists());
}

/// pyenv-win's child PATH: the version's folder, `Scripts` and `bin`, then the caller's
/// PATH without the shims folder, each entry ending in `;` (allowlist D-40).
#[cfg(windows)]
#[test]
fn win_exec_puts_the_version_folders_first_on_path() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let dir = f.root.join("versions").join("3.9.1");
    let out = f.pyenv(
        &["exec", "python"],
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_ENV", v("PATH"))],
    );
    let expected = format!(
        "{};{};{};{};",
        dir.display(),
        dir.join("Scripts").display(),
        dir.join("bin").display(),
        f.syspath.display()
    );
    let text = stdout(&out);
    assert!(
        text.lines().any(|l| l == line("env PATH", &expected)),
        "{text}"
    );
}

/// Polls until `path` exists (20 s at most).
#[cfg(unix)]
fn wait_until_exists(path: &std::path::Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Spec §8 Safety (M5b): a rehash ended by SIGINT, SIGTERM or SIGHUP removes its lock
/// instead of leaving it for two minutes, and dies by that signal.
#[cfg(target_os = "linux")]
#[test]
fn an_interrupted_rehash_removes_its_lock() {
    use std::os::unix::process::ExitStatusExt;
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let f = Fixture::new();
        f.install("3.12.10/bin/python");
        let lock = f.root.join("shims").join(".rehash.lock");
        let mut child = f
            .command(
                &built("pyenv"),
                &[("RPYENV_TEST_HOLD_REHASH_MS", v("20000"))],
            )
            .arg("rehash")
            .spawn()
            .unwrap();
        wait_until_exists(&lock);
        // SAFETY: sends a signal to the child this test started.
        unsafe { libc::kill(child.id() as libc::pid_t, signal) };
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(signal), "signal {signal}");
        assert!(!lock.exists(), "signal {signal} left the lock behind");
    }
}

/// Review focus 2: under `nohup` (SIGHUP ignored) the rehash survives SIGHUP and finishes.
#[cfg(target_os = "linux")]
#[test]
fn rehash_keeps_an_ignored_sighup() {
    use std::os::unix::process::CommandExt;
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    let lock = f.root.join("shims").join(".rehash.lock");
    let mut cmd = f.command(
        &built("pyenv"),
        &[("RPYENV_TEST_HOLD_REHASH_MS", v("1500"))],
    );
    cmd.arg("rehash");
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    wait_until_exists(&lock);
    // SAFETY: sends a signal to the child this test started.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGHUP) };
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert!(f.shim("python").exists());
    assert!(!lock.exists());
}

/// Spec §8 point 3 on Linux: a script installed while the program runs gets its shim
/// before the program exits.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_makes_a_new_scripts_shim_while_the_program_runs() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut child = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("8000")),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_until_exists(&ready);
    wait_log(&log, "live=watching");
    f.install("3.12.10/bin/black");
    let start = Instant::now();
    while !f.shim("black").exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the program ended first"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no shim while the program ran"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Review focus 4: the watcher isn't the program's child (spec §8: "Why two forks").
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_is_not_a_child_of_the_program() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("RPYENV_LIVE_REHASH", v("1")),
            ("ARGV_ECHO_CHILDREN", v("1")),
        ],
    );
    assert!(stdout(&out).contains("children=0\n"), "{}", stdout(&out));
}

/// Review focus 1: the watcher holds none of the caller's descriptors (its stdin, stdout and
/// stderr are /dev/null) and is in a session of its own.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_is_detached() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut child = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("6000")),
            ],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wait_until_exists(&ready);
    let text = wait_log(&log, "live=watching");
    let pid: u32 = text
        .split_whitespace()
        .filter_map(|w| w.strip_prefix("pid="))
        .last()
        .and_then(|p| p.parse().ok())
        .expect("watcher pid");
    for fd in 0..3 {
        let target = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).unwrap();
        assert_eq!(target, std::path::Path::new("/dev/null"), "fd {fd}");
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let fields: Vec<&str> = stat
        .rsplit(')')
        .next()
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(fields[3], pid.to_string(), "session id (setsid)");
    let _ = child.kill();
    let _ = child.wait();
}

/// Review focus 5: watching starts only for a program still running after the start delay.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_skips_a_quick_program() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let start = Instant::now();
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_LIVE_REHASH", v("1")),
        ],
    );
    let took = start.elapsed();
    assert!(out.status.success());
    std::thread::sleep(Duration::from_millis(1500));
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !text.contains("live=watching") || took >= Duration::from_secs(1),
        "watched a program that took {took:?}:\n{text}"
    );
}

/// Polls the debug log until it contains `needle` (20 s at most); returns the log.
#[cfg(unix)]
fn wait_log(log: &std::path::Path, needle: &str) -> String {
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no {needle:?} in:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
