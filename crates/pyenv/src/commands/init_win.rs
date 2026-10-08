//! `pyenv init` on Windows (spec §7; allowlist D-91, since pyenv-win has none). PowerShell
//! gets rpyenv's own code (Decision 7); bash, zsh and fish get upstream's code with MSYS
//! paths (Decision 8); cmd gets no integration (Decision 10).

use crate::commands::init::{self, Mode};
use crate::commands::shell_win::lf;
use crate::commands::{self, Listing};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname::{self, Family};
use std::path::Path;

/// The function the profile line defines. `$rest` is always an array: after
/// `$command, $args = $args` a single remaining argument is a string, and splatting a
/// string passes its characters one by one. It routes `shell` only with arguments, and not
/// for `--help`, as pyenv-win's `pyenv.ps1` does; a failure's message, printed by
/// `sh-shell` on stdout, is shown as is (Decision 7).
const PWSH_FUNCTION: &str = r#"function pyenv {
  $command = ''
  $rest = @()
  if ($args.Count -gt 0) { $command = $args[0]; $rest = @($args | Select-Object -Skip 1) }
  $pyenv = Get-Command -CommandType Application pyenv -TotalCount 1
  if (($command -eq 'shell' -and $rest.Count -gt 0 -and $rest[0] -ne '--help') -or $command -eq 'activate' -or $command -eq 'deactivate') {
    $shell_cmds = & $pyenv "sh-$command" @rest
    if ($LASTEXITCODE -ne 0) { $shell_cmds; return }
    if ($shell_cmds) { Invoke-Expression ($shell_cmds -join "`n") }
  } elseif ($command -eq '') {
    & $pyenv
  } else {
    & $pyenv $command @rest
  }
}
"#;

const RESTART: &str = "# Restart your shell for the changes to take effect.";

pub fn init(ctx: &Ctx, args: &[&str]) -> Output {
    let a = init::parse(args);
    let shell = a
        .shell
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(detected);
    let family = shellname::family(&shell, Flavor::PyenvWin);
    match a.mode {
        Mode::Help => help(&shell, family),
        Mode::DetectShell => detect(&shell, family),
        // Plan M6a, R2: PowerShell gets the profile line; other shells have no startup
        // file rpyenv edits.
        Mode::Install if family == Family::Pwsh => commands::pwsh_profile::install_all(),
        Mode::Install => Output::error(format!(
            "pyenv: cannot automatically configure startup files for {shell}"
        )),
        Mode::Path | Mode::Print if family == Family::Cmd => Output::error(
            "pyenv: cmd has no shell integration; `pyenv init cmd` explains what works without it",
        ),
        Mode::Path => {
            let mut o = Output::new();
            path_lines(&mut o, ctx, family, a.no_push_path);
            rehash_line(&mut o, family, a.no_rehash);
            finish(o, family)
        }
        Mode::Print => {
            if let Err(o) = init::init_dirs(ctx) {
                return o;
            }
            let mut o = Output::new();
            path_lines(&mut o, ctx, family, a.no_push_path);
            env_line(&mut o, &shell, family);
            completion_line(&mut o, &shell, family);
            rehash_line(&mut o, family, a.no_rehash);
            o.stdout.push_str(&function(ctx, family));
            finish(o, family)
        }
    }
}

/// The parent process, when it is a supported shell; empty otherwise.
fn detected() -> String {
    #[cfg(windows)]
    let parent = rpyenv_core::winproc::parent_image_name();
    #[cfg(not(windows))]
    let parent: Option<String> = None;
    parent
        .as_deref()
        .and_then(shellname::windows_name)
        .unwrap_or_default()
}

/// PowerShell code keeps the console's line ends and code page; bash, zsh and fish code
/// goes out as `\n` and UTF-8 (Task 6, `lf`).
fn finish(o: Output, family: Family) -> Output {
    if family == Family::Pwsh {
        o
    } else {
        lf(o)
    }
}

/// `C:\x\y` → `/c/x/y`, `\\server\share` → `//server/share` (Decision 8).
pub(crate) fn msys(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &s[2..])
    } else {
        s
    }
}

/// A PowerShell literal in ASCII only (final review #1, #2): see `shell_win::ps_literal`.
fn sq(s: &str) -> String {
    crate::commands::shell_win::ps_literal(s)
}

/// A bash or zsh single-quoted word.
fn sh_word(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A fish single-quoted word.
fn fish_word(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The POSIX PATH code with the shims path quoted as one word (final review #3). Upstream's
/// Linux form pastes the path into a nested single-quoted script, which a `'` in a Windows
/// profile name breaks; here the nested `bash` gets it as `$1`.
fn posix_path_lines_win(shims: &str, no_push_path: bool) -> Vec<String> {
    let w = sh_word(shims);
    let prepend = format!("export PATH={w}\":${{PATH}}\"");
    if no_push_path {
        return vec![
            format!("if [[ \":$PATH:\" != *:{w}:* ]]; then"),
            prepend,
            "fi".to_string(),
        ];
    }
    vec![
        format!("PATH=\"$(bash --norc -ec 'IFS=:; paths=($PATH); for i in ${{!paths[@]}}; do if [[ ${{paths[i]}} == \"$1\" ]]; then unset '\\''paths[i]'\\''; fi; done; echo \"${{paths[*]}}\"' bash {w})\""),
        prepend,
    ]
}

fn fish_path_lines_win(shims: &str, no_push_path: bool) -> Vec<String> {
    let w = fish_word(shims);
    let prepend = format!("set -gx PATH {w} $PATH");
    if no_push_path {
        return vec![
            format!("if not contains -- {w} $PATH"),
            prepend,
            "end".to_string(),
        ];
    }
    vec![
        format!("while set pyenv_index (contains -i -- {w} $PATH)"),
        "set -eg PATH[$pyenv_index]; end; set -e pyenv_index".to_string(),
        prepend,
    ]
}

fn path_lines(o: &mut Output, ctx: &Ctx, family: Family, no_push_path: bool) {
    let shims = ctx.shims_dir();
    let lines = match family {
        // `-ne` and `-contains` compare without case, as Windows paths do.
        Family::Pwsh => {
            let s = shims.display().to_string();
            if no_push_path {
                vec![format!(
                    "if (-not (($Env:PATH -split ';') -contains {})) {{ $Env:PATH = {} + $Env:PATH }}",
                    sq(&s),
                    sq(&format!("{s};"))
                )]
            } else {
                vec![
                    format!(
                        "$Env:PATH = (($Env:PATH -split ';') | Where-Object {{ $_ -ne {} -and $_ -ne {} }}) -join ';'",
                        sq(&s),
                        sq(&format!("{s}\\"))
                    ),
                    format!("$Env:PATH = {} + $Env:PATH", sq(&format!("{s};"))),
                ]
            }
        }
        Family::Fish => fish_path_lines_win(&msys(&shims), no_push_path),
        _ => posix_path_lines_win(&msys(&shims), no_push_path),
    };
    for l in lines {
        o.out(l);
    }
}

fn env_line(o: &mut Output, shell: &str, family: Family) {
    o.out(match family {
        Family::Pwsh => format!("$Env:PYENV_SHELL = {}", sq(shell)),
        Family::Fish => format!("set -gx PYENV_SHELL {shell}"),
        _ => format!("export PYENV_SHELL={shell}"),
    });
}

/// `<prefix>\completions\pyenv.<shell>` when readable; Windows PowerShell 5.1 uses the
/// pwsh script. `iex (Get-Content …)` runs it whatever the execution policy says.
fn completion_line(o: &mut Output, shell: &str, family: Family) {
    let Some(prefix) = crate::install_prefix() else {
        return;
    };
    let script = if family == Family::Pwsh {
        "pwsh"
    } else {
        shell
    };
    let path = prefix.join("completions").join(format!("pyenv.{script}"));
    if std::fs::File::open(&path).is_err() {
        return;
    }
    o.out(match family {
        Family::Pwsh => format!("iex (Get-Content {} -Raw)", sq(&path.display().to_string())),
        Family::Fish => format!("source {}", fish_word(&msys(&path))),
        _ => format!("source {}", sh_word(&msys(&path))),
    });
}

fn rehash_line(o: &mut Output, family: Family, no_rehash: bool) {
    if !no_rehash {
        o.out(if family == Family::Pwsh {
            "& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash"
        } else {
            "command pyenv rehash"
        });
    }
}

fn function(ctx: &Ctx, family: Family) -> String {
    let names = commands::command_names(ctx, Listing::ShOnly);
    let routed: Vec<&str> = names.iter().map(String::as_str).collect();
    match family {
        Family::Pwsh => PWSH_FUNCTION.to_string(),
        Family::Fish => init::fish_function(&routed),
        Family::Ksh => {
            init::posix_function("function pyenv {\n  typeset command=${1:-}\n", &routed)
        }
        _ => init::posix_function("pyenv() {\n  local command=${1:-}\n", &routed),
    }
}

fn help(shell: &str, family: Family) -> Output {
    let lines: Vec<String> = if shell.is_empty() {
        vec![
            "# pyenv can't tell which shell runs it. Name one: `pyenv init <shell>`,".into(),
            "# where <shell> is pwsh, powershell, bash, zsh, fish or cmd.".into(),
            String::new(),
        ]
    } else {
        match family {
            Family::Cmd => vec![
                "# cmd has no shell integration. `pyenv shell` prints the `set`".into(),
                "# command to run, and every other command needs no setup.".into(),
                String::new(),
            ],
            Family::Pwsh => vec![
                "# Load pyenv automatically by appending".into(),
                "# the following to your PowerShell profile ($PROFILE) :".into(),
                String::new(),
                format!("iex ((pyenv init - {shell}) -join \"`n\")"),
                String::new(),
                RESTART.into(),
                String::new(),
            ],
            Family::Fish => vec![
                "# Load pyenv automatically by appending".into(),
                "# the following to ~/.config/fish/config.fish:".into(),
                String::new(),
                "pyenv init - fish | source".into(),
                String::new(),
                RESTART.into(),
                String::new(),
            ],
            _ => {
                let rc = match shell {
                    "bash" => "~/.bashrc",
                    "zsh" => "~/.zshrc",
                    _ => "your shell's interactive startup file",
                };
                vec![
                    "# Load pyenv automatically by appending".into(),
                    format!("# the following to {rc} :"),
                    String::new(),
                    format!("eval \"$(pyenv init - {shell})\""),
                    String::new(),
                    RESTART.into(),
                    String::new(),
                ]
            }
        }
    };
    let mut o = Output::new();
    for l in lines {
        o.err(l);
    }
    o.with_code(1)
}

fn detect(shell: &str, family: Family) -> Output {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let (profile, rc) = match (family, shell) {
        (Family::Pwsh, _) => ("$PROFILE", "$PROFILE"),
        (Family::Fish, _) => ("~/.config/fish/config.fish", "~/.config/fish/config.fish"),
        (Family::Ksh, _) => ("~/.profile", "~/.profile"),
        (_, "bash") if Path::new(&home).join(".bash_profile").exists() => {
            ("~/.bash_profile", "~/.bashrc")
        }
        (_, "bash") => ("~/.profile", "~/.bashrc"),
        (_, "zsh") => ("~/.zprofile", "~/.zshrc"),
        _ => ("", ""),
    };
    let mut o = Output::new();
    o.out(format!("PYENV_SHELL_DETECT={shell}"));
    o.out(format!("PYENV_PROFILE_DETECT={profile}"));
    o.out(format!("PYENV_RC_DETECT={rc}"));
    o
}
