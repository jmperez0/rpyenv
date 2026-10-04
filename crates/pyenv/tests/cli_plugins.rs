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
