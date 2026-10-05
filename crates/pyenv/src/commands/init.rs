//! `pyenv init` on Linux (libexec/pyenv-init at 2.8.8; M3L "pyenv init"). Windows' is
//! `init_win.rs`, which reuses the argument loop and the POSIX and fish code below.

use crate::commands::{self, Listing};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::shellname;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Help,
    Print,
    Path,
    DetectShell,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Args {
    pub mode: Mode,
    pub no_push_path: bool,
    pub no_rehash: bool,
    pub shell: Option<String>,
}

/// The argument loop (libexec/pyenv-init:28-53). Every argument is read: the last mode
/// flag wins, and any other argument, unknown options included, is the shell name, the
/// last one winning.
pub(crate) fn parse(args: &[&str]) -> Args {
    let mut a = Args {
        mode: Mode::Help,
        no_push_path: false,
        no_rehash: false,
        shell: None,
    };
    for &arg in args {
        match arg {
            "-" => a.mode = Mode::Print,
            "--path" => a.mode = Mode::Path,
            "--install" => a.mode = Mode::Install,
            "--detect-shell" => a.mode = Mode::DetectShell,
            "--no-push-path" => a.no_push_path = true,
            "--no-rehash" => a.no_rehash = true,
            other => a.shell = Some(other.to_string()),
        }
    }
    a
}

/// The shell argument, or the one detected from the parent's command line
/// (libexec/pyenv-init:56-67).
pub(crate) fn shell_name(given: Option<String>) -> String {
    given.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        #[cfg(unix)]
        let cmdline = shellname::parent_cmdline();
        #[cfg(not(unix))]
        let cmdline = String::new();
        let shell = std::env::var("SHELL").ok();
        shellname::from_parent_cmdline(&cmdline, shell.as_deref())
    })
}

pub fn init(ctx: &Ctx, args: &[&str]) -> Output {
    let a = parse(args);
    let shell = shell_name(a.shell.clone());
    match a.mode {
        Mode::Help => help(ctx, &shell),
        Mode::Install => install(ctx, &shell),
        Mode::DetectShell => {
            let p = detect_profile(&shell);
            let mut o = Output::new();
            o.out(format!("PYENV_SHELL_DETECT={shell}"));
            o.out(format!("PYENV_PROFILE_DETECT={}", p.profile));
            o.out(format!("PYENV_RC_DETECT={}", p.rc));
            o
        }
        Mode::Path => {
            let mut o = Output::new();
            print_path(&mut o, ctx, &shell, a.no_push_path);
            print_rehash(&mut o, &shell, a.no_rehash);
            o
        }
        Mode::Print => {
            if let Err(o) = init_dirs(ctx) {
                return o;
            }
            let mut o = Output::new();
            print_path(&mut o, ctx, &shell, a.no_push_path);
            print_env(&mut o, &shell);
            print_completion(&mut o, &shell);
            print_rehash(&mut o, &shell, a.no_rehash);
            o.stdout.push_str(&shell_function(ctx, &shell));
            o
        }
    }
}

/// `HOME`, as the script sees it (empty when unset).
pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// What `detect_profile` sets (libexec/pyenv-init:103-142). The `~` is literal.
pub(crate) struct Profile {
    pub profile: &'static str,
    pub rc: &'static str,
    pub profile_explain: Option<&'static str>,
    pub rc_explain: Option<&'static str>,
}

pub(crate) fn detect_profile(shell: &str) -> Profile {
    let p = |profile, rc| Profile {
        profile,
        rc,
        profile_explain: None,
        rc_explain: None,
    };
    match shell {
        "bash" => Profile {
            profile: if Path::new(&format!("{}/.bash_profile", home())).exists() {
                "~/.bash_profile"
            } else {
                "~/.profile"
            },
            rc: "~/.bashrc",
            profile_explain: Some("~/.bash_profile if it exists, otherwise ~/.profile"),
            rc_explain: None,
        },
        "fish" => p("~/.config/fish/config.fish", "~/.config/fish/config.fish"),
        "pwsh" => p(
            "~/.config/powershell/profile.ps1",
            "~/.config/powershell/profile.ps1",
        ),
        "zsh" => p("~/.zprofile", "~/.zshrc"),
        "ksh" | "ksh93" | "mksh" => p("~/.profile", "~/.profile"),
        _ => Profile {
            profile: "",
            rc: "",
            profile_explain: Some("your shell's login startup file"),
            rc_explain: Some("your shell's interactive startup file"),
        },
    }
}

fn root(ctx: &Ctx) -> String {
    ctx.root.to_string_lossy().into_owned()
}

/// `[[ ${PYENV_ROOT} == "${HOME}/.pyenv" ]]`, a plain string comparison.
fn root_is_default(ctx: &Ctx) -> bool {
    root(ctx) == format!("{}/.pyenv", home())
}

/// The `PYENV_ROOT` line of the help text and of `--install` (libexec/pyenv-init:197-258).
fn root_line(ctx: &Ctx, family: &str) -> String {
    let r = root(ctx);
    let default = root_is_default(ctx);
    match family {
        "fish" if default => "set -Ux PYENV_ROOT $HOME/.pyenv".to_string(),
        "fish" => format!(
            "set -Ux PYENV_ROOT \"{}\"",
            r.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
        ),
        "pwsh" if default => "$Env:PYENV_ROOT=\"$Env:HOME/.pyenv\"".to_string(),
        "pwsh" => format!("$Env:PYENV_ROOT='{}'", r.replace('\'', "''")),
        _ if default => "export PYENV_ROOT=\"$HOME/.pyenv\"".to_string(),
        _ => format!(
            "export PYENV_ROOT=\"{}\"",
            r.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`")
        ),
    }
}

pub(crate) fn posix_shell_setup(ctx: &Ctx, shell: &str) -> Vec<String> {
    vec![
        root_line(ctx, "posix"),
        "[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"".to_string(),
        format!("eval \"$(pyenv init - {shell})\""),
    ]
}

pub(crate) const FISH_SHELL_SETUP: &str = "pyenv init - fish | source";

pub(crate) fn fish_user_path_setup(ctx: &Ctx) -> Vec<String> {
    vec![
        root_line(ctx, "fish"),
        "if functions -q fish_add_path".to_string(),
        "  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin".to_string(),
        "else".to_string(),
        "  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths"
            .to_string(),
        "end".to_string(),
    ]
}

pub(crate) fn pwsh_shell_setup(ctx: &Ctx) -> Vec<String> {
    vec![
        root_line(ctx, "pwsh"),
        "if (Test-Path -LP \"$Env:PYENV_ROOT/bin\" -PathType Container) {".to_string(),
        "  $Env:PATH=\"$Env:PYENV_ROOT/bin:$Env:PATH\" }".to_string(),
        "iex ((pyenv init -) -join \"`n\")".to_string(),
    ]
}

/// Help mode: everything on stderr, exit 1 (libexec/pyenv-init:150-190).
fn help(ctx: &Ctx, shell: &str) -> Output {
    let p = detect_profile(shell);
    let mut lines: Vec<String> = Vec::new();
    let mut add = |l: &str| lines.push(l.to_string());
    match shell {
        "fish" => {
            add("# Add pyenv executable to PATH by running");
            add("# the following interactively:");
            add("");
            for l in fish_user_path_setup(ctx) {
                add(&l);
            }
            add("");
            add("# Load pyenv automatically by appending");
            add("# the following to ~/.config/fish/config.fish:");
            add("");
            add(FISH_SHELL_SETUP);
            add("");
        }
        "pwsh" => {
            add("# Load pyenv automatically by appending");
            add(&format!("# the following to {} :", p.profile));
            add("");
            for l in pwsh_shell_setup(ctx) {
                add(&l);
            }
        }
        _ => {
            add("# Load pyenv automatically by appending");
            if p.profile == p.rc && p.rc_explain.is_none() {
                add(&format!(
                    "# the following to {} :",
                    p.profile_explain.unwrap_or(p.profile)
                ));
            } else {
                // `echo -n "# the following to "` then a bare `echo`: a trailing space.
                add("# the following to ");
                add(&format!(
                    "# {} (for login shells)",
                    p.profile_explain.unwrap_or(p.profile)
                ));
                add(&format!(
                    "# and {} (for interactive shells) :",
                    p.rc_explain.unwrap_or(p.rc)
                ));
            }
            add("");
            for l in posix_shell_setup(ctx, shell) {
                add(&l);
            }
        }
    }
    add("");
    add("# Restart your shell for the changes to take effect.");
    add("");
    let mut o = Output::new();
    for l in &lines {
        o.err(l);
    }
    o.with_code(1)
}

/// `mkdir -p "${PYENV_ROOT}/"{shims,versions}`: one message per directory that can't be
/// made, naming the first missing ancestor, as `mkdir -p` does; exit 1 with no stdout.
pub(crate) fn init_dirs(ctx: &Ctx) -> Result<(), Output> {
    let mut o = Output::new();
    for d in [ctx.shims_dir(), ctx.versions_dir()] {
        if let Err(e) = std::fs::create_dir_all(&d) {
            o.err(format!(
                "mkdir: cannot create directory \u{2018}{}\u{2019}: {}",
                first_missing(&d).display(),
                rpyenv_core::launch::io_reason(&e)
            ));
        }
    }
    if o.stderr.is_empty() {
        Ok(())
    } else {
        Err(o.with_code(1))
    }
}

/// The first ancestor of `d` (or `d`) that doesn't exist: the one `mkdir -p` fails on.
fn first_missing(d: &Path) -> PathBuf {
    let mut p = d;
    while let Some(parent) = p.parent() {
        if parent.exists() {
            return p.to_path_buf();
        }
        p = parent;
    }
    d.to_path_buf()
}

/// The POSIX PATH code: remove every exact shims entry with a nested `bash --norc`, then
/// prepend one; or, with `--no-push-path`, prepend only when absent
/// (libexec/pyenv-init:412-475). `shims` is pasted in without escaping, as upstream does.
pub(crate) fn posix_path_lines(shims: &str, no_push_path: bool) -> Vec<String> {
    let prepend = format!("export PATH=\"{shims}:${{PATH}}\"");
    if no_push_path {
        return vec![
            format!("if [[ \":$PATH:\" != *':{shims}:'* ]]; then"),
            prepend,
            "fi".to_string(),
        ];
    }
    vec![
        "PATH=\"$(bash --norc -ec 'IFS=:; paths=($PATH); ".to_string(),
        "for i in ${!paths[@]}; do ".to_string(),
        format!("if [[ ${{paths[i]}} == \"''{shims}''\" ]]; then unset '\\''paths[i]'\\''; "),
        "fi; done; ".to_string(),
        "echo \"${paths[*]}\"')\"".to_string(),
        prepend,
    ]
}

pub(crate) fn fish_path_lines(shims: &str, no_push_path: bool) -> Vec<String> {
    let prepend = format!("set -gx PATH '{shims}' $PATH");
    if no_push_path {
        return vec![
            format!("if not contains -- \"{shims}\" $PATH"),
            prepend,
            "end".to_string(),
        ];
    }
    vec![
        format!("while set pyenv_index (contains -i -- \"{shims}\" $PATH)"),
        "set -eg PATH[$pyenv_index]; end; set -e pyenv_index".to_string(),
        prepend,
    ]
}

fn print_path(o: &mut Output, ctx: &Ctx, shell: &str, no_push_path: bool) {
    let shims = format!("{}/shims", root(ctx));
    let lines = match shell {
        "fish" => fish_path_lines(&shims, no_push_path),
        "pwsh" if no_push_path => vec![
            format!("if ( $Env:PATH -notmatch \"{shims}\" ) {{"),
            format!("$Env:PATH=\"{shims}:$Env:PATH\""),
            "}".to_string(),
        ],
        "pwsh" => vec![
            format!("$Env:PATH=\"$(($Env:PATH -split ':' | where {{ -not ($_ -match '{shims}') }}) -join ':')\""),
            format!("$Env:PATH=\"{shims}:$Env:PATH\""),
        ],
        _ => posix_path_lines(&shims, no_push_path),
    };
    for l in lines {
        o.out(l);
    }
}

fn print_env(o: &mut Output, shell: &str) {
    o.out(match shell {
        "fish" => format!("set -gx PYENV_SHELL {shell}"),
        "pwsh" => format!("$Env:PYENV_SHELL=\"{shell}\""),
        _ => format!("export PYENV_SHELL={shell}"),
    });
}

/// `<prefix>/completions/pyenv.<shell>`, when readable (libexec/pyenv-init:491-503).
fn print_completion(o: &mut Output, shell: &str) {
    let Some(prefix) = crate::install_prefix() else {
        return;
    };
    let path = prefix.join("completions").join(format!("pyenv.{shell}"));
    if std::fs::File::open(&path).is_ok() {
        let p = path.display();
        o.out(if shell == "pwsh" {
            format!("iex (gc {p} -Raw)")
        } else {
            format!("source '{p}'")
        });
    }
}

fn print_rehash(o: &mut Output, shell: &str, no_rehash: bool) {
    if !no_rehash {
        o.out(if shell == "pwsh" {
            "& pyenv rehash"
        } else {
            "command pyenv rehash"
        });
    }
}

const FISH_FUNCTION: &str = r#"function pyenv
  set command $argv[1]
  set -e argv[1]

  switch "$command"
  case @ROUTED@
    source (pyenv "sh-$command" $argv|psub)
  case "*"
    command pyenv "$command" $argv
  end
end
"#;

const PWSH_FUNCTION: &str = r#"function pyenv {
  $command=""
  if ( $args.Count -gt 0 ) {
    $command, $args = $args
  }

  if ( ("@ROUTED@" -split ' ') -contains $command ) {
    $shell_cmds = (& (get-command -commandtype application pyenv -totalcount 1) sh-$command $args)
    if ( $shell_cmds.Count -gt 0 ) {
      iex ($shell_cmds -join "`n")
    }
  } else {
    & (get-command -commandtype application pyenv -totalcount 1) $command $args
  }
}
"#;

const POSIX_BODY: &str = r#"  [ "$#" -gt 0 ] && shift
  case "$command" in
  @PATTERN@)
    eval "$(pyenv "sh-$command" "$@")"
    ;;
  *)
    command pyenv "$command" "$@"
    ;;
  esac
}
"#;

pub(crate) fn fish_function(routed: &[&str]) -> String {
    FISH_FUNCTION.replace("@ROUTED@", &routed.join(" "))
}

/// `header` is the function's first two lines, ending in a newline. An empty routed list
/// becomes the pattern `/`, which matches no command (libexec/pyenv-init:571).
pub(crate) fn posix_function(header: &str, routed: &[&str]) -> String {
    let pattern = if routed.is_empty() {
        "/".to_string()
    } else {
        routed.join("|")
    };
    format!("{header}{}", POSIX_BODY.replace("@PATTERN@", &pattern))
}

/// The `pyenv` function; it routes the `sh-` commands (Decision 4, allowlist D-88).
fn shell_function(ctx: &Ctx, shell: &str) -> String {
    // Built-in and plugin `sh-*` commands (Decision 7 of plan M4a; allowlist D-88).
    let names = commands::command_names(ctx, Listing::ShOnly);
    let routed: Vec<&str> = names.iter().map(String::as_str).collect();
    match shell {
        "fish" => fish_function(&routed),
        "pwsh" => PWSH_FUNCTION.replace("@ROUTED@", &routed.join(" ")),
        "ksh" | "ksh93" | "mksh" => {
            posix_function("function pyenv {\n  typeset command=${1:-}\n", &routed)
        }
        _ => posix_function("pyenv() {\n  local command=${1:-}\n", &routed),
    }
}

/// `pyenv init --install` (libexec/pyenv-init:291-406): checks every startup file before
/// writing any, then appends the setup text to each.
fn install(ctx: &Ctx, shell: &str) -> Output {
    let home = home();
    if home.is_empty() {
        return Output::error("pyenv: HOME must be set to configure shell startup files");
    }
    let p = detect_profile(shell);
    // `${path/#\~/$HOME}`
    let expand = |s: &str| match s.strip_prefix('~') {
        Some(rest) => format!("{home}{rest}"),
        None => s.to_string(),
    };
    let files: Vec<(String, String)> = match shell {
        "bash" | "zsh" | "ksh" | "ksh93" | "mksh" => {
            let setup = posix_shell_setup(ctx, shell).join("\n");
            let (rc, profile) = (expand(p.rc), expand(p.profile));
            let mut f = vec![(rc.clone(), setup.clone())];
            if profile != rc {
                f.push((profile, setup));
            }
            f
        }
        "fish" => vec![(expand(p.rc), FISH_SHELL_SETUP.to_string())],
        "pwsh" => vec![(expand(p.rc), pwsh_shell_setup(ctx).join("\n"))],
        _ => {
            return Output::error(format!(
                "pyenv: cannot automatically configure startup files for {shell}"
            ))
        }
    };
    for (file, _) in &files {
        if let Err(o) = check_startup_file(file, shell) {
            return o;
        }
    }
    if shell == "fish" {
        if let Err(o) = install_fish_user_paths(ctx) {
            return o;
        }
    }
    for (file, text) in &files {
        // Decision 12: upstream's bash prints its own error here; no test reaches it.
        if let Err(e) = append_lines(file, text) {
            return Output::error(format!(
                "pyenv: {file}: {}",
                rpyenv_core::launch::io_reason(&e)
            ));
        }
    }
    Output::new()
}

/// `check_startup_file`: a missing file is fine (a dangling symlink counts as missing); one
/// that isn't a readable regular file, or that mentions "pyenv" in any case, refuses.
fn check_startup_file(file: &str, shell: &str) -> Result<(), Output> {
    let path = Path::new(file);
    if !path.exists() {
        return Ok(());
    }
    let inspect = || Output::error(format!("pyenv: failed to inspect {file}"));
    if !path.is_file() {
        return Err(inspect());
    }
    let bytes = std::fs::read(path).map_err(|_| inspect())?;
    // `grep -Fi pyenv`
    if bytes.to_ascii_lowercase().windows(5).any(|w| w == b"pyenv") {
        let mut o = Output::new();
        o.err(format!("pyenv: cannot automatically apply changes to {file}: it appears to already contain Pyenv-related code."));
        o.err("pyenv: review the file's contents and apply changes manually if necessary.");
        o.err(format!(
            "pyenv: run `pyenv init {shell}` to see the suggested setup."
        ));
        return Err(o.with_code(1));
    }
    Ok(())
}

/// `fish -c "<the fish PATH block>"`, with `fish` from PATH (libexec/pyenv-init:373-386).
fn install_fish_user_paths(ctx: &Ctx) -> Result<(), Output> {
    let fish = rpyenv_core::pathsearch::find_first(
        "fish",
        ctx.path.as_deref(),
        None,
        ctx.flavor,
        ctx.pathext.as_deref(),
    )
    .ok_or_else(|| {
        Output::error("pyenv: fish is not available to configure fish universal variables")
    })?;
    let ok = std::process::Command::new(fish)
        .arg("-c")
        .arg(fish_user_path_setup(ctx).join("\n"))
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err(Output::error(
            "pyenv: failed to configure fish universal variables",
        ))
    }
}

/// `append_lines`: make the parent folder, add a newline when the file doesn't end in one,
/// then append the text and a newline. Opening follows symlinks.
fn append_lines(file: &str, text: &str) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    if let Some(dir) = Path::new(file)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(file)?;
    if f.metadata()?.len() > 0 {
        let mut last = [0u8; 1];
        f.seek(SeekFrom::End(-1))?;
        f.read_exact(&mut last)?;
        if last[0] != b'\n' {
            f.write_all(b"\n")?;
        }
    }
    f.write_all(text.as_bytes())?;
    f.write_all(b"\n")
}
