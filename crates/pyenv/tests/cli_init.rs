//! `pyenv init` on Linux: help, --detect-shell, print and path modes (M3L "pyenv init").
#![cfg(unix)]

mod common;
use common::Fixture;
use std::path::Path;

fn init(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut a = vec!["init"];
    a.extend_from_slice(args);
    let r = f.pyenv_env(&a, env);
    (r.stdout, r.stderr, r.code)
}

fn root(f: &Fixture) -> String {
    f.root.display().to_string()
}

#[test]
fn detect_shell_reports_profile_and_rc() {
    let f = Fixture::new();
    let detect = |shell| init(&f, &["--detect-shell", shell], &[]).0;
    assert_eq!(
        detect("bash"),
        "PYENV_SHELL_DETECT=bash\nPYENV_PROFILE_DETECT=~/.profile\nPYENV_RC_DETECT=~/.bashrc\n"
    );
    assert_eq!(
        detect("zsh"),
        "PYENV_SHELL_DETECT=zsh\nPYENV_PROFILE_DETECT=~/.zprofile\nPYENV_RC_DETECT=~/.zshrc\n"
    );
    assert_eq!(
        detect("fish"),
        "PYENV_SHELL_DETECT=fish\nPYENV_PROFILE_DETECT=~/.config/fish/config.fish\nPYENV_RC_DETECT=~/.config/fish/config.fish\n"
    );
    assert_eq!(
        detect("pwsh"),
        "PYENV_SHELL_DETECT=pwsh\nPYENV_PROFILE_DETECT=~/.config/powershell/profile.ps1\nPYENV_RC_DETECT=~/.config/powershell/profile.ps1\n"
    );
    assert_eq!(
        detect("mksh"),
        "PYENV_SHELL_DETECT=mksh\nPYENV_PROFILE_DETECT=~/.profile\nPYENV_RC_DETECT=~/.profile\n"
    );
    assert_eq!(
        detect("nu"),
        "PYENV_SHELL_DETECT=nu\nPYENV_PROFILE_DETECT=\nPYENV_RC_DETECT=\n"
    );
    f.file(&f.base.join(".bash_profile"), "");
    assert!(detect("bash").contains("PYENV_PROFILE_DETECT=~/.bash_profile\n"));
    // The last mode flag and the last other argument win.
    assert_eq!(
        init(&f, &["--detect-shell", "zsh", "fish"], &[])
            .0
            .lines()
            .next(),
        Some("PYENV_SHELL_DETECT=fish")
    );
}

const TAIL: &str = "\n# Restart your shell for the changes to take effect.\n\n";

#[test]
fn help_for_each_family() {
    let f = Fixture::new();
    let r = root(&f);
    let bash = init(&f, &["bash"], &[]);
    assert_eq!(
        bash,
        (
            String::new(),
            format!("# Load pyenv automatically by appending\n# the following to \n# ~/.bash_profile if it exists, otherwise ~/.profile (for login shells)\n# and ~/.bashrc (for interactive shells) :\n\nexport PYENV_ROOT=\"{r}\"\n[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"\neval \"$(pyenv init - bash)\"\n{TAIL}"),
            1
        )
    );
    assert!(init(&f, &["zsh"], &[])
        .1
        .contains("# ~/.zprofile (for login shells)\n# and ~/.zshrc (for interactive shells) :\n"));
    assert_eq!(
        init(&f, &["ksh"], &[]).1,
        format!("# Load pyenv automatically by appending\n# the following to ~/.profile :\n\nexport PYENV_ROOT=\"{r}\"\n[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"\neval \"$(pyenv init - ksh)\"\n{TAIL}")
    );
    assert!(init(&f, &["sh"], &[]).1.contains("# your shell's login startup file (for login shells)\n# and your shell's interactive startup file (for interactive shells) :\n"));
    // The default root keeps the portable `$HOME` form.
    let default = [("PYENV_ROOT", f.base.join(".pyenv").display().to_string())];
    let default: Vec<(&str, &str)> = default.iter().map(|(k, v)| (*k, v.as_str())).collect();
    assert!(init(&f, &["bash"], &default)
        .1
        .contains("\nexport PYENV_ROOT=\"$HOME/.pyenv\"\n"));
    assert_eq!(
        init(&f, &["fish"], &default).1,
        format!("# Add pyenv executable to PATH by running\n# the following interactively:\n\nset -Ux PYENV_ROOT $HOME/.pyenv\nif functions -q fish_add_path\n  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin\nelse\n  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths\nend\n\n# Load pyenv automatically by appending\n# the following to ~/.config/fish/config.fish:\n\npyenv init - fish | source\n\n{TAIL}")
    );
    assert_eq!(
        init(&f, &["pwsh"], &default).1,
        format!("# Load pyenv automatically by appending\n# the following to ~/.config/powershell/profile.ps1 :\n\n$Env:PYENV_ROOT=\"$Env:HOME/.pyenv\"\nif (Test-Path -LP \"$Env:PYENV_ROOT/bin\" -PathType Container) {{\n  $Env:PATH=\"$Env:PYENV_ROOT/bin:$Env:PATH\" }}\niex ((pyenv init -) -join \"`n\")\n{TAIL}")
    );
}

/// test/init.bats:161: a custom root is quoted for each shell.
#[test]
fn custom_root_is_quoted_per_shell() {
    let f = Fixture::new();
    let odd = format!("{}/a$b\"c`d\\e", f.base.display());
    let env = [("PYENV_ROOT", odd.as_str())];
    let b = f.base.display();
    assert!(init(&f, &["bash"], &env).1.contains(&format!(
        "\nexport PYENV_ROOT=\"{b}/a\\$b\\\"c\\`d\\\\e\"\n"
    )));
    assert!(init(&f, &["fish"], &env)
        .1
        .contains(&format!("\nset -Ux PYENV_ROOT \"{b}/a\\$b\\\"c`d\\\\e\"\n")));
    let quote = format!("{}/it's", f.base.display());
    assert!(init(&f, &["pwsh"], &[("PYENV_ROOT", quote.as_str())])
        .1
        .contains(&format!("\n$Env:PYENV_ROOT='{b}/it''s'\n")));
}

#[test]
fn print_mode_for_bash() {
    let f = Fixture::new();
    let r = root(&f);
    let (out, err, code) = init(&f, &["-", "bash"], &[]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert_eq!(
        out,
        format!(
            r#"PATH="$(bash --norc -ec 'IFS=:; paths=($PATH); 
for i in ${{!paths[@]}}; do 
if [[ ${{paths[i]}} == "''{r}/shims''" ]]; then unset '\''paths[i]'\''; 
fi; done; 
echo "${{paths[*]}}"')"
export PATH="{r}/shims:${{PATH}}"
export PYENV_SHELL=bash
command pyenv rehash
pyenv() {{
  local command=${{1:-}}
  [ "$#" -gt 0 ] && shift
  case "$command" in
  rehash|shell)
    eval "$(pyenv "sh-$command" "$@")"
    ;;
  *)
    command pyenv "$command" "$@"
    ;;
  esac
}}
"#
        )
    );
    assert!(f.root.join("shims").is_dir() && f.root.join("versions").is_dir());
}

#[test]
fn print_mode_for_fish_and_pwsh() {
    let f = Fixture::new();
    let r = root(&f);
    assert_eq!(
        init(&f, &["-", "fish", "--no-rehash"], &[]).0,
        format!("while set pyenv_index (contains -i -- \"{r}/shims\" $PATH)\nset -eg PATH[$pyenv_index]; end; set -e pyenv_index\nset -gx PATH '{r}/shims' $PATH\nset -gx PYENV_SHELL fish\nfunction pyenv\n  set command $argv[1]\n  set -e argv[1]\n\n  switch \"$command\"\n  case rehash shell\n    source (pyenv \"sh-$command\" $argv|psub)\n  case \"*\"\n    command pyenv \"$command\" $argv\n  end\nend\n")
    );
    assert_eq!(
        init(&f, &["-", "pwsh"], &[]).0,
        format!("$Env:PATH=\"$(($Env:PATH -split ':' | where {{ -not ($_ -match '{r}/shims') }}) -join ':')\"\n$Env:PATH=\"{r}/shims:$Env:PATH\"\n$Env:PYENV_SHELL=\"pwsh\"\n& pyenv rehash\nfunction pyenv {{\n  $command=\"\"\n  if ( $args.Count -gt 0 ) {{\n    $command, $args = $args\n  }}\n\n  if ( (\"rehash shell\" -split ' ') -contains $command ) {{\n    $shell_cmds = (& (get-command -commandtype application pyenv -totalcount 1) sh-$command $args)\n    if ( $shell_cmds.Count -gt 0 ) {{\n      iex ($shell_cmds -join \"`n\")\n    }}\n  }} else {{\n    & (get-command -commandtype application pyenv -totalcount 1) $command $args\n  }}\n}}\n")
    );
}

#[test]
fn ksh_header_no_push_path_and_path_mode() {
    let f = Fixture::new();
    let r = root(&f);
    let ksh = init(&f, &["-", "ksh", "--no-push-path", "--no-rehash"], &[]).0;
    assert!(ksh.starts_with(&format!("if [[ \":$PATH:\" != *':{r}/shims:'* ]]; then\nexport PATH=\"{r}/shims:${{PATH}}\"\nfi\nexport PYENV_SHELL=ksh\nfunction pyenv {{\n  typeset command=${{1:-}}\n")));
    assert!(init(&f, &["-", "fish", "--no-push-path"], &[])
        .0
        .starts_with(&format!(
            "if not contains -- \"{r}/shims\" $PATH\nset -gx PATH '{r}/shims' $PATH\nend\n"
        )));
    assert!(init(&f, &["-", "pwsh", "--no-push-path"], &[])
        .0
        .starts_with(&format!(
            "if ( $Env:PATH -notmatch \"{r}/shims\" ) {{\n$Env:PATH=\"{r}/shims:$Env:PATH\"\n}}\n"
        )));
    // --path: PATH lines and the rehash only, and no directories are created.
    let fresh = f.base.join("fresh");
    let fresh_s = fresh.display().to_string();
    let (out, _, code) = init(&f, &["--path", "sh"], &[("PYENV_ROOT", fresh_s.as_str())]);
    assert_eq!(code, 0);
    assert!(out.ends_with(&format!(
        "export PATH=\"{fresh_s}/shims:${{PATH}}\"\ncommand pyenv rehash\n"
    )));
    assert!(!fresh.exists());
}

/// The completion line names `<prefix>/completions/pyenv.<shell>` when that file is
/// readable; the prefix is the parent of the binary's folder (Decision 3).
#[test]
fn completion_line_next_to_an_installed_binary() {
    let f = Fixture::new();
    let inst = f.base.join("inst");
    std::fs::create_dir_all(inst.join("bin")).unwrap();
    let exe = inst.join("bin").join("pyenv");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &exe).unwrap();
    for s in ["bash", "pwsh"] {
        f.file(&inst.join("completions").join(format!("pyenv.{s}")), "");
    }
    let run = |shell: &str| {
        let o = f
            .command(&exe, &f.work, &[])
            .args(["init", "-", shell])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let i = inst.display();
    assert!(run("bash").contains(&format!("\nsource '{i}/completions/pyenv.bash'\n")));
    assert!(run("pwsh").contains(&format!("\niex (gc {i}/completions/pyenv.pwsh -Raw)\n")));
    assert!(!run("zsh").contains("completions"));
}

#[test]
fn mkdir_failure_prints_mkdirs_messages() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return; // root can create the directory anyway
    }
    let f = Fixture::new();
    let ro = f.base.join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = ro.join("root").display().to_string();
    let got = init(&f, &["-", "bash"], &[("PYENV_ROOT", r.as_str())]);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    let msg = format!("mkdir: cannot create directory \u{2018}{r}\u{2019}: Permission denied\n");
    assert_eq!(got, (String::new(), format!("{msg}{msg}"), 1));
}

/// Help mode detects the parent shell; `"$0" init; true` keeps bash from exec'ing pyenv.
#[test]
fn help_mode_detects_the_parent_shell() {
    let f = Fixture::new();
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[])
        .args([
            "-c",
            "\"$0\" init; echo \"rc=$?\"; \"$0\" init --detect-shell; true",
            env!("CARGO_BIN_EXE_pyenv"),
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(String::from_utf8_lossy(&o.stderr).contains("eval \"$(pyenv init - bash)\""));
    assert!(out.starts_with("rc=1\nPYENV_SHELL_DETECT=bash\n"), "{out}");
}

/// The printed code, evaluated by a real bash: shims end up first, once, and the function
/// routes `shell` through `sh-shell`.
#[test]
fn bash_evaluates_init_and_shell() {
    let f = Fixture::new();
    f.version("3.12.1");
    for (name, target) in [
        ("pyenv", env!("CARGO_BIN_EXE_pyenv")),
        ("bash", "/bin/bash"),
    ] {
        std::os::unix::fs::symlink(target, f.syspath.join(name)).unwrap();
    }
    let shims = f.root.join("shims").display().to_string();
    let sys = f.syspath.display().to_string();
    let path = format!("{shims}:{sys}:{shims}");
    let script = r#"eval "$(pyenv init - bash --no-rehash)"
pyenv shell 3.12.1
echo "[$PYENV_VERSION]"
pyenv shell
echo "$PATH"
type -t pyenv
"#;
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[("PATH", path.as_str())])
        .args(["-c", script])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        format!("[3.12.1]\n3.12.1\n{shims}:{sys}\nfunction\n"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
