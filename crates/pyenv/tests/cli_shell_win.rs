//! Windows `pyenv shell`, `sh-shell` and `sh-rehash` (M3W; Decisions 1, 6, 10).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;

fn run(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

fn not_installed(name: &str) -> String {
    format!("pyenv specific python requisite didn't meet. Project is using different version of python.\r\nInstall python '{name}' by typing: 'pyenv install {name}'\r\n")
}

const HELP: &str = "Usage: pyenv shell <version>\r\n       pyenv shell --unset\r\n\r\nSets a shell-specific Python version by setting the `PYENV_VERSION'\r\nenvironment variable in your shell. This version overrides local\r\napplication-specific versions and the global version.\r\n\r\n";

#[test]
fn pyenv_win_output_forms() {
    let f = Fixture::new();
    f.version("3.7.7").version("3.8.9");
    assert_eq!(
        run(&f, &["shell"], &[]),
        (
            "no shell-specific version configured\r\n".to_string(),
            String::new(),
            0
        )
    );
    assert_eq!(
        run(&f, &["shell"], &[("PYENV_VERSION", "3.7.7 3.8.9")]).0,
        "3.7.7 3.8.9\r\n"
    );
    assert_eq!(
        run(&f, &["shell", "9.9.9"], &[]),
        (not_installed("9.9.9"), String::new(), 1)
    );
    assert_eq!(
        run(&f, &["shell", "3.7.7", "9.9.9"], &[]).0,
        not_installed("9.9.9")
    );
    assert_eq!(
        run(&f, &["shell", "3.7.7", "--unset"], &[]).0,
        not_installed("--unset")
    );
    // Decision 6: no prefix resolution and no `system`.
    assert_eq!(run(&f, &["shell", "3.7"], &[]).0, not_installed("3.7"));
    assert_eq!(
        run(&f, &["shell", "system"], &[]).0,
        not_installed("system")
    );
    assert_eq!(run(&f, &["shell", ""], &[]).0, not_installed(""));
    for args in [
        &["shell", "--help"][..],
        &["--help", "shell"],
        &["help", "shell"],
    ] {
        assert_eq!(run(&f, args, &[]).0, HELP, "{args:?}");
    }
}

/// allowlist D-89
/// Review focus 3: run by a program that isn't a shell (this test), `shell` prints every
/// shell's line, labeled, and exits 1.
#[test]
fn unknown_parent_prints_every_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    assert_eq!(
        run(&f, &["shell", "3.7.7"], &[]),
        (
            "cmd: set \"PYENV_VERSION=3.7.7\"\r\nPowerShell: $Env:PYENV_VERSION = '3.7.7'\r\nbash: export PYENV_VERSION='3.7.7'\r\nfish: set -gx PYENV_VERSION '3.7.7'\r\n".to_string(),
            "pyenv: shell integration is not enabled, so nothing was changed: run the line for your shell above.\r\n".to_string(),
            1
        )
    );
    // With only PYENV_SHELL to go by, that shell's line. `--unset` matches in any case.
    assert_eq!(
        run(&f, &["shell", "--UNSET", "x"], &[("PYENV_SHELL", "pwsh")]),
        (
            "Remove-Item Env:PYENV_VERSION -ErrorAction SilentlyContinue\r\n".to_string(),
            "pyenv: shell integration is not enabled in this shell, so nothing was changed: run the command above.\r\npyenv: to enable it, see `pyenv init pwsh`.\r\n".to_string(),
            1
        )
    );
}

/// Review focus 4, Decision 1: cmd's own line wins over an inherited PYENV_SHELL.
#[test]
fn cmd_parent_wins_over_inherited_pyenv_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let mut c = host(&f, &winshell::cmd_exe(), &[("PYENV_SHELL", "pwsh")], &[]);
    c.args(["/d", "/c", "call", "pyenv", "shell", "3.7.7"]);
    assert_eq!(
        output(c),
        (
            "set \"PYENV_VERSION=3.7.7\"\r\n".to_string(),
            "pyenv: cmd has no shell integration, so nothing was changed: run the command above.\r\n".to_string(),
            1
        )
    );
}

#[test]
fn x86_appends_win32_unless_present() {
    let f = Fixture::new();
    f.version("3.7.7-win32");
    let x86 = [("PYENV_FORCE_ARCH", "X86"), ("PYENV_SHELL", "cmd")];
    assert_eq!(
        run(&f, &["shell", "3.7.7"], &x86).0,
        "set \"PYENV_VERSION=3.7.7-win32\"\r\n"
    );
    assert_eq!(
        run(&f, &["shell", "3.7.7-WIN32"], &x86).0,
        "set \"PYENV_VERSION=3.7.7-WIN32\"\r\n"
    );
}

/// `sh-shell`'s code, evaluated by Windows PowerShell 5.1 and, when installed, pwsh 7.
#[test]
fn powershell_evaluates_sh_shell() {
    let f = Fixture::new();
    f.version("3.7.7").version("3.8.9");
    winshell::install_pyenv(&f);
    let script = "iex ((pyenv sh-shell 3.7.7 3.8.9) -join \"`n\"); $env:PYENV_VERSION; iex ((pyenv sh-shell --unset) -join \"`n\"); [string]::IsNullOrEmpty($env:PYENV_VERSION)";
    let mut shells = vec![winshell::powershell()];
    shells.extend(winshell::pwsh());
    for sh in shells {
        let mut c = host(&f, &sh, &[("PYENV_SHELL", "pwsh")], &[]);
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        let (out, err, code) = output(c);
        assert_eq!(
            (out.as_str(), code),
            ("3.7.7 3.8.9\r\nTrue\r\n", 0),
            "{}: {err}",
            sh.display()
        );
    }
}

/// Review focus 1: a value with a quote character survives each shell's code. POSIX and
/// fish code is `\n`-terminated even on Windows.
#[test]
fn quotes_survive_each_shell() {
    let f = Fixture::new();
    f.version("a'b");
    winshell::install_pyenv(&f);
    assert_eq!(
        run(&f, &["sh-shell", "a'b"], &[("PYENV_SHELL", "fish")]).0,
        "set -gx PYENV_VERSION 'a\\'b'\n"
    );
    assert_eq!(
        run(&f, &["sh-shell", "a'b"], &[("PYENV_SHELL", "bash")]).0,
        "export PYENV_VERSION='a'\\''b'\n"
    );
    let mut c = host(&f, &winshell::powershell(), &[("PYENV_SHELL", "pwsh")], &[]);
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "iex ((pyenv sh-shell \"a'b\") -join \"`n\"); $env:PYENV_VERSION",
    ]);
    assert_eq!(output(c).0, "a'b\r\n");
    if let Some(bash) = winshell::git_bash() {
        let mut c = host(&f, &bash, &[("PYENV_SHELL", "bash")], &[]);
        c.args([
            "-c",
            "eval \"$(pyenv sh-shell \"a'b\")\"; echo \"$PYENV_VERSION\"",
        ]);
        assert_eq!(output(c).0, "a'b\n");
    }
}

#[test]
fn sh_shell_for_the_posix_function_and_failures() {
    let f = Fixture::new();
    f.version("3.7.7");
    let bash = [("PYENV_SHELL", "bash"), ("PYENV_VERSION", "3.7.7")];
    assert_eq!(run(&f, &["sh-shell"], &bash).0, "echo \"$PYENV_VERSION\"\n");
    // Evaluated by bash or fish: the message on stderr and `false` to run.
    let (out, err, code) = run(&f, &["sh-shell", "9.9.9"], &[("PYENV_SHELL", "bash")]);
    assert_eq!(
        (out.as_str(), err, code),
        ("false\n", not_installed("9.9.9"), 1)
    );
    // Evaluated by the PowerShell function: the message on stdout, which it prints.
    assert_eq!(
        run(&f, &["sh-shell", "9.9.9"], &[("PYENV_SHELL", "pwsh")]).0,
        not_installed("9.9.9")
    );
}

#[test]
fn sh_rehash_listing_help_and_completions() {
    let f = Fixture::new();
    let rehash = |shell| run(&f, &["sh-rehash"], &[("PYENV_SHELL", shell)]).0;
    assert_eq!(
        rehash("pwsh"),
        "& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash\r\n"
    );
    assert_eq!(
        rehash("bash"),
        "command pyenv rehash\nhash -r 2>/dev/null || true\n"
    );
    assert_eq!(rehash("fish"), "command pyenv rehash\n");
    assert_eq!(rehash("cmd"), "pyenv rehash\r\n");
    let commands = run(&f, &["commands"], &[]).0;
    assert_eq!(commands.lines().filter(|l| *l == "shell").count(), 1);
    assert!(!commands.contains("sh-"));
    assert_eq!(
        run(&f, &["sh-shell", "--help"], &[]).0,
        "pyenv help \"sh-shell\"\r\n"
    );
    assert_eq!(run(&f, &["help", "sh-shell"], &[]).0, HELP);
    assert_eq!(
        run(&f, &["completions", "shell"], &[]).0,
        "--help\r\n--unset\r\n"
    );
}

/// Final review #1: a typographic quote in a version name can neither end the PowerShell
/// literal nor inject code.
#[test]
fn powershell_value_with_a_typographic_quote_cannot_inject() {
    let f = Fixture::new();
    let name = "x\u{2019};Write-Output INJECTED;\u{2019}";
    f.version(name);
    winshell::install_pyenv(&f);
    let units: Vec<String> = name.encode_utf16().map(|u| u.to_string()).collect();
    let script = format!(
        "$n = [string]::new([char[]]@({})); iex ((pyenv sh-shell $n) -join \"`n\"); $env:PYENV_VERSION -eq $n",
        units.join(",")
    );
    let mut c = host(&f, &winshell::powershell(), &[("PYENV_SHELL", "pwsh")], &[]);
    c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    let (out, err, _) = output(c);
    assert_eq!(out, "True\r\n", "{err}");
}

/// allowlist D-90
/// Someone switching from pyenv-win without the profile line: a real PowerShell is
/// detected as the parent, so `pyenv shell` prints PowerShell's command and names the
/// setup, and exits 1 without changing anything.
#[test]
fn powershell_without_the_profile_line_prints_the_command_and_the_setup() {
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let mut c = host(&f, &winshell::powershell(), &[], &[]);
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "pyenv shell 3.7.7; \"rc=$LASTEXITCODE\"; \"set=[$env:PYENV_VERSION]\"",
    ]);
    let (out, err, _) = output(c);
    assert_eq!(
        out, "$Env:PYENV_VERSION = '3.7.7'\r\nrc=1\r\nset=[]\r\n",
        "{err}"
    );
    assert!(
        err.contains("to enable it, see `pyenv init powershell`."),
        "{err}"
    );
}
