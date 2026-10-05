//! `pyenv shell`, `sh-shell` and `sh-rehash` on Windows (spec §7; pyenv-win's semantics,
//! M3W; Decisions 1, 6, 10). `shell` is what runs without integration: it validates, then
//! prints the command for the detected shell and exits 1 (allowlist D-89, D-90).
//! `sh-shell` and `sh-rehash` print code for the `pyenv` function of `pyenv init -`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname::{self, Family};

/// pyenv-win's `pyenv shell` help (libexec\pyenv-shell.bat:10-17).
pub const HELP: &str = "Usage: pyenv shell <version>\n       pyenv shell --unset\n\nSets a shell-specific Python version by setting the `PYENV_VERSION'\nenvironment variable in your shell. This version overrides local\napplication-specific versions and the global version.\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Set(String),
    Unset,
}

/// Code for bash, zsh or fish: `\n` line ends and UTF-8, written as they are. `emit`
/// would add `\r`, which `eval "$(…)"` keeps.
pub(crate) fn lf(mut o: Output) -> Output {
    o.raw_stdout = Some(std::mem::take(&mut o.stdout).into_bytes());
    o
}

/// pyenv-win's not-installed message, on stdout (libexec\libs\pyenv-lib.vbs:265-268).
fn not_installed(name: &str) -> Output {
    let mut o = Output::new();
    o.out("pyenv specific python requisite didn't meet. Project is using different version of python.");
    o.out(format!(
        "Install python '{name}' by typing: 'pyenv install {name}'"
    ));
    o.with_code(1)
}

/// `-win32` on X86 unless the name already ends in it, in any case (pyenv-lib.vbs:450-455).
fn with_arch(name: &str, ctx: &Ctx) -> String {
    if ctx.arch_suffix == "-win32" && !name.to_ascii_lowercase().ends_with("-win32") {
        format!("{name}-win32")
    } else {
        name.to_string()
    }
}

/// Decision 6: each name must be a folder of exactly that name, and a safe path segment;
/// the value joins them with a space. `Err` is the first name that fails.
fn validate(ctx: &Ctx, args: &[&str]) -> Result<String, String> {
    let mut names = Vec::new();
    for a in args {
        let name = with_arch(a, ctx);
        // A `%` would be expanded by cmd when the user runs the printed `set` line, and no
        // Python version name has one.
        if name.contains('%')
            || !(crate::install::is_safe_win_segment(&name)
                && ctx.versions_dir().join(&name).is_dir())
        {
            return Err(name);
        }
        names.push(name);
    }
    Ok(names.join(" "))
}

/// No arguments: the raw value or the "no version" line, on stdout, exit 0.
fn show(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    o.out(
        ctx.pyenv_version
            .as_deref()
            .unwrap_or("no shell-specific version configured"),
    );
    o
}

/// A PowerShell string expression in ASCII only. PowerShell also takes U+2018 to U+201B
/// as single quotes, and its code goes through the console code page, so each non-ASCII
/// character becomes `[char]0xNNNN`; pure ASCII stays one `'…'` literal. The expression
/// starts with a string so `+` concatenates.
pub(crate) fn ps_literal(s: &str) -> String {
    if s.is_ascii() {
        return format!("'{}'", s.replace('\'', "''"));
    }
    let mut parts: Vec<String> = Vec::new();
    let mut run = String::new();
    for c in s.chars() {
        if c.is_ascii() {
            if c == '\'' {
                run.push_str("''");
            } else {
                run.push(c);
            }
            continue;
        }
        if !run.is_empty() || parts.is_empty() {
            parts.push(format!("'{run}'"));
            run.clear();
        }
        let mut buf = [0u16; 2];
        for u in c.encode_utf16(&mut buf) {
            parts.push(format!("[char]0x{u:04X}"));
        }
    }
    if !run.is_empty() {
        parts.push(format!("'{run}'"));
    }
    format!("({})", parts.join(" + "))
}

/// The command that applies `action` in a shell of `family`.
fn code(family: Family, action: &Action) -> Vec<String> {
    let line = match (family, action) {
        (Family::Pwsh, Action::Set(v)) => format!("$Env:PYENV_VERSION = {}", ps_literal(v)),
        (Family::Pwsh, Action::Unset) => {
            "Remove-Item Env:PYENV_VERSION -ErrorAction SilentlyContinue".to_string()
        }
        (Family::Cmd, Action::Set(v)) => format!("set \"PYENV_VERSION={v}\""),
        (Family::Cmd, Action::Unset) => "set \"PYENV_VERSION=\"".to_string(),
        (Family::Fish, Action::Set(v)) => format!(
            "set -gx PYENV_VERSION '{}'",
            v.replace('\\', "\\\\").replace('\'', "\\'")
        ),
        (Family::Fish, Action::Unset) => "set -e PYENV_VERSION".to_string(),
        (_, Action::Set(v)) => format!("export PYENV_VERSION='{}'", v.replace('\'', "'\\''")),
        (_, Action::Unset) => "unset PYENV_VERSION".to_string(),
    };
    vec![line]
}

pub(crate) fn parent_image() -> Option<String> {
    #[cfg(windows)]
    {
        rpyenv_core::winproc::parent_image_name()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub(crate) fn pyenv_shell_var() -> Option<String> {
    std::env::var("PYENV_SHELL").ok().filter(|s| !s.is_empty())
}

/// `pyenv shell` without integration (spec §7 "Without integration", Decision 1).
pub fn shell(ctx: &Ctx, args: &[&str]) -> Output {
    let action = match args.first() {
        None => return show(ctx),
        Some(a) if a.eq_ignore_ascii_case("--unset") => Action::Unset,
        Some(_) => match validate(ctx, args) {
            Ok(v) => Action::Set(v),
            Err(name) => return not_installed(&name),
        },
    };
    let name = shellname::windows_shell(parent_image().as_deref(), pyenv_shell_var().as_deref());
    let mut o = Output::new();
    match name {
        Some(name) => {
            let family = shellname::family(&name, Flavor::PyenvWin);
            for l in code(family, &action) {
                o.out(l);
            }
            if family == Family::Cmd {
                o.err("pyenv: cmd has no shell integration, so nothing was changed: run the command above.");
            } else {
                o.err("pyenv: shell integration is not enabled in this shell, so nothing was changed: run the command above.");
                o.err(format!("pyenv: to enable it, see `pyenv init {name}`."));
            }
        }
        None => {
            for (label, family) in [
                ("cmd", Family::Cmd),
                ("PowerShell", Family::Pwsh),
                ("bash", Family::Posix),
                ("fish", Family::Fish),
            ] {
                for l in code(family, &action) {
                    o.out(format!("{label}: {l}"));
                }
            }
            o.err("pyenv: shell integration is not enabled, so nothing was changed: run the line for your shell above.");
        }
    }
    o.with_code(1)
}

/// The shell `sh-shell` and `sh-rehash` print code for: `PYENV_SHELL`, which `pyenv init`
/// set in the shell that runs the function, else the parent process.
pub(crate) fn code_family() -> Option<Family> {
    pyenv_shell_var()
        .and_then(|s| shellname::windows_name(&s))
        .or_else(|| shellname::windows_shell(parent_image().as_deref(), None))
        .map(|n| shellname::family(&n, Flavor::PyenvWin))
}

pub fn sh_shell(ctx: &Ctx, args: &[&str]) -> Output {
    let Some(family) = code_family() else {
        return Output::error("pyenv: can't tell which shell to print code for: set PYENV_SHELL, or load the integration (`pyenv init`)");
    };
    let evaluated = matches!(family, Family::Posix | Family::Ksh | Family::Fish);
    let action = match args.first() {
        // The POSIX and fish functions route `shell` without arguments here (Task 7):
        // print code that echoes the variable, as upstream's sh-shell does.
        None if evaluated => {
            return match ctx.pyenv_version {
                None => Output::error("pyenv: no shell-specific version configured"),
                Some(_) => {
                    let mut o = Output::new();
                    o.out("echo \"$PYENV_VERSION\"");
                    lf(o)
                }
            }
        }
        // The PowerShell function routes `shell` only with arguments, but Windows PowerShell
        // 5.1 drops an empty one: return the state as a literal, which `iex` prints.
        None if family == Family::Pwsh => {
            let shown = show(ctx);
            let mut o = Output::new();
            o.out(ps_literal(shown.stdout.trim_end_matches('\n')));
            return o;
        }
        None => return show(ctx),
        Some(a) if a.eq_ignore_ascii_case("--unset") => Action::Unset,
        Some(_) => match validate(ctx, args) {
            Ok(v) => Action::Set(v),
            // bash and fish evaluate stdout: the message goes to stderr, then `false`.
            Err(name) if evaluated => {
                let m = not_installed(&name);
                let mut o = Output::new();
                o.stderr = m.stdout;
                o.out("false");
                return lf(o.with_code(1));
            }
            // The PowerShell function prints stdout when the exit code isn't 0 (Task 7).
            Err(name) => return not_installed(&name),
        },
    };
    let mut o = Output::new();
    for l in code(family, &action) {
        o.out(l);
    }
    if evaluated {
        lf(o)
    } else {
        o
    }
}

pub fn sh_rehash(_ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    match code_family() {
        Some(Family::Pwsh) => {
            o.out("& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash");
            return o;
        }
        Some(Family::Cmd) => {
            o.out("pyenv rehash");
            return o;
        }
        Some(Family::Fish) => o.out("command pyenv rehash"),
        _ => {
            o.out("command pyenv rehash");
            o.out("hash -r 2>/dev/null || true");
        }
    }
    lf(o)
}
