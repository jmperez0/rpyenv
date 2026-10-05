//! Windows `pyenv init` (spec §7; Decisions 2, 7, 8, 10; allowlist D-91).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

/// stdout as the UTF-8 bytes rpyenv writes for bash and fish (the Fixture decodes the
/// console code page, which is right only for cmd and PowerShell output).
fn raw(f: &Fixture, args: &[&str]) -> String {
    let o = f
        .command(
            std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[],
        )
        .args(args)
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

/// `C:\x\y` → `/c/x/y` (Decision 8).
fn msys(p: &std::path::Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    format!("/{}{}", s[..1].to_ascii_lowercase(), &s[2..])
}

const FUNCTION: &str = "function pyenv {\n  $command = ''\n  $rest = @()\n  if ($args.Count -gt 0) { $command = $args[0]; $rest = @($args | Select-Object -Skip 1) }\n  $pyenv = Get-Command -CommandType Application pyenv -TotalCount 1\n  if ($command -eq 'shell' -and $rest.Count -gt 0 -and $rest[0] -ne '--help') {\n    $shell_cmds = & $pyenv sh-shell @rest\n    if ($LASTEXITCODE -ne 0) { $shell_cmds; return }\n    if ($shell_cmds) { Invoke-Expression ($shell_cmds -join \"`n\") }\n  } elseif ($command -eq '') {\n    & $pyenv\n  } else {\n    & $pyenv $command @rest\n  }\n}\n";

#[test]
fn print_mode_for_powershell() {
    let f = Fixture::new();
    let s = f.root.join("shims").display().to_string();
    // The fixture's root has a `ñ`: PowerShell literals stay ASCII (final review #2).
    let lit = |t: &str| {
        let (a, b) = t.split_once('\u{f1}').unwrap();
        format!("('{a}' + [char]0x00F1 + '{b}')")
    };
    let (q, qs, qsemi) = (lit(&s), lit(&format!("{s}\\")), lit(&format!("{s};")));
    let want = format!("$Env:PATH = (($Env:PATH -split ';') | Where-Object {{ $_ -ne {q} -and $_ -ne {qs} }}) -join ';'\n$Env:PATH = {qsemi} + $Env:PATH\n$Env:PYENV_SHELL = 'pwsh'\n{FUNCTION}");
    assert_eq!(
        run(&f, &["init", "-", "pwsh", "--no-rehash"]),
        (want.replace('\n', "\r\n"), String::new(), 0)
    );
    assert!(f.root.join("shims").is_dir() && f.root.join("versions").is_dir());
    let push = run(&f, &["init", "-", "powershell", "--no-push-path"]).0;
    assert!(push.starts_with(&format!("if (-not (($Env:PATH -split ';') -contains {q})) {{ $Env:PATH = {qsemi} + $Env:PATH }}\r\n$Env:PYENV_SHELL = 'powershell'\r\n& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash\r\n")));
}

#[test]
fn print_mode_for_bash_uses_msys_paths_and_lf() {
    let f = Fixture::new();
    let s = msys(&f.root.join("shims"));
    let out = raw(&f, &["init", "-", "bash", "--no-push-path", "--no-rehash"]);
    assert_eq!(
        out,
        format!("if [[ \":$PATH:\" != *:'{s}':* ]]; then\nexport PATH='{s}'\":${{PATH}}\"\nfi\nexport PYENV_SHELL=bash\npyenv() {{\n  local command=${{1:-}}\n  [ \"$#\" -gt 0 ] && shift\n  case \"$command\" in\n  activate|deactivate|rehash|shell)\n    eval \"$(pyenv \"sh-$command\" \"$@\")\"\n    ;;\n  *)\n    command pyenv \"$command\" \"$@\"\n    ;;\n  esac\n}}\n")
    );
    assert!(raw(&f, &["init", "-", "fish"]).contains(&format!("set -gx PATH '{s}' $PATH\n")));
}

#[test]
fn help_and_refusals() {
    let f = Fixture::new();
    let tail = "\r\n# Restart your shell for the changes to take effect.\r\n\r\n";
    assert_eq!(
        run(&f, &["init", "pwsh"]),
        (String::new(), format!("# Load pyenv automatically by appending\r\n# the following to your PowerShell profile ($PROFILE) :\r\n\r\niex ((pyenv init - pwsh) -join \"`n\")\r\n{tail}"), 1)
    );
    assert_eq!(
        run(&f, &["init", "bash"]).1,
        format!("# Load pyenv automatically by appending\r\n# the following to ~/.bashrc :\r\n\r\neval \"$(pyenv init - bash)\"\r\n{tail}")
    );
    assert_eq!(
        run(&f, &["init", "fish"]).1,
        format!("# Load pyenv automatically by appending\r\n# the following to ~/.config/fish/config.fish:\r\n\r\npyenv init - fish | source\r\n{tail}")
    );
    assert_eq!(
        run(&f, &["init", "cmd"]),
        (String::new(), "# cmd has no shell integration. `pyenv shell` prints the `set`\r\n# command to run, and every other command needs no setup.\r\n\r\n".to_string(), 1)
    );
    // Decision 2: no startup-file editing on Windows before M6.
    assert_eq!(
        run(&f, &["init", "--install", "pwsh"]),
        (
            String::new(),
            "pyenv: cannot automatically configure startup files for pwsh\r\n".to_string(),
            1
        )
    );
    assert_eq!(run(&f, &["init", "-", "cmd"]).2, 1);
    // Run by this test, no shell is detected.
    assert_eq!(
        run(&f, &["init"]).1,
        "# pyenv can't tell which shell runs it. Name one: `pyenv init <shell>`,\r\n# where <shell> is pwsh, powershell, bash, zsh, fish or cmd.\r\n\r\n"
    );
}

#[test]
fn detect_shell_from_cmd_and_powershell() {
    let f = Fixture::new();
    winshell::install_pyenv(&f);
    let mut c = host(&f, &winshell::cmd_exe(), &[], &[]);
    c.args(["/d", "/c", "call", "pyenv", "init", "--detect-shell"]);
    assert_eq!(
        output(c).0,
        "PYENV_SHELL_DETECT=cmd\r\nPYENV_PROFILE_DETECT=\r\nPYENV_RC_DETECT=\r\n"
    );
    let mut c = host(&f, &winshell::powershell(), &[], &[]);
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "pyenv init --detect-shell",
    ]);
    assert_eq!(output(c).0, "PYENV_SHELL_DETECT=powershell\r\nPYENV_PROFILE_DETECT=$PROFILE\r\nPYENV_RC_DETECT=$PROFILE\r\n");
}

/// allowlist D-90
/// Review focus 2: the fixture's root (a space and `ñ`) reaches PowerShell's PATH
/// unchanged, and the function sets, shows and unsets the version, and reports a failure.
#[test]
fn powershell_runs_init_and_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let shims = f
        .root
        .join("shims")
        .display()
        .to_string()
        .replace('\'', "''");
    let script = format!("iex ((pyenv init - powershell --no-rehash) -join \"`n\"); pyenv shell 3.7.7; $env:PYENV_VERSION; (($env:PATH -split ';')[0] -eq '{shims}'); pyenv shell; pyenv shell --unset; [string]::IsNullOrEmpty($env:PYENV_VERSION); pyenv shell 9.9.9; $LASTEXITCODE");
    let mut shells = vec![winshell::powershell()];
    shells.extend(winshell::pwsh());
    for sh in shells {
        let mut c = host(&f, &sh, &[], &[]);
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let (out, err, _) = output(c);
        assert_eq!(
            out,
            "3.7.7\r\nTrue\r\n3.7.7\r\nTrue\r\npyenv specific python requisite didn't meet. Project is using different version of python.\r\nInstall python '9.9.9' by typing: 'pyenv install 9.9.9'\r\n1\r\n",
            "{}: {err}",
            sh.display()
        );
    }
}

/// Git Bash, when installed: the bash code with MSYS paths runs and routes `shell`.
#[test]
fn git_bash_runs_init_and_shell() {
    let Some(bash) = winshell::git_bash() else {
        eprintln!("skipped: no Git Bash");
        return;
    };
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let usr_bin = std::path::PathBuf::from(r"C:\Program Files\Git\usr\bin");
    let mut c = host(&f, &bash, &[], &[&usr_bin]);
    c.args(["-c", "eval \"$(pyenv init - bash --no-rehash)\"; pyenv shell 3.7.7; echo \"$PYENV_VERSION\"; type -t pyenv; pyenv shell 9.9.9; echo \"rc=$?\""]);
    let (out, err, _) = output(c);
    assert_eq!(out, "3.7.7\nfunction\nrc=1\n", "{err}");
}

/// `s` as a PowerShell expression built from UTF-16 code units, so a test script can name
/// any path without quoting it.
fn ps_units(s: &str) -> String {
    let units: Vec<String> = s.encode_utf16().map(|u| u.to_string()).collect();
    format!("[string]::new([char[]]@({}))", units.join(","))
}

/// Final review #1, #2: a root with a letter outside the console code page and a
/// typographic quote reaches PowerShell's PATH unchanged.
#[test]
fn powershell_init_keeps_any_letter_and_typographic_quotes() {
    let f = Fixture::new();
    winshell::install_pyenv(&f);
    let root = f.base.join("\u{141}ukasz\u{2019}s root");
    let shims = root.join("shims").display().to_string();
    let r = root.display().to_string();
    let script = format!(
        "iex ((pyenv init - powershell --no-rehash) -join \"`n\"); ($env:PATH -split ';')[0] -eq {}",
        ps_units(&shims)
    );
    let mut c = host(
        &f,
        &winshell::powershell(),
        &[("PYENV_ROOT", r.as_str())],
        &[],
    );
    c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    let (out, err, _) = output(c);
    assert_eq!(out, "True\r\n", "{err}");
}

/// Final review #3: a root with an apostrophe still gives working bash code, and a second
/// `init` leaves one shims entry.
#[test]
fn git_bash_init_with_an_apostrophe_in_the_root() {
    let Some(bash) = winshell::git_bash() else {
        eprintln!("skipped: no Git Bash");
        return;
    };
    let f = Fixture::new();
    winshell::install_pyenv(&f);
    let root = f.base.join("O'Brien");
    let r = root.display().to_string();
    let want = msys(&root.join("shims"));
    let usr_bin = std::path::PathBuf::from(r"C:\Program Files\Git\usr\bin");
    let mut c = host(
        &f,
        &bash,
        &[("PYENV_ROOT", r.as_str()), ("WANT", want.as_str())],
        &[&usr_bin],
    );
    c.args(["-c", "eval \"$(pyenv init - bash --no-rehash)\"; eval \"$(pyenv init - bash --no-rehash)\"; type -t pyenv; [ \"${PATH%%:*}\" = \"$WANT\" ] && echo first; tr ':' '\\n' <<<\"$PATH\" | grep -cxF \"$WANT\""]);
    let (out, err, _) = output(c);
    assert_eq!(out, "function\nfirst\n1\n", "{err}");
    let o = f
        .command(
            std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[("PYENV_ROOT", r.as_str())],
        )
        .args(["init", "-", "fish", "--no-rehash"])
        .output()
        .unwrap();
    let fish = String::from_utf8(o.stdout).unwrap();
    let quoted = want.replace('\'', "\\'");
    assert!(
        fish.contains(&format!("set -gx PATH '{quoted}' $PATH")),
        "{fish}"
    );
}

/// Final review #4: in an integrated Git Bash, completion candidates carry no `\r`.
#[test]
fn git_bash_completion_has_no_carriage_returns() {
    let Some(bash) = winshell::git_bash() else {
        eprintln!("skipped: no Git Bash");
        return;
    };
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../completions/pyenv.bash");
    let script = msys(&script);
    let usr_bin = std::path::PathBuf::from(r"C:\Program Files\Git\usr\bin");
    let mut c = host(
        &f,
        &bash,
        &[("PYENV_SHELL", "bash"), ("SCRIPT", script.as_str())],
        &[&usr_bin],
    );
    c.args(["-c", "source \"$SCRIPT\"; COMP_WORDS=(pyenv sh); COMP_CWORD=1; _pyenv; printf '%s|' \"${COMPREPLY[@]}\"; echo; COMP_WORDS=(pyenv shell ''); COMP_CWORD=2; _pyenv; printf '%s|' \"${COMPREPLY[@]}\""]);
    let (out, err, _) = output(c);
    assert_eq!(out, "shell|shims|\n--help|--unset|3.7.7|", "{err}");
}
