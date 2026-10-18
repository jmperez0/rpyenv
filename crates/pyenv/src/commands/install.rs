//! `pyenv install` (docs/parity/pyenv-m2-reference.md "pyenv install"), Linux flavor.

use crate::commands::latest::resolve_known;
use crate::install::builder::{self, patch_files, Job, Options};
use crate::install::conda;
use crate::install::defs::{self, Found, Origin};
use crate::install::fetch::Fetcher;
use crate::install::pypy::{self, Verdict};
use crate::install::txn::{is_complete, Txn};
use crate::install::{
    catalog, default_packages, interrupted, preflight, prompt, store, watch_interrupt,
    InstallError, Reply,
};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::verfile;
use std::ffi::OsString;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n       pyenv install [-f] [-kvp] <definition-file>[:<alias>]\n       pyenv install -l|--list [--bare]\n       pyenv install --version\n\n  -l/--list          List all available versions\n  -f/--force         Install even if the version appears to be installed already\n  -s/--skip-existing Skip if the version appears to be installed already\n\n  python-build options:\n\n  -k/--keep          Keep source tree in $PYENV_BUILD_ROOT after installation\n                     (defaults to $PYENV_ROOT/sources)\n  -p/--patch         Apply a patch from stdin before building\n  -v/--verbose       Verbose mode: print compilation status to stdout\n  --version          Show version of python-build\n  -g/--debug         Build a debug version\n  --deps=<route>     How to get missing build libraries: system, install, build, prebuilt\n\n  Append `:<alias>' to a version to install it under a custom name, so that\n  several builds of the same version can coexist:\n\n      pyenv install 3.12.0:my-3.12\n\n  This installs into $PYENV_ROOT/versions/my-3.12.\n\nFor detailed information on installing Python versions with\npython-build, including a list of environment variables for adjusting\ncompilation, see: https://github.com/pyenv/pyenv#readme\n\n";
pub const USAGE: &str = "Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n       pyenv install [-f] [-kvp] <definition-file>[:<alias>]\n       pyenv install -l|--list [--bare]\n       pyenv install --version";

fn say(line: &str) {
    rpyenv_core::textout::write(true, &format!("{line}\n"));
}

#[derive(Default)]
struct Flags {
    list: bool,
    bare: bool,
    force: bool,
    skip: bool,
    keep: bool,
    verbose: bool,
    patch: bool,
    debug: bool,
    deps: Option<String>,
}

enum Early {
    Help,
    Usage,
    Version,
}

/// python-build's `parse_options`: every `-…` argument is options (`--name` or `-abc`),
/// handled in order; the rest are positional.
fn parse<'a>(args: &[&'a str]) -> Result<(Flags, Vec<&'a str>), Early> {
    let mut f = Flags::default();
    let mut pos = Vec::new();
    for a in args {
        let names: Vec<String> = if let Some(long) = a.strip_prefix("--") {
            vec![long.to_string()]
        } else if let Some(short) = a.strip_prefix('-') {
            short.chars().map(String::from).collect()
        } else {
            pos.push(*a);
            continue;
        };
        for n in names {
            if let Some(route) = n.strip_prefix("deps=") {
                f.deps = Some(route.to_string());
                continue;
            }
            match n.as_str() {
                "h" | "help" => return Err(Early::Help),
                "bare" => f.bare = true,
                "l" | "list" => f.list = true,
                "f" | "force" => f.force = true,
                "s" | "skip-existing" => f.skip = true,
                "k" | "keep" => f.keep = true,
                "v" | "verbose" => f.verbose = true,
                "p" | "patch" => f.patch = true,
                "g" | "debug" => f.debug = true,
                "version" => return Err(Early::Version),
                _ => return Err(Early::Usage),
            }
        }
    }
    Ok((f, pos))
}

/// A name that can't be a directory directly under `versions/`, or is one of the
/// installer's own staging names (allowlist D-69).
fn invalid_name(name: &str) -> bool {
    name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || rpyenv_core::installed::is_staging_name(name)
}

fn not_found(definition: &str, names: &[String]) {
    say(&format!("python-build: definition not found: {definition}"));
    let matches: Vec<&String> = names.iter().filter(|n| n.contains(definition)).collect();
    if !matches.is_empty() {
        say("");
        say(&format!(
            "The following versions contain `{definition}' in the name:"
        ));
        for n in matches {
            say(&format!("  {n}"));
        }
    }
    say("");
    say("See all available versions with `pyenv install --list'.");
    say("");
    say("If the version you need is missing, try upgrading pyenv.");
}

fn needs_patch(found: &Found, def: &defs::Definition) -> bool {
    def.packages
        .iter()
        .any(|p| p.name.starts_with("Python-") && !patch_files(found, &p.name).is_empty())
}

/// `k`'s raw value in `env` (the last entry wins), when non-empty.
fn get_os(env: &[(OsString, OsString)], k: &str) -> Option<OsString> {
    env.iter()
        .rev()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

/// M7a design §4.2: what a `pypy*` definition installs here, as a one-line definition the
/// interpreter reads. The vendored copy uses the table's row; a user's own one that the
/// interpreter can't read is evaluated with `bash`. `None` for every other definition.
fn pypy_text(found: &Found, interpretable: bool) -> Option<Result<String, String>> {
    let builtin = found.origin == Origin::Builtin;
    if !found.name.starts_with("pypy") || (!builtin && interpretable) {
        return None;
    }
    let arch = pypy::host_arch();
    let shown = arch.unwrap_or(std::env::consts::ARCH);
    let verdict = match (&found.origin, arch) {
        (_, None) => Ok(Verdict::NoBinary),
        (Origin::Builtin, Some(a)) => pypy::linux_table().map(|rows| {
            pypy::linux_verdict(&rows, &found.name, a)
                .cloned()
                .unwrap_or(Verdict::NoBinary)
        }),
        (Origin::Dir(d), Some(a)) => pypy::evaluate(&d.join(&found.name), a),
        (Origin::Path(p), Some(a)) => pypy::evaluate(p, a),
    };
    let name = &found.name;
    Some(verdict.and_then(|v| match v {
        Verdict::Ok {
            package,
            url,
            steps,
        } => Ok(pypy::definition_text(&package, &url, &steps)),
        Verdict::NoBinary => Err(format!(
            "python-build: {name}: The binary distribution of PyPy is not available for {shown}."
        )),
        Verdict::Unsupported(_) if builtin => Err(format!(
            "python-build: {name}: PyPy source, nightly, distribution-specific and third-party builds are not supported"
        )),
        Verdict::Unsupported(why) => Err(format!(
            "python-build: {name}: rpyenv cannot read this PyPy definition ({why})"
        )),
    }))
}
/// M7b design §4.2-4.3: what a conda-family definition installs here, as a one-line definition
/// the interpreter reads. The vendored copy uses the table's row; a user's own one that the
/// interpreter can't read is evaluated with `bash`. `None` for every other definition.
fn conda_text(
    found: &Found,
    interpretable: bool,
    fetcher: &Fetcher,
    say: &mut dyn FnMut(&str),
) -> Option<Result<String, String>> {
    let builtin = found.origin == Origin::Builtin;
    if !conda::is_conda_name(&found.name) || (!builtin && interpretable) {
        return None;
    }
    let name = &found.name;
    let Some(arch) = conda::host_arch() else {
        return Some(Err(format!(
            "\nERROR: The binary distribution of {name} is not available for Linux-{}.\n",
            std::env::consts::ARCH
        )));
    };
    let verdict = match &found.origin {
        Origin::Builtin => conda::linux_table().map(|rows| {
            conda::linux_verdict(&rows, name, arch)
                .cloned()
                .unwrap_or(conda::Verdict::NoBinary(name.clone()))
        }),
        Origin::Dir(d) => conda::evaluate(&d.join(name), arch),
        Origin::Path(p) => conda::evaluate(p, arch),
    };
    let notice = |i: &conda::Install, say: &mut dyn FnMut(&str)| {
        if i.env
            .iter()
            .any(|(k, _)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS")
        {
            say(conda::TERMS_NOTICE);
        }
    };
    Some(verdict.and_then(|v| match v {
        conda::Verdict::Ok(i) => {
            notice(&i, say);
            Ok(conda::definition_text(&i))
        }
        conda::Verdict::OkLatest(i) => {
            notice(&i, say);
            conda::latest_source(name, &i, arch, fetcher).map(|i| conda::definition_text(&i))
        }
        conda::Verdict::NoBinary(what) => Err(format!(
            "\nERROR: The binary distribution of {what} is not available for {arch}.\n"
        )),
        conda::Verdict::Unsupported(why) if builtin => {
            let hint = conda::linux_table()
                .ok()
                .and_then(|rows| conda::newest_installable(&rows, name, arch))
                .map(|n| {
                    format!(
                        "; the newest {} rpyenv can install here is {n}",
                        conda::family(name)
                    )
                })
                .unwrap_or_default();
            Err(format!(
                "python-build: {name}: rpyenv cannot install this conda definition ({why}){hint}"
            ))
        }
        conda::Verdict::Unsupported(why) => Err(format!(
            "python-build: {name}: rpyenv cannot read this conda definition ({why})"
        )),
    }))
}

/// A terminal on stdin (debug builds: RPYENV_TEST_TTY=1 stands in for one).
fn at_terminal(get: &dyn Fn(&str) -> Option<String>) -> bool {
    std::io::stdin().is_terminal()
        || (cfg!(debug_assertions) && get("RPYENV_TEST_TTY").as_deref() == Some("1"))
}

/// Runs `program args` with `PATH` from `get` and no stdin; `(success, stdout, stderr)`, or
/// `None` when it can't start. Locale-neutral, so `sudo`'s English text can be matched.
fn run_probe(
    get: &dyn Fn(&str) -> Option<String>,
    program: &str,
    args: &[&str],
) -> Option<(bool, String, String)> {
    let o = std::process::Command::new(program)
        .args(args)
        .env("PATH", get("PATH").unwrap_or_default())
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    Some((
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    ))
}

/// CPython 2.x: the pre-M8 check, unchanged (plan ruling P2).
fn old_preflight(get: &dyn Fn(&str) -> Option<String>, patch: bool) -> Result<(), i32> {
    let missing = preflight::check(get, patch);
    let os = std::fs::read_to_string("/etc/os-release").ok();
    let r = preflight::report(
        &missing,
        os.as_deref(),
        get("USER").as_deref() == Some("root"),
    );
    for l in &r.lines {
        say(l);
    }
    if r.refuse {
        Err(1)
    } else {
        Ok(())
    }
}

/// What `resolve_deps` settled. `cont`: the prompt's `[c]`, so the builder warns at the end
/// instead of failing. `store`: the build store's entries to link (`[b]`, `--deps=build`).
struct Resolution {
    cont: bool,
    store: Vec<store::Entry>,
}

/// Builds catalog keys into the store (`store::ensure` with the job's setup).
type StoreBuild<'a> = dyn FnMut(&[&'static str], &mut dyn FnMut(&str)) -> Result<Vec<store::Entry>, InstallError>
    + 'a;

/// M8 design §5: detect, then print (system), install (install) or build into the store
/// (build) until nothing is missing. `Ok` when the build may start; `Err(exit code)` otherwise.
fn resolve_deps(
    get: &dyn Fn(&str) -> Option<String>,
    version: &str,
    minor: u32,
    patch: bool,
    flag: Option<&str>,
    root: &Path,
    build: &mut StoreBuild,
) -> Result<Resolution, i32> {
    use crate::install::deps::{self, Route, SudoUse};
    use crate::install::Reply;
    // The flag wins over the variable; a bad value names where it came from (review M10).
    let given = match flag {
        Some(v) => Some(("--deps", v.to_string())),
        None => get("RPYENV_BUILD_DEPS")
            .filter(|v| !v.is_empty())
            .map(|v| ("RPYENV_BUILD_DEPS", v)),
    };
    let chosen = match given {
        Some((source, v)) => match Route::parse(&v) {
            Some(r) => Some(r),
            None => {
                say(&format!(
                    "pyenv: {source} takes system, install, build or prebuilt"
                ));
                return Err(1);
            }
        },
        None => None,
    };
    if chosen == Some(Route::Prebuilt) {
        say("pyenv: the prebuilt route comes in a later rpyenv release");
        return Err(1);
    }
    let os_path = if cfg!(debug_assertions) {
        get("RPYENV_TEST_OS_RELEASE")
    } else {
        None
    }
    .unwrap_or_else(|| "/etc/os-release".into());
    let os = std::fs::read_to_string(os_path).ok();
    let manager = os.as_deref().and_then(deps::Manager::from_os_release);
    let tty = at_terminal(get);
    // An explicit route wins at a terminal too (ruling R9): no prompt, but no `-y` or
    // `sudo -n` either, because apt and sudo can ask the user themselves there.
    let interactive = tty && chosen.is_none();
    let is_root = run_probe(get, "id", &["-u"]).is_some_and(|(_, out, _)| out.trim() == "0");
    // RHEL family: CRB must be on before dnf can find some packages (ruling R15).
    let crb = os.as_deref().and_then(|o| deps::crb_line(o, is_root));
    // Package lists: apt can't find a package on a minimal image without them (§5.3).
    let lists = if cfg!(debug_assertions) {
        get("RPYENV_TEST_APT_LISTS")
    } else {
        None
    }
    .unwrap_or_else(|| "/var/lib/apt/lists".into());
    let apt_update =
        manager == Some(deps::Manager::Apt) && !deps::apt_lists_present(Path::new(&lists));
    // `sudo` is probed only when an install may be offered or run.
    let mut sudo_probe: Option<(bool, Option<(bool, String)>)> = None;
    let mut installed = false;
    let mut last_failed = false;
    // The build route (M8 design §6): what the store supplied counts as present (ruling P6).
    let root_ok: Result<(), String> = if store::plain_path(root) {
        Ok(())
    } else {
        Err(format!(
            "PYENV_ROOT has a space or a shell character in its path ({})",
            root.display()
        ))
    };
    // The report names `--deps=build` only where that route can run (rulings P2, P5).
    let build_route = minor >= 7 && root_ok.is_ok();
    let mut tools: Option<preflight::Tools> = None;
    let mut supplied: Vec<&'static str> = Vec::new();
    let mut store_entries: Vec<store::Entry> = Vec::new();
    loop {
        let after = if installed && cfg!(debug_assertions) {
            get("RPYENV_TEST_PREFLIGHT_AFTER")
        } else {
            None
        };
        let probe_env = |k: &str| {
            if k == "RPYENV_TEST_PREFLIGHT" {
                after.clone().or_else(|| get(k))
            } else {
                get(k)
            }
        };
        let mut missing = preflight::check3(&probe_env, patch, minor);
        missing.retain(|d| !supplied.contains(&d.lib));
        // Ctrl+C killed the probing compilers, so `missing` may be false: no prompt, no
        // report and no install after it (final review M1).
        if interrupted() {
            return Err(130);
        }
        if missing.is_empty() {
            return Ok(Resolution {
                cont: false,
                store: store_entries,
            });
        }
        // `[c]`: only optional libraries are missing (M8a follow-up).
        let continue_offered = !missing.iter().any(|d| deps::is_hard(d.lib));
        // `[b]` and `--deps=build`: the build tools are probed only then, once (they spawn
        // perl and the compiler).
        let wants_plan = interactive || chosen == Some(Route::Build);
        if wants_plan && tools.is_none() {
            tools = Some(preflight::build_tools(get));
            // Ctrl+C killed the probes: what they found may be false (final review S7).
            if interrupted() {
                return Err(130);
            }
        }
        let plan = tools
            .filter(|_| wants_plan)
            .map(|t| deps::build_plan(&missing, minor, &t, root_ok.clone(), manager));
        let build_offered = plan.as_ref().is_some_and(|p| p.blocked.is_none());
        if (tty || chosen == Some(Route::Install)) && sudo_probe.is_none() {
            let has_sudo = run_probe(get, "sudo", &["--version"]).is_some();
            let sudo_n = if has_sudo {
                run_probe(get, "sudo", &["-n", "true"]).map(|(ok, _, err)| (ok, err))
            } else {
                None
            };
            sudo_probe = Some((has_sudo, sudo_n));
        }
        // The commands to offer: apt's `update` first when its lists are empty.
        let offer: Result<Vec<Vec<String>>, &'static str> = match (manager, &sudo_probe) {
            (None, _) => Err("this distribution isn't recognized"),
            (_, None) => Err("sudo was not probed"),
            (Some(m), Some((has_sudo, sudo_n))) => {
                match deps::classify_sudo(is_root, *has_sudo, sudo_n.clone()) {
                    SudoUse::Not(r) => Err(r),
                    SudoUse::Allowed if !tty && sudo_n.as_ref().is_some_and(|(ok, _)| !ok) => {
                        Err("sudo needs a password and there is no terminal to ask it")
                    }
                    _ if installed && last_failed => Err("the last install command failed"),
                    _ if installed => Err("the last install left these missing"),
                    s => {
                        let with_sudo = !matches!(s, SudoUse::Root);
                        let mut cmds = Vec::new();
                        if apt_update {
                            cmds.push(deps::apt_update_argv(with_sudo));
                        }
                        cmds.push(deps::install_argv(
                            m,
                            &deps::packages(m, &missing),
                            with_sudo,
                            !tty,
                        ));
                        Ok(cmds)
                    }
                }
            }
        };
        let route = if interactive {
            // The prompt shows the commands joined by `&&`, as a shell would run them.
            let shown = offer.clone().map(|cmds| {
                let mut one: Vec<String> = Vec::new();
                for (i, c) in cmds.iter().enumerate() {
                    if i > 0 {
                        one.push("&&".into());
                    }
                    one.extend(c.iter().cloned());
                }
                one
            });
            for l in deps::prompt_lines(
                version,
                &missing,
                manager,
                shown,
                plan.as_ref(),
                crb.as_deref(),
                continue_offered,
            ) {
                say(&l);
            }
            loop {
                match crate::install::prompt("> ") {
                    Reply::Interrupted => return Err(130),
                    Reply::Eof => return Err(1),
                    Reply::Line(l) => {
                        match deps::choice(&l, offer.is_ok(), build_offered, continue_offered) {
                            Some(r) => break r,
                            None => say(&deps::answer_hint(
                                offer.is_ok(),
                                build_offered,
                                continue_offered,
                            )),
                        }
                    }
                }
            }
        } else if let Some(r) = chosen {
            r
        } else {
            // D13, lenient mode: only the hard set stops the build; the rest warns and the
            // build goes on (the builder prints the end summary).
            let hard_missing = missing.iter().any(|d| deps::is_hard(d.lib));
            for l in preflight::report_deps_with(
                &missing,
                os.as_deref(),
                is_root,
                hard_missing,
                apt_update,
                build_route,
            ) {
                say(&l);
            }
            return if hard_missing {
                Err(1)
            } else {
                Ok(Resolution {
                    cont: false,
                    store: Vec::new(),
                })
            };
        };
        match (route, offer) {
            (Route::Build, _) => {
                let Some(p) = plan else {
                    return Err(1);
                };
                // Without the prompt, every module is required (D5): refuse before building
                // anything when one can't be built here (rulings P3-P5).
                if !interactive {
                    if let Some(why) = &p.blocked {
                        say(&format!("pyenv: --deps=build can't run: {why}"));
                        return Err(1);
                    }
                    if !p.not.is_empty() {
                        for (n, why) in &p.not {
                            say(&format!("pyenv: --deps=build can't build {n} here: {why}"));
                        }
                        return Err(1);
                    }
                }
                match build(&p.keys, &mut |s: &str| say(s)) {
                    Ok(e) => {
                        // A later round reuses ncurses or Tcl: each entry once (review S4).
                        store::add_unique(&mut store_entries, e);
                        supplied.extend(p.libs.iter().copied());
                    }
                    Err(InstallError::Interrupted) => return Err(130),
                    Err(InstallError::Message(m)) => {
                        say(&m);
                        return Err(1);
                    }
                    Err(InstallError::Failed) => return Err(1),
                }
            }
            // The builder warns at the end instead of rolling back (`RPYENV_DEPS_MODE=continue`).
            (Route::Continue, _) => {
                return Ok(Resolution {
                    cont: true,
                    store: store_entries,
                })
            }
            (Route::Install, Ok(cmds)) => {
                last_failed = false;
                for argv in cmds {
                    // Without a terminal, sudo must not ask (`-n`).
                    let argv: Vec<String> = if !tty && argv.first().is_some_and(|a| a == "sudo") {
                        let mut v = vec!["sudo".to_string(), "-n".to_string()];
                        v.extend(argv.into_iter().skip(1));
                        v
                    } else {
                        argv
                    };
                    say(&format!("pyenv: running: {}", argv.join(" ")));
                    let mut c = std::process::Command::new(&argv[0]);
                    c.args(&argv[1..])
                        .env("PATH", get("PATH").unwrap_or_default());
                    if !tty {
                        c.stdin(std::process::Stdio::null());
                    }
                    // A command that can't even start (not found, "Text file busy") says why.
                    let ok = match c.status() {
                        Ok(st) => {
                            crate::install::note_child_status(&st);
                            st.success()
                        }
                        Err(e) => {
                            say(&format!("pyenv: cannot run {}: {e}", argv[0]));
                            false
                        }
                    };
                    // Ctrl+C reached the command too: end the run before any hint.
                    if interrupted() {
                        return Err(130);
                    }
                    if !ok {
                        last_failed = true;
                        say("pyenv: the install command failed");
                        // The prompt showed the CRB line already; `--deps=install` did not.
                        if let (false, Some(l)) = (interactive, &crb) {
                            say(l);
                        }
                        if manager == Some(deps::Manager::Apt) && !apt_update {
                            say("pyenv: if apt couldn't find a package, run sudo apt-get update first");
                        }
                        if !interactive {
                            return Err(1);
                        }
                        break;
                    }
                }
                installed = true;
            }
            (Route::Install, Err(reason)) => {
                say(&format!("pyenv: can't install the packages: {reason}"));
                return Err(1);
            }
            _ => {
                // system: today's text, now listing every missing module.
                for l in preflight::report_deps_with(
                    &missing,
                    os.as_deref(),
                    is_root,
                    true,
                    apt_update,
                    build_route,
                ) {
                    say(&l);
                }
                return Err(1);
            }
        }
    }
}

pub fn install(ctx: &Ctx, args: &[&str]) -> Output {
    // The process environment, which every lookup and the build itself see.
    let base: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let env = |k: &str| builder::get(&base, k);
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        for opt in [
            "--bare",
            "--list",
            "--force",
            "--skip-existing",
            "--keep",
            "--patch",
            "--verbose",
            "--version",
            "--debug",
            "--deps=",
        ] {
            o.out(opt);
        }
        for n in defs::names(&ctx.root, &env) {
            o.out(n);
        }
        return o;
    }
    let (mut f, positional) = match parse(args) {
        Ok(v) => v,
        Err(Early::Help) => {
            return Output {
                stdout: HELP.into(),
                ..Output::new()
            }
        }
        Err(Early::Usage) => {
            return Output {
                stderr: HELP.into(),
                code: 1,
                ..Output::new()
            }
        }
        Err(Early::Version) => {
            let mut o = Output::new();
            o.out(format!(
                "python-build {} (rpyenv {})",
                defs::UPSTREAM_VERSION,
                env!("CARGO_PKG_VERSION")
            ));
            return o;
        }
    };
    if env("PYENV_DEBUG").is_some_and(|v| !v.is_empty()) {
        f.verbose = true;
    }
    let names = defs::names(&ctx.root, &env);
    if f.list {
        let mut o = Output::new();
        if !f.bare {
            o.out("Available versions:");
        }
        for n in &names {
            o.out(if f.bare { n.clone() } else { format!("  {n}") });
        }
        return o;
    }
    let wanted: Vec<String> = if positional.is_empty() {
        verfile::find_local(&ctx.pwd)
            .map(|file| {
                verfile::read_pyenv(&file, &file.display().to_string(), &ctx.versions_dir())
                    .versions
            })
            .unwrap_or_default()
    } else {
        positional.iter().map(|s| s.to_string()).collect()
    };
    if wanted.is_empty() {
        return Output {
            stderr: HELP.into(),
            code: 1,
            ..Output::new()
        };
    }
    watch_interrupt();
    // Reads the two small pages a conda `-latest` name needs; no cache.
    let probe = Fetcher::from_env(&env, None);
    // M8 design §6.3 and §6.5: once per run, before any version is installed.
    // The shared Tcl/Tk script folders venvs read (§6.3), when one was deleted.
    crate::install::record::heal_scripts(&ctx.root);
    if let Ok(rows) = catalog::table(&env) {
        let now = |lib: &str, def: &str, origin: catalog::Origin| -> Option<String> {
            if origin == catalog::Origin::Definition {
                // A definition that can't be found or read: unknown, not the catalog's version
                // (final review S2).
                let d = defs::find(&ctx.root, def, &env).and_then(|found| {
                    defs::parse(&found, &env, &|n| defs::find(&ctx.root, n, &env)).ok()
                })?;
                if let Some(r) = catalog::pinned(lib, &d) {
                    return Some(r.version);
                }
            }
            rows.iter()
                .find(|r| r.key == lib)
                .map(|r| r.version.clone())
        };
        for l in crate::install::record::stale_lines(&ctx.versions_dir(), &now) {
            say(&l);
        }
        // The private OPENSSLDIR follows the system's certificates (D-127).
        let bundle = store::ssl_dir(&ctx.root).join("cert.pem");
        if store::ssl_dir(&ctx.root).is_dir() {
            let version = rows
                .iter()
                .find(|r| r.key == "cacert")
                .map(|r| r.version.clone())
                .unwrap_or_default();
            match store::sync_ssl_dir(
                &ctx.root,
                &store::detect_system_certs(),
                &rows,
                &probe,
                &mut |s: &str| say(s),
            ) {
                Ok(true) => say(&format!(
                    "pyenv: updated the CA bundle in {} ({version})",
                    bundle.display()
                )),
                Ok(false) => {}
                Err(InstallError::Message(m)) => {
                    say(&format!("pyenv: warning: cannot update the CA bundle: {m}"))
                }
                Err(_) => say(&format!(
                    "pyenv: warning: cannot update the CA bundle in {}",
                    bundle.display()
                )),
            }
        }
    }
    let mut status = 0;
    let mut stdin_patch: Option<Vec<u8>> = None;
    for arg in &wanted {
        // A Ctrl+C during the previous version's default packages or rehash.
        if interrupted() {
            return Output::new().with_code(130);
        }
        // Alias: the text after the last `:`, unless it is `latest`. An empty alias is no
        // alias (`${VERSION_ALIAS:-$VERSION_NAME}`).
        let (mut definition, alias) = match arg.rsplit_once(':') {
            Some((d, a)) if a != "latest" => {
                (d.to_string(), (!a.is_empty()).then(|| a.to_string()))
            }
            _ => (arg.clone(), None),
        };
        if let Some(prefix) = definition.strip_suffix(":latest") {
            match resolve_known(prefix, &names) {
                Some(v) => definition = v,
                None => {
                    say(&format!(
                        "pyenv: no known versions match the prefix `{prefix}'"
                    ));
                    status = status.max(1);
                    break;
                }
            }
        } else if let Some(v) = resolve_known(&definition, &names) {
            definition = v;
        }
        let base_name = Path::new(&definition)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let version_name = if f.debug {
            format!("{base_name}-debug")
        } else {
            base_name
        };
        let name = alias.unwrap_or(version_name);
        if invalid_name(&name) {
            say(&format!("pyenv: invalid version name: {name}"));
            status = status.max(1);
            break;
        }
        let prefix = ctx.versions_dir().join(&name);
        if is_complete(&prefix) {
            if f.skip {
                continue;
            }
            if !f.force {
                say(&format!("pyenv: {} already exists", prefix.display()));
                match prompt("continue with installation? (y/N) ") {
                    Reply::Interrupted => return Output::new().with_code(130),
                    Reply::Eof => return Output::new().with_code(1),
                    Reply::Line(r) if ["y", "Y", "yes", "YES"].contains(&r.as_str()) => {}
                    Reply::Line(_) => {
                        status = status.max(1);
                        continue;
                    }
                }
            }
        }
        let Some(found) = defs::find(&ctx.root, &definition, &env) else {
            not_found(&definition, &names);
            status = status.max(2);
            break;
        };
        let root = ctx.root.clone();
        let here = match &found.origin {
            Origin::Path(p) => p.parent().map(Path::to_path_buf),
            _ => None,
        };
        let sibling = |n: &str| match &here {
            Some(dir) => defs::find(&root, &dir.join(n).display().to_string(), &env),
            None => defs::find(&root, n, &env),
        };
        let first = defs::parse(&found, &env, &sibling);
        let (found, parsed) = match pypy_text(&found, first.is_ok())
            .or_else(|| conda_text(&found, first.is_ok(), &probe, &mut say))
        {
            None => match first {
                Ok(d) => (found, d),
                Err(e) => {
                    say(&e);
                    status = status.max(1);
                    break;
                }
            },
            Some(Err(m)) => {
                say(&m);
                status = status.max(1);
                break;
            }
            Some(Ok(text)) => {
                let f = Found { text, ..found };
                match defs::parse(&f, &env, &sibling) {
                    Ok(d) => (f, d),
                    Err(e) => {
                        say(&e);
                        status = status.max(1);
                        break;
                    }
                }
            }
        };
        // `-k`, or a non-empty PYENV_BUILD_ROOT: the build happens in (and is kept at)
        // `<build root>/<name>` (reference "Per-version loop", step 5).
        let build_root: Option<PathBuf> = get_os(&base, "PYENV_BUILD_ROOT")
            .map(PathBuf::from)
            .or_else(|| f.keep.then(|| ctx.root.join("sources")));
        let mut job_env = base.clone();
        if let Some(r) = &build_root {
            job_env.push(("PYTHON_BUILD_BUILD_PATH".into(), r.join(&name).into()));
        }
        // M8 design D13: a run with no terminal and neither `--deps` nor RPYENV_BUILD_DEPS is
        // lenient; the builder reads the mode to print its end summary.
        let strict = at_terminal(&env)
            || f.deps.is_some()
            || env("RPYENV_BUILD_DEPS").is_some_and(|v| !v.is_empty());
        // The mode is rpyenv's own: one set by hand must not make a run lenient (review M8).
        job_env.retain(|(k, _)| k != "RPYENV_DEPS_MODE");
        if !strict {
            job_env.push(("RPYENV_DEPS_MODE".into(), "lenient".into()));
        }
        let job_get = |k: &str| builder::get(&job_env, k);
        // python-build reads a cache only when it is a directory; `pyenv install` supplies
        // `$PYENV_ROOT/cache` when PYTHON_BUILD_CACHE_PATH is empty (reference "Cache"). The
        // store downloads with the same mirror and cache settings.
        let cache = match get_os(&job_env, "PYTHON_BUILD_CACHE_PATH") {
            Some(p) => Some(PathBuf::from(p)),
            None => Some(ctx.root.join("cache")),
        }
        .filter(|c| c.is_dir());
        let fetcher = Fetcher::from_env(&job_get, cache);
        // A binary install (PyPy) compiles nothing, so it needs no build dependencies.
        let compiles = parsed
            .packages
            .iter()
            .any(|p| p.steps.iter().any(|s| s == "standard" || s == "python"));
        let minor = parsed
            .packages
            .iter()
            .flat_map(|p| p.steps.iter())
            .find_map(|s| crate::install::stdlib::minor_of_step(s));
        let mut store_entries: Vec<store::Entry> = Vec::new();
        if compiles && job_get("RPYENV_SKIP_PREFLIGHT").as_deref() != Some("1") {
            let patch = f.patch || needs_patch(&found, &parsed);
            let abs_root = std::path::absolute(&ctx.root).unwrap_or_else(|_| ctx.root.clone());
            let snapshot = job_env.clone();
            let mut build = |keys: &[&'static str],
                             say: &mut dyn FnMut(&str)|
             -> Result<Vec<store::Entry>, InstallError> {
                let rows = catalog::table(&|k: &str| builder::get(&snapshot, k))
                    .map_err(InstallError::Message)?;
                let openssldir = store::ssl_dir(&abs_root);
                store::ensure(
                    &store::Build {
                        root: &abs_root,
                        rows: &rows,
                        definition: &parsed,
                        env: &snapshot,
                        fetcher: &fetcher,
                        openssldir,
                        certs: store::detect_system_certs(),
                        verbose: f.verbose,
                    },
                    keys,
                    say,
                )
            };
            let ok = match minor {
                Some(m) => resolve_deps(
                    &job_get,
                    &name,
                    m,
                    patch,
                    f.deps.as_deref(),
                    &abs_root,
                    &mut build,
                ),
                // CPython 2.x keeps today's check (plan ruling P2 of M8a).
                None => old_preflight(&job_get, patch).map(|()| Resolution {
                    cont: false,
                    store: Vec::new(),
                }),
            };
            match ok {
                Err(code) => {
                    if code == 130 {
                        return Output::new().with_code(130);
                    }
                    status = status.max(code);
                    break;
                }
                Ok(r) => {
                    // `[c]`: build without the optional libraries; the builder warns at the end.
                    if r.cont {
                        job_env.retain(|(k, _)| k != "RPYENV_DEPS_MODE");
                        job_env.push(("RPYENV_DEPS_MODE".into(), "continue".into()));
                    }
                    store_entries = r.store;
                }
            }
        }
        let linked = (!store_entries.is_empty()).then(|| {
            store::link(store_entries, minor.unwrap_or(u32::MAX), &|p: &Path| {
                std::fs::read_to_string(p).ok()
            })
        });
        let mut txn = match Txn::begin(&ctx.versions_dir(), &name) {
            Ok(t) => t,
            Err(m) => {
                say(&m);
                status = status.max(1);
                break;
            }
        };
        if f.patch && stdin_patch.is_none() {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            stdin_patch = Some(buf);
        }
        let job = Job {
            found: &found,
            definition: &parsed,
            prefix: prefix.clone(),
            opts: Options {
                keep: build_root.is_some(),
                verbose: f.verbose,
                debug: f.debug,
                stdin_patch: stdin_patch.take(),
            },
            env: &job_env,
            fetcher: &fetcher,
            store: linked.as_ref(),
        };
        match builder::run(&job, &mut txn, &mut |s: &str| say(s)) {
            Ok(()) => {
                // A Ctrl+C after `run` returned but before the commit still rolls back.
                if interrupted() {
                    drop(txn);
                    return Output::new().with_code(130);
                }
                if let Err(e) = txn.commit() {
                    say(&format!(
                        "pyenv: cannot finish installing {}: {e}",
                        prefix.display()
                    ));
                    status = status.max(1);
                    break;
                }
                if let Some(line) = default_packages::run(&ctx.root, &prefix) {
                    say(&line);
                }
                let r = crate::commands::rehash::rehash(ctx, &[]);
                // A failed rehash ends the run, as upstream's `set -e` does.
                if r.code != 0 {
                    r.emit(ctx.flavor);
                    status = status.max(r.code);
                    break;
                }
            }
            Err(InstallError::Interrupted) => {
                drop(txn);
                return Output::new().with_code(130);
            }
            Err(e) => {
                // `run` prints everything once the build has started; an early message
                // is left to the caller.
                if let InstallError::Message(m) = e {
                    say(&m);
                }
                drop(txn);
                status = status.max(1);
                break;
            }
        }
    }
    if interrupted() {
        return Output::new().with_code(130);
    }
    Output::new().with_code(status)
}
