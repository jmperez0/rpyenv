//! `pyenv sh-shell`, `sh-rehash`, `shell`, and the command listing on Linux (M3L).
#![cfg(unix)]

mod common;
use common::Fixture;
use std::path::Path;

fn run(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

fn out(s: &str) -> (String, String, i32) {
    (s.to_string(), String::new(), 0)
}

#[test]
fn no_arguments_prints_code_or_fails() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["sh-shell"], &[]),
        (
            String::new(),
            "pyenv: no shell-specific version configured\n".to_string(),
            1
        )
    );
    for shell in ["bash", "fish", "pwsh"] {
        assert_eq!(
            run(
                &f,
                &["sh-shell"],
                &[("PYENV_SHELL", shell), ("PYENV_VERSION", "1.2.3")]
            ),
            out("echo \"$PYENV_VERSION\"\n"),
            "{shell}"
        );
    }
    // An empty first argument counts as none.
    assert_eq!(run(&f, &["sh-shell", ""], &[]).2, 1);
}

#[test]
fn unset_per_shell() {
    let f = Fixture::new();
    let unset = |shell| {
        run(
            &f,
            &["sh-shell", "--unset", "extra"],
            &[("PYENV_SHELL", shell)],
        )
    };
    assert_eq!(
        unset("bash"),
        out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"\nunset PYENV_VERSION\n")
    );
    assert_eq!(
        unset("fish"),
        out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -e PYENV_VERSION\n")
    );
    assert_eq!(
        unset("pwsh"),
        out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $null, $Env:PYENV_VERSION\n")
    );
    // PYENV_SHELL empty: $SHELL decides.
    assert_eq!(
        run(
            &f,
            &["sh-shell", "--unset"],
            &[("PYENV_SHELL", ""), ("SHELL", "/usr/bin/fish")]
        )
        .0,
        "set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -e PYENV_VERSION\n"
    );
}

#[test]
fn revert_per_shell() {
    let f = Fixture::new();
    let revert = |shell| run(&f, &["sh-shell", "-"], &[("PYENV_SHELL", shell)]).0;
    assert_eq!(
        revert("bash"),
        r#"if [ -n "${PYENV_VERSION_OLD+x}" ]; then
  if [ -n "$PYENV_VERSION_OLD" ]; then
    PYENV_VERSION_OLD_="$PYENV_VERSION"
    export PYENV_VERSION="$PYENV_VERSION_OLD"
    PYENV_VERSION_OLD="$PYENV_VERSION_OLD_"
    unset PYENV_VERSION_OLD_
  else
    PYENV_VERSION_OLD="$PYENV_VERSION"
    unset PYENV_VERSION
  fi
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
fi
"#
    );
    assert!(revert("fish").starts_with("if set -q PYENV_VERSION_OLD\n"));
    // The fifth line ends with a space (libexec/pyenv-sh-shell:89).
    assert_eq!(
        revert("pwsh"),
        "if ( Get-Item -Path Env:\\PYENV_VERSION* ) {\n  $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $Env:PYENV_VERSION_OLD, $Env:PYENV_VERSION\n} else {\n  Write-Error \"pyenv: Env:PYENV_VERSION_OLD is not set\"\n  return $false\n} \n"
    );
}

#[test]
fn set_per_shell_stores_the_literal_arguments() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    let set = |args: &[&str], shell| {
        let mut a = vec!["sh-shell"];
        a.extend_from_slice(args);
        run(&f, &a, &[("PYENV_SHELL", shell)])
    };
    // `3.12` resolves for validation, but the literal argument is stored (M3L).
    assert_eq!(
        set(&["3.12"], "bash"),
        out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"\nexport PYENV_VERSION=\"3.12\"\n")
    );
    assert_eq!(
        set(&["3.12.1", "3.11.9"], "fish"),
        out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -gx PYENV_VERSION \"3.12.1:3.11.9\"\n")
    );
    assert_eq!(
        set(&["3.12.1"], "pwsh"),
        out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = \"3.12.1\", $Env:PYENV_VERSION\n")
    );
    // The same value as the current one prints nothing.
    assert_eq!(
        run(&f, &["sh-shell", "3.12.1"], &[("PYENV_VERSION", "3.12.1")]),
        out("")
    );
}

#[test]
fn a_version_not_installed_fails_with_false() {
    let f = Fixture::new();
    f.version("3.12.1");
    assert_eq!(
        run(&f, &["sh-shell", "1.2.3"], &[]),
        (
            "false\n".to_string(),
            "pyenv: version `1.2.3' not installed\n".to_string(),
            1
        )
    );
    // Every argument is a version name, options included.
    assert_eq!(
        run(&f, &["sh-shell", "3.12.1", "--unset"], &[]).1,
        "pyenv: version `--unset' not installed\n"
    );
}

#[test]
fn sh_rehash_prints_code_and_does_not_rehash() {
    let f = Fixture::new();
    let rehash = |shell| run(&f, &["sh-rehash", "x"], &[("PYENV_SHELL", shell)]);
    assert_eq!(
        rehash("bash"),
        out("command pyenv rehash\nhash -r 2>/dev/null || true\n")
    );
    assert_eq!(rehash("fish"), out("command pyenv rehash\n"));
    assert_eq!(
        rehash("pwsh"),
        out("& (get-command pyenv -commandtype application) rehash\n")
    );
    assert!(!f.root.join("shims").exists());
}

const SHELL_HELP: &str = "Usage: pyenv shell <version>...\n       pyenv shell -\n       pyenv shell --unset\n\nSets a shell-specific Python version by setting the `PYENV_VERSION'\nenvironment variable in your shell. This version overrides local\napplication-specific versions and the global version.\n\n<version> should be a string matching a Python version known to pyenv.\nThe special version string `system' will use your default system Python.\nRun `pyenv versions' for a list of available Python versions.\n\nWhen `-` is passed instead of the version string, the previously set\nversion will be restored. With `--unset`, the `PYENV_VERSION`\nenvironment variable gets unset, restoring the environment to the\nstate before the first `pyenv shell` call.\n\n";

#[test]
fn shell_without_integration_and_help() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["shell", "3.12"], &[]),
        (
            String::new(),
            "pyenv: shell integration not enabled. Run `pyenv init' for instructions.\n"
                .to_string(),
            1
        )
    );
    // The dispatcher prints the help command for the shell function to evaluate.
    assert_eq!(
        run(&f, &["sh-shell", "--help"], &[]),
        out("pyenv help \"sh-shell\"\n")
    );
    assert_eq!(
        run(&f, &["sh-rehash", "--help"], &[]),
        out("pyenv help \"sh-rehash\"\n")
    );
    assert_eq!(run(&f, &["help", "shell"], &[]), out(SHELL_HELP));
    assert_eq!(run(&f, &["help", "sh-shell"], &[]), out(SHELL_HELP));
    assert_eq!(
        run(&f, &["help", "--usage", "shell"], &[]),
        out("Usage: pyenv shell <version>...\n       pyenv shell -\n       pyenv shell --unset\n")
    );
    assert_eq!(
        run(&f, &["help", "sh-rehash"], &[]),
        (
            String::new(),
            "Sorry, this command isn't documented yet.\n".to_string(),
            1
        )
    );
    assert_eq!(run(&f, &["help", "--usage", "sh-rehash"], &[]), out(""));
}

#[test]
fn commands_lists_sh_commands_by_their_short_name() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["commands", "--sh"], &[]), out("rehash\nshell\n"));
    let all = run(&f, &["commands"], &[]).0;
    assert_eq!(all.lines().filter(|l| *l == "shell").count(), 1);
    assert_eq!(all.lines().filter(|l| *l == "rehash").count(), 1);
    assert!(!all.contains("sh-"));
    let no_sh = run(&f, &["commands", "--no-sh"], &[]).0;
    assert!(no_sh.lines().any(|l| l == "rehash"));
    assert!(!no_sh.lines().any(|l| l == "shell"));
    assert!(run(&f, &["help"], &[])
        .0
        .contains("   shell       Set or show the shell-specific Python version\n"));
}

/// The printed code, evaluated by a real bash.
#[test]
fn bash_evaluates_the_code() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_pyenv"), f.syspath.join("pyenv")).unwrap();
    let script = r#"set -e
eval "$(pyenv sh-shell 3.12.1)"; echo "a[$PYENV_VERSION][${PYENV_VERSION_OLD-unset}]"
eval "$(pyenv sh-shell 3.11.9)"; echo "b[$PYENV_VERSION][$PYENV_VERSION_OLD]"
eval "$(pyenv sh-shell -)"; echo "c[$PYENV_VERSION][$PYENV_VERSION_OLD]"
eval "$(pyenv sh-shell --unset)"; echo "d[${PYENV_VERSION-unset}][$PYENV_VERSION_OLD]"
"#;
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[("PYENV_SHELL", "bash")])
        .arg("-c")
        .arg(script)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "a[3.12.1][]\nb[3.11.9][3.12.1]\nc[3.12.1][3.11.9]\nd[unset][3.12.1]\n",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
