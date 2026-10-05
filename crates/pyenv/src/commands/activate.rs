//! `sh-activate`, `sh-deactivate`, `activate`/`deactivate` without the shell function, and
//! `virtualenv-init` (spec §10; reference "pyenv sh-activate", "pyenv sh-deactivate",
//! "pyenv virtualenv-init"). POSIX and fish code is v1.4.0's byte for byte; pwsh gets
//! PowerShell (allowlist D-100); Windows shells: D-101.

use crate::commands::shell_win::ps_literal;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
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

fn sh_of(family: Family) -> Sh {
    match family {
        Family::Fish => Sh::Fish,
        Family::Pwsh => Sh::Pwsh,
        Family::Cmd => Sh::Cmd,
        _ => Sh::Posix,
    }
}

/// Windows: the shell `sh-*` code is for, as `sh-shell` finds it (`PYENV_SHELL`, else the
/// parent process).
fn win_shell() -> Option<Sh> {
    crate::commands::shell_win::code_family().map(sh_of)
}

const NO_SHELL: &str = "pyenv: can't tell which shell to print code for: set PYENV_SHELL, or load the integration (`pyenv init`)";

/// `${PYENV_SHELL:-${SHELL##*/}}` on Linux; Windows: `win_shell`, POSIX when unknown.
pub(crate) fn shell(ctx: &Ctx) -> Sh {
    if ctx.flavor == Flavor::PyenvWin {
        return win_shell().unwrap_or(Sh::Posix);
    }
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

/// Windows code for bash, zsh or fish is `\n`-terminated (`shell_win::lf`).
fn finish(ctx: &Ctx, sh: Sh, o: Output) -> Output {
    if ctx.flavor == Flavor::PyenvWin && matches!(sh, Sh::Posix | Sh::Fish) {
        crate::commands::shell_win::lf(o)
    } else {
        o
    }
}

pub fn sh_deactivate(ctx: &Ctx, args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin && win_shell().is_none() {
        return Output::error(NO_SHELL);
    }
    let sh = shell(ctx);
    finish(ctx, sh, deactivate_code(ctx, args, sh))
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
    // `[ -d "${prefix}/conda-meta" ] || [ -x "${prefix}/bin/conda" ]`, built as text as
    // upstream does: with no VIRTUAL_ENV (the nested `--force` deactivate) that is
    // `/conda-meta` and `/bin/conda`, so a host with `/bin/conda` gets `unset CONDA_PREFIX`.
    let conda = Path::new(&format!("{venv_path}/conda-meta")).is_dir()
        || rpyenv_core::pathsearch::is_runnable(Path::new(&format!("{venv_path}/bin/conda")));
    if conda && sh != Sh::Fish {
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
        Sh::Cmd => {
            let old = var("_OLD_VIRTUAL_PROMPT");
            if !old.is_empty() {
                o.out(format!("set \"PROMPT={old}\""));
                o.out("set \"_OLD_VIRTUAL_PROMPT=\"");
            }
        }
    }
    if !quiet {
        if let Some(m) = unrun_hooks(ctx, "deactivate") {
            o.err(m);
        }
    }
    o
}

/// User decision (2026-10-05): pyenv-virtualenv's hook points have no equivalent in rpyenv
/// (allowlist D-51), so hook files written for them are named as not run, on stderr.
pub(crate) fn unrun_hooks(ctx: &Ctx, point: &str) -> Option<String> {
    if ctx.flavor != Flavor::Pyenv {
        return None;
    }
    let listed = crate::commands::misc::hooks(ctx, &[point]).stdout;
    let files: Vec<&str> = listed.lines().filter(|l| !l.is_empty()).collect();
    (!files.is_empty()).then(|| {
        format!(
            "pyenv-virtualenv: rpyenv runs no hooks, so these `{point}' hooks were not run: {}",
            files.join(", ")
        )
    })
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
    if ctx.flavor == Flavor::PyenvWin
        && win_shell().is_none()
        && args.first() != Some(&"--complete")
    {
        return Output::error(NO_SHELL);
    }
    let sh = shell(ctx);
    finish(ctx, sh, activate_code(ctx, args, sh))
}

fn activate_code(ctx: &Ctx, args: &[&str], sh: Sh) -> Output {
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
    // Git Bash and fish on Windows keep paths in MSYS form, as venv's own bash `activate`
    // does (Decision 6).
    let shown = if ctx.flavor == Flavor::PyenvWin && matches!(sh, Sh::Posix | Sh::Fish) {
        crate::commands::init_win::msys(&prefix)
    } else {
        p.clone()
    };
    if (virtual_env == p || virtual_env == shown) && !force {
        if !quiet {
            o.err(format!(
                "pyenv-virtualenv: version `{venv_name}' is already activated"
            ));
        }
        verdict(&mut o, sh, "true");
        return o;
    }
    // cmd starts fresh: its deactivate would restore a prompt it never saved.
    if sh != Sh::Cmd {
        let d = deactivate_code(ctx, &["--force", "--quiet"], sh);
        o.stdout.push_str(&d.stdout);
        o.stderr.push_str(&d.stderr);
    }
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
    o.out(set(sh, "PYENV_VIRTUAL_ENV", &shown));
    o.out(set(sh, "VIRTUAL_ENV", &shown));
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
            Sh::Cmd => {
                // The prompt saved by an earlier activation, so the tags don't stack.
                let old = [var("_OLD_VIRTUAL_PROMPT"), var("PROMPT")]
                    .into_iter()
                    .find(|p| !p.is_empty())
                    .unwrap_or_else(|| "$P$G".into());
                o.out(format!("set \"_OLD_VIRTUAL_PROMPT={old}\""));
                o.out(format!("set \"PROMPT={tag} {old}\""));
            }
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
    if !quiet {
        if let Some(m) = unrun_hooks(ctx, "activate") {
            o.err(m);
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

/// Windows without integration (allowlist D-101): the code for the detected shell, or for
/// every shell, then what to do with it, exit 1 (as `pyenv shell`, D-89).
fn without_integration(ctx: &Ctx, code: impl Fn(Sh) -> Output) -> Output {
    let name = shellname::windows_shell(
        crate::commands::shell_win::parent_image().as_deref(),
        crate::commands::shell_win::pyenv_shell_var().as_deref(),
    );
    let mut o = Output::new();
    match name {
        Some(name) => {
            let family = shellname::family(&name, ctx.flavor);
            let c = code(sh_of(family));
            if c.code != 0 {
                return c;
            }
            o.stdout = c.stdout;
            o.stderr = c.stderr;
            if family == Family::Cmd {
                o.err("pyenv: cmd has no shell integration, so nothing was changed: run the commands above.");
            } else {
                o.err("pyenv: shell integration is not enabled in this shell, so nothing was changed: run the commands above.");
                o.err(format!("pyenv: to enable it, see `pyenv init {name}`."));
            }
        }
        None => {
            for (label, sh) in [
                ("cmd", Sh::Cmd),
                ("PowerShell", Sh::Pwsh),
                ("bash", Sh::Posix),
                ("fish", Sh::Fish),
            ] {
                let c = code(sh);
                if c.code != 0 {
                    return c;
                }
                for l in c.stdout.lines() {
                    o.out(format!("{label}: {l}"));
                }
            }
            o.err("pyenv: shell integration is not enabled, so nothing was changed: run the lines for your shell above.");
        }
    }
    o.with_code(1)
}

pub fn activate(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        o.out("--unset");
        o.stdout
            .push_str(&crate::commands::venv::virtualenvs(ctx, &["--bare"]).stdout);
        return o;
    }
    if ctx.flavor == Flavor::PyenvWin {
        return without_integration(ctx, |sh| activate_code(ctx, args, sh));
    }
    needs_shell("activate")
}

pub fn deactivate(ctx: &Ctx, args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin {
        return without_integration(ctx, |sh| deactivate_code(ctx, args, sh));
    }
    needs_shell("deactivate")
}

/// GNU `stat` (`_stat_fmt` in bin/pyenv-virtualenv-init:13-17); rpyenv's Linux flavor is GNU.
const STAT_FMT: &str = "-L -c %Y";

/// bin/pyenv-virtualenv-init:186-247 (bash and zsh), `{STAT}` for the stat format. The cache
/// parts are left out when `pyenv hooks version-name` lists hooks, as upstream does.
const POSIX_HEAD: &str = "_pyenv_virtualenv_hook() {\n  local ret=$?\n";
const POSIX_CACHE_CHECK: &str = r#"  if [ "${PYENV_VERSION-}" = "${_PYENV_VH_VERSION-}" ] \
    && [ "${VIRTUAL_ENV-}" = "${_PYENV_VH_VENV-}" ]; then
    if [ -n "${PYENV_VERSION-}" ]; then
      return $ret
    fi
    if [ "${PWD}" = "${_PYENV_VH_PWD-}" ] \
      && [ "$(stat {STAT} "${_PYENV_VH_PATHS[@]}" 2>/dev/null)" = "${_PYENV_VH_MTIMES-}" ]; then
      return $ret
    fi
  fi
"#;
const POSIX_EVAL: &str = r#"  if [ -n "${VIRTUAL_ENV-}" ]; then
    eval "$(pyenv sh-activate --quiet || pyenv sh-deactivate --quiet || true)" || true
  else
    eval "$(pyenv sh-activate --quiet || true)" || true
  fi
"#;
const POSIX_CACHE_SAVE: &str = "  _PYENV_VH_PWD=\"${PWD}\"\n  _PYENV_VH_VERSION=\"${PYENV_VERSION-}\"\n  _PYENV_VH_VENV=\"${VIRTUAL_ENV-}\"\n  local _pvh_d=\"${PWD}\" _pvh_found_local=0\n  _PYENV_VH_PATHS=()\n  while :; do\n    if [ -f \"${_pvh_d}/.python-version\" ] || [ -L \"${_pvh_d}/.python-version\" ]; then\n      _PYENV_VH_PATHS+=(\"${_pvh_d}/.python-version\")\n      if [ -f \"${_pvh_d}/.python-version\" ]; then \n        _pvh_found_local=1\n        break\n      fi\n    else\n      _PYENV_VH_PATHS+=(\"${_pvh_d}\")\n    fi\n    [ \"${_pvh_d}\" = \"/\" ] && break\n    _pvh_d=\"${_pvh_d%/*}\"\n    [ -z \"${_pvh_d}\" ] && _pvh_d=\"/\"\n  done\n  if [ \"${_pvh_found_local}\" = \"0\" ]; then\n    _PYENV_VH_PATHS+=(\"${PYENV_ROOT}/version\")\n  fi\n  _PYENV_VH_MTIMES=\"$(stat {STAT} \"${_PYENV_VH_PATHS[@]}\" 2>/dev/null)\"\n";
const POSIX_TAIL: &str = "  return $ret\n};\n";

/// bin/pyenv-virtualenv-init:121-184 (fish).
const FISH_HEAD: &str =
    "function _pyenv_virtualenv_hook --on-event fish_prompt;\n  set -l ret $status\n";
const FISH_CACHE_CHECK: &str = r#"  if test "$PYENV_VERSION" = "$_PYENV_VH_VERSION" \
    -a "$VIRTUAL_ENV" = "$_PYENV_VH_VENV"
    if test -n "$PYENV_VERSION"
      return $ret
    end
    if test "$PWD" = "$_PYENV_VH_PWD" \
      -a "(stat {STAT} $_PYENV_VH_PATHS 2>/dev/null)" = "$_PYENV_VH_MTIMES"
      return $ret
    end
  end
"#;
const FISH_EVAL: &str = "  if [ -n \"$VIRTUAL_ENV\" ]\n    pyenv activate --quiet; or pyenv deactivate --quiet; or true\n  else\n    pyenv activate --quiet; or true\n  end\n";
const FISH_CACHE_SAVE: &str = "  set -g _PYENV_VH_PWD \"$PWD\"\n  set -g _PYENV_VH_VERSION \"$PYENV_VERSION\"\n  set -g _PYENV_VH_VENV \"$VIRTUAL_ENV\"\n  set -l d \"$PWD\"\n  set -l _pvh_found_local 0\n  set -g _PYENV_VH_PATHS\n  while true\n    if test -f \"$d/.python-version\"; or test -L \"$d/.python-version\"\n      set -g _PYENV_VH_PATHS $_PYENV_VH_PATHS \"$d/.python-version\"\n      if test -f \"$d/.python-version\" \n        set _pvh_found_local 1\n        break\n      end\n    else\n      set -g _PYENV_VH_PATHS $_PYENV_VH_PATHS \"$d\"\n    end\n    test \"$d\" = \"/\"; and break\n    set d (string replace -r '/[^/]*$' '' -- \"$d\")\n    test -z \"$d\"; and set d \"/\"\n  end\n  if test \"$_pvh_found_local\" = \"0\"\n    set -g _PYENV_VH_PATHS $_PYENV_VH_PATHS \"$PYENV_ROOT/version\"\n  end\n  set -g _PYENV_VH_MTIMES (stat {STAT} $_PYENV_VH_PATHS 2>/dev/null)\n";
const FISH_TAIL: &str = "  return $ret\nend\n";

const BASH_REGISTER: &str = "if ! [[ \"${PROMPT_COMMAND-}\" =~ _pyenv_virtualenv_hook ]]; then\n  PROMPT_COMMAND=\"_pyenv_virtualenv_hook;${PROMPT_COMMAND-}\"\nfi\n";
const ZSH_REGISTER: &str = "typeset -g -a precmd_functions\nif [[ -z $precmd_functions[(r)_pyenv_virtualenv_hook] ]]; then\n  precmd_functions=(_pyenv_virtualenv_hook $precmd_functions);\nfi\n";

/// shims/activate and shims/deactivate of v1.4.0: `source activate <env>` helpers.
const SHIM_ACTIVATE: &str = "#!/usr/bin/env bash\nif [[ \"$0\" != \"${BASH_SOURCE}\" ]]; then\n  eval \"$(pyenv sh-activate --verbose \"$@\" || true)\"\nelse\n  echo \"pyenv-virtualenv: activate must be sourced. Run 'source activate envname' instead of 'activate envname'\" 1>&2\n  false\nfi\n";
const SHIM_DEACTIVATE: &str = "#!/usr/bin/env bash\nif [[ \"$0\" != \"${BASH_SOURCE}\" ]]; then\n  eval \"$(pyenv sh-deactivate --verbose \"$@\" || true)\"\nelse\n  echo \"pyenv-virtualenv: deactivate must be sourced. Run 'source deactivate' instead of 'deactivate'\" 1>&2\n  false\nfi\n";

fn hook(head: &str, check: &str, eval: &str, save: &str, tail: &str, cached: bool) -> String {
    let mut s = head.to_string();
    if cached {
        s.push_str(&check.replace("{STAT}", STAT_FMT));
    }
    s.push_str(eval);
    if cached {
        s.push_str(&save.replace("{STAT}", STAT_FMT));
    }
    s.push_str(tail);
    s
}

/// The folder `virtualenv-init` puts first on `PATH` (allowlist D-102):
/// `$PYENV_VIRTUALENV_ROOT/shims` when set (nothing is written there), else
/// `<root>/.rpyenv/virtualenv/shims`, whose two helpers are written when missing, and never
/// through a symlinked `.rpyenv` or `virtualenv` folder (M4a's rule for rpyenv's own files).
fn helper_shims(ctx: &Ctx) -> PathBuf {
    if let Some(r) = std::env::var_os("PYENV_VIRTUALENV_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(r).join("shims");
    }
    let own = ctx.root.join(".rpyenv");
    let venv_dir = own.join("virtualenv");
    let shims = venv_dir.join("shims");
    let linked = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    if linked(&own) || linked(&venv_dir) || linked(&shims) {
        return shims;
    }
    if std::fs::create_dir_all(&shims).is_ok() {
        for (name, body) in [("activate", SHIM_ACTIVATE), ("deactivate", SHIM_DEACTIVATE)] {
            let p = shims.join(name);
            if std::fs::symlink_metadata(&p).is_err() && std::fs::write(&p, body).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
    }
    shims
}

/// `virtualenv-init [-] [<shell>]` (reference "pyenv virtualenv-init"). Windows: nothing
/// (allowlist D-101).
pub fn virtualenv_init(ctx: &Ctx, args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin {
        return Output::new();
    }
    // Each `-` sets print mode and shifts away the first remaining argument, whichever it is.
    let mut rest: Vec<&str> = args.to_vec();
    let mut print = false;
    for a in args {
        if *a == "-" {
            print = true;
            if !rest.is_empty() {
                rest.remove(0);
            }
        }
    }
    let shell = rest
        .first()
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PYENV_SHELL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| crate::commands::init::shell_name(None));
    if !print {
        let profile = match shell.as_str() {
            "bash" => "~/.bashrc",
            "zsh" => "~/.zshrc",
            "ksh" => "~/.profile",
            "fish" => "~/.config/fish/config.fish",
            _ => "your profile",
        };
        let line = if shell == "fish" {
            "status --is-interactive; and source (pyenv virtualenv-init -|psub)"
        } else {
            "eval \"$(pyenv virtualenv-init -)\""
        };
        return Output {
            stderr: format!(
                "# Load pyenv-virtualenv automatically by adding\n# the following to {profile}:\n\n{line}\n\n"
            ),
            code: 1,
            ..Output::new()
        };
    }
    let shims = helper_shims(ctx);
    let s = shims.display();
    let mut o = Output::new();
    if shell == "fish" {
        o.stdout.push_str(&format!(
            "while set index (contains -i -- \"{s}\" $PATH)\nset -eg PATH[$index]; end; set -e index\nset -gx PATH '{s}' $PATH;\nset -gx PYENV_VIRTUALENV_INIT 1;\n"
        ));
    } else {
        o.out(format!("export PATH=\"{s}:${{PATH}}\";"));
        o.out("export PYENV_VIRTUALENV_INIT=1;");
    }
    let cached = crate::commands::misc::hooks(ctx, &["version-name"])
        .stdout
        .is_empty();
    match shell.as_str() {
        "bash" | "zsh" => {
            o.stdout.push_str(&hook(
                POSIX_HEAD,
                POSIX_CACHE_CHECK,
                POSIX_EVAL,
                POSIX_CACHE_SAVE,
                POSIX_TAIL,
                cached,
            ));
            o.stdout.push_str(if shell == "bash" {
                BASH_REGISTER
            } else {
                ZSH_REGISTER
            });
        }
        "fish" => o.stdout.push_str(&hook(
            FISH_HEAD,
            FISH_CACHE_CHECK,
            FISH_EVAL,
            FISH_CACHE_SAVE,
            FISH_TAIL,
            cached,
        )),
        _ => {}
    }
    o
}
