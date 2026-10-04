//! Plugin dispatch on Linux (spec §6; M1 reference "Environment setup").
#![cfg(unix)]

mod common;
use common::Fixture;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// An executable shell script at `root/plugins/<plugin>/bin/pyenv-<cmd>`.
fn plugin(f: &Fixture, plugin: &str, cmd: &str, body: &str) -> std::path::PathBuf {
    let p = f
        .root
        .join("plugins")
        .join(plugin)
        .join("bin")
        .join(format!("pyenv-{cmd}"));
    f.file(&p, &format!("#!/bin/sh\n{body}"));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

#[test]
fn a_plugin_runs_with_the_dispatcher_s_environment() {
    let f = Fixture::new();
    plugin(
        &f,
        "hello",
        "hello",
        "printf '%s|' \"$@\"; echo \"$PYENV_ROOT|$PYENV_DIR|${PYENV_HOOK_PATH%%:*}\"\n",
    );
    let r = f.root.display();
    let w = f.work.display();
    assert_eq!(
        run(&f, &["hello", "a b", "c"]),
        (format!("a b|c|{r}|{w}|{r}/pyenv.d\n"), String::new(), 0)
    );
    // The plugin folders and the built-in links lead PATH, after <prefix>/libexec.
    plugin(
        &f,
        "path",
        "path",
        // Shell builtins only: the fixture's PATH has no `tr` or `head`.
        "IFS=:; set -- $PATH; printf '%s\\n' \"$1\" \"$2\" \"$3\"\n",
    );
    let out = run(&f, &["path"]).0;
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].ends_with("/libexec"), "{out}");
    assert_eq!(lines[1], format!("{r}/plugins/path/bin"));
    assert_eq!(lines[2], format!("{r}/plugins/hello/bin"));
}

#[test]
fn unknown_commands_and_help() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["nosuch"]),
        (
            String::new(),
            "pyenv: no such command `nosuch'\n".to_string(),
            1
        )
    );
    plugin(&f, "x", "sh-hi", "echo hi\n");
    assert_eq!(
        run(&f, &["sh-hi", "--help"]),
        ("pyenv help \"sh-hi\"\n".to_string(), String::new(), 0)
    );
}

/// Review focus 1.
#[test]
fn a_plugin_cannot_shadow_a_built_in() {
    let f = Fixture::new();
    plugin(&f, "x", "root", "echo plugin\n");
    assert_eq!(run(&f, &["root"]).0, format!("{}\n", f.root.display()));
}

/// Review focus 2 and Decision 2: `pyenv-prefix` by name reaches rpyenv through the links.
#[test]
fn a_bash_plugin_reaches_built_ins_by_name() {
    let f = Fixture::new();
    f.version("3.12.1");
    plugin(
        &f,
        "x",
        "where",
        "pyenv-prefix 3.12.1; pyenv-version-name\n",
    );
    let r = f.pyenv_env(&["where"], &[("PYENV_VERSION", "3.12.1")]);
    assert_eq!(
        (r.stdout, r.code),
        (format!("{}/versions/3.12.1\n3.12.1\n", f.root.display()), 0),
        "{}",
        r.stderr
    );
    let link = f.root.join(".rpyenv/libexec/pyenv-version-name");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        Path::new(env!("CARGO_BIN_EXE_pyenv"))
            .canonicalize()
            .unwrap()
    );
}

/// Review focus 3.
#[test]
fn a_plugin_runs_when_the_root_is_read_only() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = Fixture::new();
    plugin(&f, "x", "hello", "echo hi\n");
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let got = run(&f, &["hello"]);
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(got, ("hi\n".to_string(), String::new(), 0));
}

/// Decision 3: a link named `pyenv-<built-in>` acts as `pyenv <built-in>`.
#[test]
fn multicall_by_argv0() {
    let f = Fixture::new();
    let link = f.base.join("pyenv-root");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_pyenv"), &link).unwrap();
    let o = f.command(&link, &f.work, &[]).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        format!("{}\n", f.root.display())
    );
}

fn hello(f: &Fixture, block: &str) {
    plugin(f, "hello", "hello", &format!("{block}echo hello\n"));
}

#[test]
fn help_for_a_plugin_reads_its_comment_block() {
    let f = Fixture::new();
    hello(&f, "# Usage: pyenv hello <world>\n# Summary: Says \"hello\" to you, from pyenv\n# This command is useful for saying hello.\n");
    assert_eq!(
        run(&f, &["help", "hello"]).0,
        "Usage: pyenv hello <world>\n\nThis command is useful for saying hello.\n\n"
    );
    assert_eq!(
        run(&f, &["hello", "--help"]).0,
        run(&f, &["help", "hello"]).0
    );
    assert_eq!(
        run(&f, &["help", "--usage", "hello"]).0,
        "Usage: pyenv hello <world>\n"
    );
    assert!(run(&f, &["help"])
        .0
        .contains("   hello       Says \"hello\" to you, from pyenv\n"));

    let g = Fixture::new();
    hello(&g, "# Usage: pyenv hello <world>\n#        pyenv hi [everybody]\n# Summary: Says \"hello\" to you, from pyenv\n");
    assert_eq!(
        run(&g, &["help", "hello"]).0,
        "Usage: pyenv hello <world>\n       pyenv hi [everybody]\n\nSays \"hello\" to you, from pyenv\n\n"
    );
    let h = Fixture::new();
    hello(
        &h,
        "# Usage: pyenv hello <world>\n# Summary: S\n# Line one.\n#\n# Line two.\n",
    );
    assert_eq!(
        run(&h, &["help", "hello"]).0,
        "Usage: pyenv hello <world>\n\nLine one.\n\nLine two.\n\n"
    );
    let u = Fixture::new();
    hello(&u, "# nothing documented\n");
    assert_eq!(
        run(&u, &["help", "hello"]),
        (
            String::new(),
            "Sorry, this command isn't documented yet.\n".to_string(),
            1
        )
    );
}

#[test]
fn commands_completions_and_init_include_plugins() {
    let f = Fixture::new();
    plugin(&f, "x", "hello", "# provide pyenv completions\nif [ \"$1\" = --complete ]; then shift; for a; do echo \"$a\"; done; fi\n");
    plugin(&f, "x", "sh-activate", "echo :\n");
    let commands = run(&f, &["commands"]).0;
    assert!(commands.lines().any(|l| l == "hello") && commands.lines().any(|l| l == "activate"));
    assert_eq!(
        run(&f, &["commands", "--sh"]).0,
        "activate\nrehash\nshell\n"
    );
    assert_eq!(
        run(&f, &["completions", "hello", "happy", "world"]).0,
        "--help\nhappy\nworld\n"
    );
    // Decision 7: plugin sh-* commands join the routed set.
    assert!(run(&f, &["init", "-", "bash"])
        .0
        .contains("\n  activate|rehash|shell)\n"));
}

/// Review focus 4, corrected by probing upstream: bash's `command -v` falls back to a
/// non-executable file, so it is listed and documented, and running it fails as an
/// unstartable file does (allowlist D-43).
#[test]
fn a_non_executable_plugin_is_found_but_cannot_start() {
    let f = Fixture::new();
    let p = plugin(
        &f,
        "x",
        "plain",
        "# Summary: Plain one\n# Usage: pyenv plain\necho no\n",
    );
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(run(&f, &["commands"]).0.lines().any(|l| l == "plain"));
    assert_eq!(
        run(&f, &["help", "plain"]).0,
        "Usage: pyenv plain\n\nPlain one\n\n"
    );
    let (out, err, code) = run(&f, &["plain"]);
    assert_eq!((out.as_str(), code), ("", 126), "{err}");
    assert!(err.ends_with("Permission denied\n"), "{err}");
}

/// Final review #1: parallel first runs, as when several shells start, each find every
/// built-in link (setup is atomic per link and never drops the folder).
#[test]
fn parallel_first_runs_all_find_the_built_in_links() {
    for _round in 0..5 {
        let f = Fixture::new();
        plugin(&f, "x", "where", "pyenv-root\n");
        let outs: Vec<String> = std::thread::scope(|s| {
            let runs: Vec<_> = (0..10).map(|_| s.spawn(|| run(&f, &["where"]))).collect();
            runs.into_iter()
                .map(|h| {
                    let (out, err, code) = h.join().unwrap();
                    format!("{code}|{out}|{err}")
                })
                .collect()
        });
        let want = format!("0|{}\n|", f.root.display());
        for o in outs {
            assert_eq!(o, want);
        }
    }
}

/// Final review #2: pyenv-virtualenv's pattern, `set -e` and a hooks list read by name,
/// works; `pyenv hooks` lists `<hook path>/<cmd>/*.bash`, resolved (rpyenv runs none).
#[test]
fn a_plugin_can_list_hooks_by_name() {
    let f = Fixture::new();
    plugin(
        &f,
        "venv",
        "sh-activate",
        "set -e\nIFS='\n' scripts=(`pyenv-hooks activate`)\necho ok\n",
    );
    // bash arrays: run the plugin under bash, as pyenv-virtualenv does.
    let p = f.root.join("plugins/venv/bin/pyenv-sh-activate");
    let body = std::fs::read_to_string(&p)
        .unwrap()
        .replacen("#!/bin/sh", "#!/bin/bash", 1);
    std::fs::write(&p, body).unwrap();
    assert_eq!(
        run(&f, &["sh-activate"]),
        ("ok\n".to_string(), String::new(), 0)
    );
    let hooks = f.base.join("my hooks");
    f.file(&hooks.join("exec/b.bash"), "")
        .file(&hooks.join("exec/a.bash"), "")
        .file(&hooks.join("exec/c.sh"), "");
    let h = hooks.display().to_string();
    let r = f.pyenv_env(&["hooks", "exec"], &[("PYENV_HOOK_PATH", h.as_str())]);
    assert_eq!(r.stdout, format!("{h}/exec/a.bash\n{h}/exec/b.bash\n"));
    assert_eq!(
        run(&f, &["hooks"]),
        (
            String::new(),
            "Usage: pyenv hooks <command>\n".to_string(),
            1
        )
    );
}

/// Final review #3: rpyenv's own `pyenv-shim` beside `pyenv` on PATH isn't a command.
#[test]
fn rpyenv_s_own_shim_is_not_a_plugin() {
    let f = Fixture::new();
    let exe = f.syspath.join("pyenv");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &exe).unwrap();
    let shim = Path::new(env!("CARGO_BIN_EXE_pyenv")).with_file_name("pyenv-shim");
    std::fs::copy(&shim, f.syspath.join("pyenv-shim")).unwrap();
    let go = |args: &[&str]| {
        for _ in 0..100 {
            match f.command(&exe, &f.work, &[]).args(args).output() {
                Ok(o) => {
                    return (
                        String::from_utf8_lossy(&o.stdout).into_owned(),
                        String::from_utf8_lossy(&o.stderr).into_owned(),
                    )
                }
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(e) => panic!("{e}"),
            }
        }
        panic!("busy");
    };
    assert!(!go(&["commands"]).0.lines().any(|l| l == "shim"));
    assert_eq!(go(&["shim"]).1, "pyenv: no such command `shim'\n");
}

/// A plugin gets the full `PYENV_HOOK_PATH` already; its `pyenv-hooks` lists each hook once.
#[test]
fn a_plugin_s_hooks_are_listed_once() {
    let f = Fixture::new();
    f.file(&f.root.join("pyenv.d/activate/x.bash"), "");
    plugin(&f, "venv", "list", "pyenv-hooks activate\n");
    let x = f.root.join("pyenv.d/activate/x.bash");
    assert_eq!(run(&f, &["list"]).0, format!("{}\n", x.display()));
}
