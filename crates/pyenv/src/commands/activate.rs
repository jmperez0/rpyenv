//! `sh-activate`, `sh-deactivate`, `activate`/`deactivate` without the shell function, and
//! `virtualenv-init` (spec §10; reference "pyenv sh-activate", "pyenv sh-deactivate",
//! "pyenv virtualenv-init"). POSIX and fish code is v1.4.0's byte for byte; pwsh gets
//! PowerShell (allowlist D-100); Windows shells: D-101.

use crate::commands::shell_win::ps_literal;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::shellname::{self, Family};
use rpyenv_core::venv;
use std::path::{Path, PathBuf};

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sh {
    Posix,
    Fish,
    Pwsh,
    Cmd,
}

/// `${PYENV_SHELL:-${SHELL##*/}}` on Linux; Task 8 adds Windows.
pub(crate) fn shell(ctx: &Ctx) -> Sh {
    let name = shellname::from_env(
        std::env::var("PYENV_SHELL").ok().as_deref(),
        std::env::var("SHELL").ok().as_deref(),
    );
    match shellname::family(&name, ctx.flavor) {
        Family::Fish => Sh::Fish,
        Family::Pwsh => Sh::Pwsh,
        Family::Cmd => Sh::Cmd,
        _ => Sh::Posix,
    }
}

const TAIL_POSIX: &str = "if [ -n \"${_OLD_VIRTUAL_PATH:-}\" ]; then\n  export PATH=\"${_OLD_VIRTUAL_PATH}\";\n  unset _OLD_VIRTUAL_PATH;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PYTHONHOME:-}\" ]; then\n  export PYTHONHOME=\"${_OLD_VIRTUAL_PYTHONHOME}\";\n  unset _OLD_VIRTUAL_PYTHONHOME;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PS1:-}\" ]; then\n  export PS1=\"${_OLD_VIRTUAL_PS1}\";\n  unset _OLD_VIRTUAL_PS1;\nfi;\nif declare -f deactivate 1>/dev/null 2>&1; then\n  unset -f deactivate;\nfi;\n";

const TAIL_FISH: &str = "if [ -n \"$_OLD_VIRTUAL_PATH\" ];\n  set -gx PATH \"$_OLD_VIRTUAL_PATH\";\n  set -e _OLD_VIRTUAL_PATH;\nend;\nif [ -n \"$_OLD_VIRTUAL_PYTHONHOME\" ];\n  set -gx PYTHONHOME \"$_OLD_VIRTUAL_PYTHONHOME\";\n  set -e _OLD_VIRTUAL_PYTHONHOME;\nend;\n# check if old prompt function exists\nif functions -q _pyenv_old_prompt\n  # remove old prompt function if exists.\n  functions -e fish_prompt\n  functions -c _pyenv_old_prompt fish_prompt\n  functions -e _pyenv_old_prompt\nend\nif functions -q deactivate;\n  functions -e deactivate;\nend;\n";

const TAIL_PWSH: &str = "if ($Env:_OLD_VIRTUAL_PATH) { $Env:PATH = $Env:_OLD_VIRTUAL_PATH; Remove-Item Env:_OLD_VIRTUAL_PATH }\nif ($Env:_OLD_VIRTUAL_PYTHONHOME) { $Env:PYTHONHOME = $Env:_OLD_VIRTUAL_PYTHONHOME; Remove-Item Env:_OLD_VIRTUAL_PYTHONHOME }\nif (Test-Path function:_pyenv_old_prompt) { Set-Item function:global:prompt (Get-Item function:_pyenv_old_prompt).ScriptBlock; Remove-Item function:_pyenv_old_prompt }\nif (Test-Path function:deactivate) { Remove-Item function:deactivate }\n";

fn set(sh: Sh, k: &str, v: &str) -> String {
    match sh {
        Sh::Posix => format!("export {k}=\"{v}\";"),
        Sh::Fish => format!("set -gx {k} \"{v}\";"),
        Sh::Pwsh => format!("$Env:{k} = {}", ps_literal(v)),
        Sh::Cmd => format!("set \"{k}={v}\""),
    }
}

fn unset(sh: Sh, k: &str) -> String {
    match sh {
        Sh::Posix => format!("unset {k};"),
        Sh::Fish => format!("set -e {k};"),
        Sh::Pwsh => format!("Remove-Item Env:{k} -ErrorAction SilentlyContinue"),
        Sh::Cmd => format!("set \"{k}=\""),
    }
}

/// `<dir>/*.<ext>` in glob order.
fn scripts(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == ext))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The failure shape: stdout `false` (or `true`) for an evaluating shell, nothing for pwsh,
/// whose function prints stdout on failure.
fn verdict(o: &mut Output, sh: Sh, word: &str) {
    if sh != Sh::Pwsh {
        o.out(word);
    }
}

pub fn sh_deactivate(ctx: &Ctx, args: &[&str]) -> Output {
    deactivate_code(ctx, args, shell(ctx))
}

pub(crate) fn deactivate_code(ctx: &Ctx, args: &[&str], sh: Sh) -> Output {
    let (mut force, mut quiet, mut verbose) = (false, false, false);
    for a in args {
        match *a {
            "-f" | "--force" => force = true,
            "-q" | "--quiet" => quiet = true,
            "-v" | "--verbose" => {
                quiet = false;
                verbose = true;
            }
            _ => break,
        }
    }
    verbose |= !var("PYENV_VIRTUALENV_VERBOSE_ACTIVATE").is_empty();
    let venv_path = var("VIRTUAL_ENV");
    let mut o = Output::new();
    if venv_path.is_empty() && !force {
        if !quiet {
            o.err("pyenv-virtualenv: no virtualenv has been activated.");
        }
        verdict(&mut o, sh, "false");
        return o.with_code(1);
    }
    let root = format!("{}/", ctx.versions_dir().display());
    let name = match venv_path.strip_prefix(&root) {
        Some(rest) if rest.contains("/envs/") => rest.to_string(),
        _ => venv_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_string(),
    };
    if verbose {
        o.err(format!("pyenv-virtualenv: deactivate {name}"));
    }
    let prefix = Path::new(&venv_path);
    if !venv_path.is_empty() && venv::is_conda(prefix) && sh != Sh::Fish {
        if sh == Sh::Posix {
            for s in scripts(&prefix.join("etc/conda/deactivate.d"), "sh") {
                o.out(format!(". \"{}\";", s.display()));
            }
            o.out("unset CONDA_PREFIX");
        } else {
            o.out(unset(sh, "CONDA_PREFIX"));
        }
    }
    if !var("PYENV_ACTIVATE_SHELL").is_empty() {
        o.out(unset(sh, "PYENV_VERSION"));
        o.out(unset(sh, "PYENV_ACTIVATE_SHELL"));
    }
    o.out(unset(sh, "PYENV_VIRTUAL_ENV"));
    o.out(unset(sh, "VIRTUAL_ENV"));
    if !var("CONDA_DEFAULT_ENV").is_empty() {
        o.out(unset(sh, "CONDA_DEFAULT_ENV"));
    }
    match sh {
        Sh::Posix => o.stdout.push_str(TAIL_POSIX),
        Sh::Fish => o.stdout.push_str(TAIL_FISH),
        Sh::Pwsh => o.stdout.push_str(TAIL_PWSH),
        Sh::Cmd => o.out("set \"PROMPT=%_OLD_VIRTUAL_PROMPT%\""),
    }
    o
}

/// `versions/<v>`, one level of link resolved, as v1.4.0 computes `VIRTUAL_ENV`.
fn env_prefix(ctx: &Ctx, name: &str) -> PathBuf {
    let p =
        rpyenv_core::prefix::prefix_of(ctx, name).unwrap_or_else(|_| venv::version_dir(ctx, name));
    std::fs::read_link(&p)
        .map(|t| {
            if t.is_absolute() {
                t
            } else {
                p.parent().unwrap_or(&p).join(t)
            }
        })
        .unwrap_or(p)
}

pub fn sh_activate(ctx: &Ctx, args: &[&str]) -> Output {
    let sh = shell(ctx);
    let (mut force, mut quiet, mut verbose) = (false, false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--complete" => {
                let mut o = Output::new();
                o.out("--unset");
                o.stdout
                    .push_str(&crate::commands::venv::virtualenvs(ctx, &["--bare"]).stdout);
                return o;
            }
            "-f" | "--force" => force = true,
            "-q" | "--quiet" => quiet = true,
            "--unset" => return deactivate_code(ctx, &[], sh),
            "-v" | "--verbose" => {
                quiet = false;
                verbose = true;
            }
            _ => break,
        }
        i += 1;
    }
    verbose |= !var("PYENV_VIRTUALENV_VERBOSE_ACTIVATE").is_empty();
    let mut versions: Vec<String> = args[i..].iter().map(|s| s.to_string()).collect();
    let mut no_shell = false;
    if versions.is_empty() {
        no_shell = true;
        versions = match crate::commands::venv::current(ctx) {
            Ok(v) => v,
            Err(o) => return o,
        };
        if versions.is_empty() {
            versions.push("system".into());
        }
    }
    if var("PYENV_VIRTUALENV_INIT").is_empty() {
        no_shell = false;
    }
    let mut venv_name = versions[0].clone();
    let mut o = Output::new();
    let (virtual_env, pyenv_virtual_env) = (var("VIRTUAL_ENV"), var("PYENV_VIRTUAL_ENV"));
    if !virtual_env.is_empty()
        && (pyenv_virtual_env.is_empty() || virtual_env != pyenv_virtual_env)
        && !force
    {
        if !quiet {
            o.err(format!(
                "pyenv-virtualenv: virtualenv `{virtual_env}' is already activated"
            ));
        }
        verdict(&mut o, sh, "true");
        return o;
    }
    if venv::base_prefix(ctx, &venv_name).is_err() {
        let cur = crate::commands::venv::current_names(ctx)
            .into_iter()
            .next()
            .unwrap_or_default();
        let long = format!(
            "{}/envs/{venv_name}",
            cur.split("/envs/").next().unwrap_or("")
        );
        if venv::base_prefix(ctx, &long).is_ok() {
            venv_name = long;
        } else {
            if !quiet {
                o.err(format!(
                    "pyenv-virtualenv: version `{venv_name}' is not a virtualenv"
                ));
            }
            verdict(&mut o, sh, "false");
            return o.with_code(1);
        }
    }
    if versions[1..]
        .iter()
        .any(|v| venv::base_prefix(ctx, v).is_ok())
    {
        if !quiet {
            o.err(format!(
                "pyenv-virtualenv: cannot activate multiple versions at once: {}",
                versions.join(" ")
            ));
        }
        verdict(&mut o, sh, "false");
        return o.with_code(1);
    }
    let prefix = env_prefix(ctx, &venv_name);
    let p = prefix.display().to_string();
    if virtual_env == p && !force {
        if !quiet {
            o.err(format!(
                "pyenv-virtualenv: version `{venv_name}' is already activated"
            ));
        }
        verdict(&mut o, sh, "true");
        return o;
    }
    let d = deactivate_code(ctx, &["--force", "--quiet"], sh);
    o.stdout.push_str(&d.stdout);
    o.stderr.push_str(&d.stderr);
    if verbose {
        o.err(format!("pyenv-virtualenv: activate {venv_name}"));
    }
    if !no_shell {
        o.out(set(sh, "PYENV_VERSION", &versions.join(":")));
        o.out(match sh {
            Sh::Posix => "export PYENV_ACTIVATE_SHELL=1;".to_string(),
            Sh::Fish => "set -gx PYENV_ACTIVATE_SHELL 1;".to_string(),
            _ => set(sh, "PYENV_ACTIVATE_SHELL", "1"),
        });
    }
    o.out(set(sh, "PYENV_VIRTUAL_ENV", &p));
    o.out(set(sh, "VIRTUAL_ENV", &p));
    let conda = venv::is_conda(&prefix);
    if conda {
        let name = if p.contains("/envs/") {
            venv_name.rsplit('/').next().unwrap_or("").to_string()
        } else {
            "root".into()
        };
        o.out(set(sh, "CONDA_DEFAULT_ENV", &name));
    }
    let pythonhome = var("PYTHONHOME");
    if !pythonhome.is_empty() {
        o.out(set(sh, "_OLD_VIRTUAL_PYTHONHOME", &pythonhome));
        o.out(unset(sh, "PYTHONHOME"));
    }
    let disabled = [
        "PYENV_VIRTUALENV_DISABLE_PROMPT",
        "PYENV_VIRTUAL_ENV_DISABLE_PROMPT",
        "VIRTUAL_ENV_DISABLE_PROMPT",
    ]
    .iter()
    .any(|k| !var(k).is_empty());
    if !disabled {
        let tag = match var("PYENV_VIRTUALENV_PROMPT") {
            t if t.is_empty() => format!("({venv_name})"),
            t => t.replacen("{venv}", &venv_name, 1),
        };
        match sh {
            Sh::Posix => {
                o.out("export _OLD_VIRTUAL_PS1=\"${PS1:-}\";");
                o.out(format!("export PS1=\"{tag} ${{PS1:-}}\";"));
            }
            Sh::Fish if !quiet => o.stdout.push_str(&fish_prompt(&venv_name)),
            Sh::Fish => {}
            Sh::Pwsh => {
                o.out("if (-not (Test-Path function:_pyenv_old_prompt)) { Set-Item function:global:_pyenv_old_prompt (Get-Item function:prompt).ScriptBlock }");
                o.out(format!(
                    "function global:prompt {{ Write-Host -NoNewline {}; _pyenv_old_prompt }}",
                    ps_literal(&format!("{tag} "))
                ));
            }
            Sh::Cmd => o.out(format!("set \"PROMPT={tag} $P$G\"")),
        }
    }
    if conda {
        match sh {
            Sh::Posix => {
                o.out(format!("export CONDA_PREFIX=\"{p}\";"));
                for s in scripts(&prefix.join("etc/conda/activate.d"), "sh")
                    .into_iter()
                    .chain(scripts(&prefix.join("etc/profile.d"), "sh"))
                {
                    o.out(format!(". \"{}\";", s.display()));
                }
            }
            Sh::Fish => {
                for s in scripts(&prefix.join("etc/fish/conf.d"), "fish") {
                    o.out(format!("source \"{}\";", s.display()));
                }
            }
            _ => o.out(set(sh, "CONDA_PREFIX", &p)),
        }
    }
    o
}

/// bin/pyenv-sh-activate:247-261 with `${venv}` substituted: copy the bytes from the source.
fn fish_prompt(venv: &str) -> String {
    format!(
        "functions -e _pyenv_old_prompt              # remove old prompt function if exists. \n                                            # since everything is in memory, it's safe to\n                                            # remove it.\nfunctions -c fish_prompt _pyenv_old_prompt  # backup old prompt function\n\n# from python-venv\nfunction fish_prompt\n    set -l prompt (_pyenv_old_prompt)       # call old prompt function first since it might \n                                            # read exit status\n    echo -n \"({venv}) \"                    # add virtualenv to prompt\n    string join -- \\n $prompt              # handle multiline prompts\nend\n"
    )
}

/// `pyenv activate`/`deactivate` when the shell function isn't loaded (bin/pyenv-activate:23-30).
fn needs_shell(cmd: &str) -> Output {
    Output {
        stderr: format!("\u{1b}[31;1m\n`pyenv {cmd}' requires Pyenv and Pyenv-Virtualenv to be loaded into your shell.\nCheck your shell configuration and Pyenv and Pyenv-Virtualenv installation instructions.\n\n\u{1b}[0m"),
        code: 1,
        ..Output::new()
    }
}

pub fn activate(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        o.out("--unset");
        o.stdout
            .push_str(&crate::commands::venv::virtualenvs(ctx, &["--bare"]).stdout);
        return o;
    }
    needs_shell("activate")
}

pub fn deactivate(_ctx: &Ctx, _args: &[&str]) -> Output {
    needs_shell("deactivate")
}
