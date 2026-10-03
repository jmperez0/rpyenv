//! `pyenv install` (docs/parity/pyenv-m2-reference.md "pyenv install"), Linux flavor.

use crate::commands::latest::resolve_known;
use crate::install::builder::{self, patch_files, Job, Options};
use crate::install::defs::{self, Found, Origin};
use crate::install::fetch::Fetcher;
use crate::install::txn::{is_complete, Txn};
use crate::install::{
    default_packages, interrupted, preflight, prompt, watch_interrupt, InstallError, Reply,
};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::verfile;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n       pyenv install [-f] [-kvp] <definition-file>[:<alias>]\n       pyenv install -l|--list [--bare]\n       pyenv install --version\n\n  -l/--list          List all available versions\n  -f/--force         Install even if the version appears to be installed already\n  -s/--skip-existing Skip if the version appears to be installed already\n\n  python-build options:\n\n  -k/--keep          Keep source tree in $PYENV_BUILD_ROOT after installation\n                     (defaults to $PYENV_ROOT/sources)\n  -p/--patch         Apply a patch from stdin before building\n  -v/--verbose       Verbose mode: print compilation status to stdout\n  --version          Show version of python-build\n  -g/--debug         Build a debug version\n\n  Append `:<alias>' to a version to install it under a custom name, so that\n  several builds of the same version can coexist:\n\n      pyenv install 3.12.0:my-3.12\n\n  This installs into $PYENV_ROOT/versions/my-3.12.\n\nFor detailed information on installing Python versions with\npython-build, including a list of environment variables for adjusting\ncompilation, see: https://github.com/pyenv/pyenv#readme\n\n";
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

/// A name that can't be a directory directly under `versions/`.
fn invalid_name(name: &str) -> bool {
    name.is_empty() || name == "." || name == ".." || name.contains('/')
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
        let parsed = match defs::parse(&found, &env, &sibling) {
            Ok(d) => d,
            Err(e) => {
                say(&e);
                status = status.max(1);
                break;
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
        let job_get = |k: &str| builder::get(&job_env, k);
        if job_get("RPYENV_SKIP_PREFLIGHT").as_deref() != Some("1") {
            let missing = preflight::check(&job_get, f.patch || needs_patch(&found, &parsed));
            let os = std::fs::read_to_string("/etc/os-release").ok();
            let r = preflight::report(
                &missing,
                os.as_deref(),
                job_get("USER").as_deref() == Some("root"),
            );
            for l in &r.lines {
                say(l);
            }
            if r.refuse {
                status = status.max(1);
                break;
            }
        }
        let mut txn = match Txn::begin(&ctx.versions_dir(), &name) {
            Ok(t) => t,
            Err(m) => {
                say(&m);
                status = status.max(1);
                break;
            }
        };
        // python-build reads a cache only when it is a directory; `pyenv install` supplies
        // `$PYENV_ROOT/cache` when PYTHON_BUILD_CACHE_PATH is empty (reference "Cache").
        let cache = match get_os(&job_env, "PYTHON_BUILD_CACHE_PATH") {
            Some(p) => Some(PathBuf::from(p)),
            None => Some(ctx.root.join("cache")),
        }
        .filter(|c| c.is_dir());
        let fetcher = Fetcher::from_env(&job_get, cache);
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
