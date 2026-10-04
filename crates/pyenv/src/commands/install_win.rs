//! `pyenv install` for the pyenv-win flavor (docs/parity/pyenv-win-m2-reference.md "install";
//! plan M2b Decisions 10–12). Lines stream to stdout as they happen, in CRLF.

use crate::install::fetch::Fetcher;
use crate::install::wincatalog::{parse_code, read_db, DbError, Row, DB_NAME};
use crate::install::winpkg::{self, Job};
use crate::install::winsource::{banner, base};
use crate::install::{interrupted, watch_interrupt, InstallError};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{installed, select, winresolve};

pub const HELP: &str = "Usage: pyenv install [-s] [-f] <version> [<version> ...] [-r|--register]\n       pyenv install [-f] [--32only|--64only] -a|--all\n       pyenv install [-f] -c|--clear\n       pyenv install -l|--list\n\n  -l/--list              List all available versions\n  -a/--all               Installs all known version from the local version DB cache\n  -c/--clear             Removes downloaded installers from the cache to free space\n  -f/--force             Install even if the version appears to be installed already\n  -s/--skip-existing     Skip the installation if the version appears to be installed already\n  -r/--register          Register version for py launcher\n  -q/--quiet             Install using /quiet. This does not show the UI nor does it prompt for inputs\n  --32only               Installs only 32bit Python using -a/--all switch, no effect on 32-bit windows.\n  --64only               Installs only 64bit Python using -a/--all switch, no effect on 32-bit windows.\n  --dev                  Installs precompiled standard libraries, debug symbols, and debug binaries (only applies to web installer).\n  --help                 Help, list of options allowed on pyenv install\n\n";

/// One line on stdout, CRLF, flushed (pyenv-win prints everything on stdout).
fn say(line: &str) {
    rpyenv_core::textout::write(false, &format!("{line}\r\n"));
}

/// `\n`-separated text, each line CRLF.
fn say_text(text: &str) {
    rpyenv_core::textout::write(false, &text.replace('\n', "\r\n"));
}

#[derive(Default)]
struct Opts {
    list: bool,
    force: bool,
    all: bool,
    clear: bool,
    only32: bool,
    only64: bool,
    register: bool,
}

/// pyenv-win's `Check32Bit`: on a 32-bit host every name gets `-win32` unless it has it.
fn check32(code: &str, arch_suffix: &str) -> String {
    if arch_suffix == "-win32" && !code.to_ascii_lowercase().ends_with("-win32") {
        format!("{code}-win32")
    } else {
        code.to_string()
    }
}

/// `-c`: every file, then every folder, of `install_cache` (reference "--clear"). A missing
/// folder is not an error (allowlist).
fn clear(ctx: &Ctx) -> i32 {
    let cache = ctx.root.join("install_cache");
    let Ok(rd) = std::fs::read_dir(&cache) else {
        return 0;
    };
    let entries: Vec<std::path::PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    let mut status = 0;
    for p in entries.iter().filter(|p| !p.is_dir()) {
        if let Err(e) = std::fs::remove_file(p) {
            say(&format!("pyenv: Error deleting file {}: {e}", p.display()));
            status = 1;
        }
    }
    for p in entries.iter().filter(|p| p.is_dir()) {
        if let Err(e) = std::fs::remove_dir_all(p) {
            say(&format!(
                "pyenv: Error deleting folder {}: {e}",
                p.display()
            ));
            status = 1;
        }
    }
    status
}

pub fn install(ctx: &Ctx, args: &[&str]) -> Output {
    say_text(&banner());
    let db = read_db(&ctx.root);
    let codes: Vec<String> = match &db {
        Ok(rows) => rows.iter().map(|r| r.code.clone()).collect(),
        Err(_) => Vec::new(),
    };
    let mut o = Opts::default();
    let mut wanted: Vec<String> = Vec::new();
    for a in args {
        match *a {
            "--help" => {
                say_text(HELP);
                return Output::new();
            }
            "-l" | "--list" => o.list = true,
            "-f" | "--force" => o.force = true,
            // Accepted with no effect, as in pyenv-win (reference "Options").
            "-s" | "--skip-existing" | "-q" | "--quiet" | "--dev" => {}
            "-a" | "--all" => o.all = true,
            "-c" | "--clear" => o.clear = true,
            "--32only" => o.only32 = true,
            "--64only" => o.only64 = true,
            "-r" | "--register" => o.register = true,
            v => {
                let r = winresolve::resolve(v, &codes, ctx.arch_suffix);
                if !wanted.contains(&r) {
                    wanted.push(r);
                }
            }
        }
    }
    if ctx.arch_suffix == "-win32" {
        o.only32 = false;
        o.only64 = false;
    }
    let fail = |m: &str| {
        say(m);
        Output::new().with_code(1)
    };
    if o.only32 && o.only64 {
        return fail("pyenv-install: only --32only or --64only may be specified, not both.");
    }
    if o.register && o.only32 {
        return fail("pyenv-install: --register not supported for 32 bits.");
    }
    if o.register && o.all {
        return fail("pyenv-install: --register not supported for all versions.");
    }
    let rows: Vec<Row> = match db {
        Ok(r) => r,
        Err(DbError::Missing | DbError::Empty) => {
            say("pyenv-install: no definitions in local database");
            say("");
            return fail("Please update the local database cache with `pyenv update'.");
        }
        Err(DbError::Malformed(m)) => {
            return fail(&format!(
                "pyenv-install: cannot read {}: {m}",
                ctx.root.join(DB_NAME).display()
            ));
        }
    };
    if o.list {
        for r in &rows {
            say(&r.code);
        }
        return Output::new();
    }
    if o.clear {
        return Output::new().with_code(clear(ctx));
    }
    if o.all {
        wanted = rows
            .iter()
            .filter(|r| (!o.only64 || r.x64) && (!o.only32 || !r.x64))
            .map(|r| check32(&r.code, ctx.arch_suffix))
            .filter(|c| codes.contains(c))
            .fold(Vec::new(), |mut v, c| {
                if !v.contains(&c) {
                    v.push(c);
                }
                v
            });
    } else if wanted.is_empty() {
        match select::win_select(ctx).first() {
            Some(s) => wanted.push(winresolve::resolve(&s.name, &codes, ctx.arch_suffix)),
            None => {
                say_text(HELP);
                return Output::new();
            }
        }
    }
    if let Some(missing) = wanted.iter().find(|w| !codes.contains(w)) {
        say(&format!("pyenv-install: definition not found: {missing}"));
        say("");
        say("See all available versions with `pyenv install --list`.");
        return fail("Does the list seem out of date? Update it using `pyenv update`.");
    }
    watch_interrupt();
    let fetcher = Fetcher::direct();
    let base = base();
    let mut status = 0;
    for w in &wanted {
        if interrupted() {
            return Output::new().with_code(130);
        }
        let row = rows.iter().find(|r| &r.code == w).expect("checked above");
        let code = parse_code(w).filter(|_| row.zip_root_dir.is_none());
        let Some(code) = code.filter(|_| !installed::is_staging_name(w)) else {
            say(&format!(
                ":: [Error] :: rpyenv cannot install {w} yet: only CPython is supported."
            ));
            status = 1;
            break;
        };
        let job = Job {
            root: &ctx.root,
            code: &code,
            force: o.force,
            base: &base,
            fetcher: &fetcher,
        };
        match winpkg::install(&job, &mut |l| say(l)) {
            Ok(winpkg::Done::Installed) => {
                let prefix = ctx.versions_dir().join(w);
                if let Some(line) = crate::install::default_packages::run(&ctx.root, &prefix) {
                    say(&line);
                }
            }
            Ok(winpkg::Done::Skipped) => {}
            Err(InstallError::Interrupted) => return Output::new().with_code(130),
            Err(e) => {
                if let InstallError::Message(m) = e {
                    say(&format!(":: [Error] :: {m}"));
                }
                say(&format!(":: [Error] :: couldn't install {w}"));
                status = 1;
                break;
            }
        }
    }
    if interrupted() {
        return Output::new().with_code(130);
    }
    if o.register {
        say(":: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.");
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    if r.code != 0 {
        r.emit(ctx.flavor);
        status = status.max(r.code);
    }
    Output::new().with_code(status)
}
