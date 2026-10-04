//! `pyenv completions` and `--complete` (M3L "pyenv completions"; Decision 5).

mod common;
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

/// One version, 3.12.1, with `pip` and `python`, rehashed so `shims --short` lists both.
fn fixture() -> Fixture {
    let f = Fixture::new();
    for exe in ["pip", "python"] {
        let name = if cfg!(windows) {
            format!("{exe}.exe")
        } else {
            exe.to_string()
        };
        let rel = if cfg!(windows) {
            format!("3.12.1/{name}")
        } else {
            format!("3.12.1/bin/{name}")
        };
        f.exe(&rel);
    }
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    f
}

/// M3L "pyenv completions <cmd> for every command, at 2.8.8", for the commands rpyenv has.
#[cfg(unix)]
#[test]
fn every_linux_command_completes_as_upstream() {
    let f = fixture();
    for (cmd, want) in [
        ("--version", "--help"),
        ("latest", "--help"),
        ("rehash", "--help"),
        ("root", "--help"),
        ("version", "--help"),
        ("version-name", "--help"),
        ("commands", "--help --sh --no-sh"),
        ("exec", "--help --environment pip python"),
        ("global", "--help system 3.12.1"),
        ("prefix", "--help system 3.12.1"),
        ("local", "--help --unset system 3.12.1"),
        ("shell", "--help --unset system 3.12.1"),
        ("sh-shell", "--help --unset system 3.12.1"),
        ("init", "--help - --path --install --no-push-path --no-rehash --detect-shell bash fish ksh pwsh zsh"),
        ("shims", "--help --short"),
        ("versions", "--help --bare --skip-aliases --skip-envs"),
        ("whence", "--help --path pip python"),
        ("which", "--help pip python"),
        ("uninstall", "--help --force 3.12.1"),
    ] {
        let (out, err, code) = run(&f, &["completions", cmd]);
        assert_eq!(
            (out.split_whitespace().collect::<Vec<_>>().join(" ").as_str(), err.as_str(), code),
            (want, "", 0),
            "{cmd}"
        );
    }
    // `completions` and `help` list every command after their own words.
    let commands = run(&f, &["commands"]).0;
    assert_eq!(
        run(&f, &["completions", "completions"]).0,
        format!("--help\n{commands}")
    );
    assert_eq!(
        run(&f, &["completions", "help"]).0,
        format!("--help\n--usage\n{commands}")
    );
    // `install`: --help, nine options, then every definition.
    let install = run(&f, &["completions", "install"]).0;
    let list = run(&f, &["install", "--list", "--bare"]).0;
    assert!(install.starts_with("--help\n--bare\n--list\n--force\n--skip-existing\n--keep\n--patch\n--verbose\n--version\n--debug\n"));
    assert!(install.ends_with(&list));
}

#[cfg(unix)]
#[test]
fn completions_edge_cases() {
    let f = fixture();
    assert_eq!(
        run(&f, &["completions"]),
        (
            String::new(),
            "Usage: pyenv completions <command> [arg1 arg2...]\n".to_string(),
            1
        )
    );
    // A command that exists nowhere: no output at all, exit 1.
    assert_eq!(
        run(&f, &["completions", "nosuchcmd"]),
        (String::new(), String::new(), 1)
    );
    assert_eq!(
        run(&f, &["completions", "--complete"]),
        run(&f, &["commands"])
    );
    // Extra arguments are ignored by the built-in commands.
    assert_eq!(
        run(&f, &["completions", "shell", "foo", "bar"]),
        run(&f, &["completions", "shell"])
    );
    // `<cmd> --complete` directly: version has no marker but answers.
    assert_eq!(run(&f, &["version", "--complete"]).0, "--bare\n");
    assert_eq!(
        run(&f, &["exec", "--complete"]).0,
        "--environment\npip\npython\n"
    );
    assert_eq!(run(&f, &["init", "--complete"]).0.lines().count(), 11);
    // `latest` has no `--complete`: the flag is a prefix to match.
    assert_eq!(run(&f, &["latest", "--complete"]).2, 1);
    assert_eq!(
        run(&f, &["help", "completions"]).0,
        "Usage: pyenv completions <command> [arg1 arg2...]\n"
    );
}

/// Decision 11: `sh-rehash --complete` rehashes, as upstream's does.
#[cfg(unix)]
#[test]
fn sh_rehash_complete_rehashes() {
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    assert_eq!(
        run(&f, &["completions", "sh-rehash"]),
        ("--help\n".to_string(), String::new(), 0)
    );
    assert!(f.root.join("shims").join("python").exists());
}

/// The vendored bash script, sourced into a real bash (M3L "The completion scripts").
#[cfg(unix)]
#[test]
fn the_bash_script_completes_through_pyenv() {
    let f = fixture();
    for (name, target) in [
        ("pyenv", env!("CARGO_BIN_EXE_pyenv")),
        ("bash", "/bin/bash"),
    ] {
        std::os::unix::fs::symlink(target, f.syspath.join(name)).unwrap();
    }
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../completions/pyenv.bash");
    let body = format!(
        "source '{}'\nCOMP_WORDS=(pyenv sh); COMP_CWORD=1; _pyenv; echo \"${{COMPREPLY[*]}}\"\nCOMP_WORDS=(pyenv shell ''); COMP_CWORD=2; _pyenv; echo \"${{COMPREPLY[*]}}\"\n",
        script.display()
    );
    let o = f
        .command(std::path::Path::new("/bin/bash"), &f.work, &[])
        .args(["-c", &body])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "shell shims\n--help --unset system 3.12.1\n",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Windows: `pyenv completions` reads the table, and `<cmd> --complete` keeps pyenv-win's
/// behavior (Decision 5).
#[cfg(windows)]
#[test]
fn windows_completions_read_the_table() {
    let f = fixture();
    assert_eq!(
        run(&f, &["completions", "versions"]).0,
        "--help\r\n--bare\r\n--skip-aliases\r\n"
    );
    assert_eq!(
        run(&f, &["completions", "update"]).0,
        "--help\r\n--ignore\r\n"
    );
    assert_eq!(
        run(&f, &["completions", "uninstall"]).0,
        "--help\r\n--force\r\n--all\r\n3.12.1\r\n"
    );
    assert_eq!(run(&f, &["completions", "latest"]).0, "--help\r\n");
    assert!(!run(&f, &["versions", "--complete"]).0.contains("--bare"));
}
