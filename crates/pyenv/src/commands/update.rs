//! `pyenv update [--ignore]` (pyenv-win flavor; docs/parity/pyenv-win-m2-reference.md
//! "update"): rewrites `<root>\.versions_cache.xml` from python.org directly (spec §9.1,
//! plan M2b Decision 3).

use crate::install::fetch::{Fetcher, TextError};
use crate::install::pypy;
use crate::install::wincatalog::{read_db, write_db};
use crate::install::winsource::{
    banner, base, catalog, folder_rows, index_zips, listing_names, mirror, pypy_rows,
    version_folders,
};
use crate::install::{conda, condacatalog};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;

pub const HELP: &str = "Usage: pyenv update [--ignore]\n\n  --ignore  Ignores any HTTP/VBScript errors that occur during downloads.\n\nUpdates the internal database of python installer URL's.\n\n";

fn report(o: &mut Output, what: &str, url: &str, e: &TextError) {
    o.out(format!("HTTP Error downloading from {what} \"{url}\""));
    match e.status {
        Some(s) => o.out(format!("Error({s}): {}", e.message)),
        None => o.out(format!("Error: {}", e.message)),
    }
}

/// Folder listings, 8 at a time, in `folders` order.
fn fetch_all(f: &Fetcher, src: &str, folders: &[String]) -> Vec<Result<String, TextError>> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<String, TextError>>>> = folders
        .iter()
        .map(|_| std::sync::Mutex::new(None))
        .collect();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(folder) = folders.get(i) else { break };
                let r = f.get_text(&format!("{src}/{folder}/"));
                *results[i].lock().unwrap() = Some(r);
            });
        }
    });
    results
        .into_iter()
        .map(|m| m.into_inner().unwrap().expect("every folder was fetched"))
        .collect()
}

pub fn update(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    o.stdout.push_str(&banner());
    let ignore = match args.first() {
        Some(&"--help") => {
            o.stdout.push_str(HELP);
            return o;
        }
        Some(&"--ignore") => true,
        _ => false,
    };
    let src = mirror().unwrap_or_else(base);
    let src = src.trim_end_matches('/').to_string();
    let f = Fetcher::direct();
    let root_url = format!("{src}/");
    let names = match f.get_text(&root_url) {
        Ok(t) => listing_names(&t),
        Err(e) => {
            report(&mut o, "mirror", &root_url, &e);
            return o.with_code(if ignore { 0 } else { 1 });
        }
    };
    let folders = version_folders(&names);
    let mut rows = Vec::new();
    let mut pages = 0;
    for (folder, r) in folders.iter().zip(fetch_all(&f, &src, &folders)) {
        match r {
            Ok(t) => {
                pages += 1;
                rows.extend(folder_rows(&src, folder, &listing_names(&t)));
            }
            Err(e) => {
                report(&mut o, "mirror page", &format!("{src}/{folder}/"), &e);
                if !ignore {
                    return o.with_code(1);
                }
            }
        }
    }
    match index_zips(&f, &src) {
        Ok((zips, n)) => {
            pages += n;
            rows = catalog(rows, &zips);
        }
        Err((url, e)) => {
            report(&mut o, "mirror", &url, &e);
            return o.with_code(if ignore { 0 } else { 1 });
        }
    }
    // PyPy's list, as pyenv-win reads it, unless a mirror replaces python.org (pyenv-win's
    // banner then names only the mirror). M7a design §5.1. A failed read never blocks the
    // python.org list (ruling R3): the PyPy rows already in the cache are kept, with a warning.
    if mirror().is_none() {
        let url = format!("{}/versions.json", pypy::urls().0);
        match f
            .get_text(&url)
            .map_err(|e| (e.status, e.message))
            .and_then(|t| pypy_rows(&t).map_err(|m| (None, m)))
            .and_then(|r| {
                // Parsed but no win64 zip: pypy.org reshaped the file; treat it as a failed read.
                if r.is_empty() {
                    Err((
                        None,
                        "versions.json lists no Windows PyPy zips: has its layout changed?"
                            .to_string(),
                    ))
                } else {
                    Ok(r)
                }
            }) {
            Ok(r) => {
                pages += 1;
                rows.extend(r);
            }
            Err((status, message)) => {
                report(&mut o, "mirror", &url, &TextError { status, message });
                match read_db(&ctx.root) {
                    Ok(old) => {
                        let kept: Vec<_> = old
                            .into_iter()
                            .filter(|r| pypy::win_code(&r.code).is_some())
                            .collect();
                        o.out(format!(
                            ":: [Warning] :: PyPy's list couldn't be read; kept the {} PyPy versions already listed.",
                            kept.len()
                        ));
                        rows.extend(kept);
                    }
                    Err(_) => o.out(
                        ":: [Warning] :: PyPy's list couldn't be read, and no earlier list could be read either; no PyPy versions are listed.",
                    ),
                }
            }
        }
    }
    if let Err(e) = write_db(&ctx.root, &rows) {
        o.out(format!(
            "pyenv: cannot write {}: {e}",
            ctx.root.join(crate::install::wincatalog::DB_NAME).display()
        ));
        return o.with_code(1);
    }
    // M7c design §4.3: the conda table, rebuilt from the publishers. A failed read keeps the
    // previous table whole and warns (D9); a changed hash is kept pinned and warns (D8).
    // Neither fails the command: the user can't act on them. `update` is pyenv-win only
    // (commands/mod.rs WIN_ONLY), so the Linux flavor never reaches this (D7). Unlike PyPy,
    // a python.org mirror doesn't skip it: pyenv-win has no conda banner to match.
    match condacatalog::read_published(&f, &condacatalog::sources()) {
        Err(e) => o.out(format!(
            ":: [Warning] :: conda: {e}; kept the previous table"
        )),
        Ok(p) => {
            // D8: the merged table (reviewed rows win) is both the base and the pin baseline.
            let (base, previous) = match conda::windows_table(&ctx.root) {
                Ok(t) => {
                    for w in &t.warnings {
                        o.out(format!(":: [Warning] :: conda: {w}"));
                    }
                    (condacatalog::refresh(&t.rows, &p), t.rows)
                }
                Err(_) => match conda::linux_table() {
                    Ok(l) => (condacatalog::from_linux(&l, &p), Vec::new()),
                    Err(e) => {
                        o.out(format!(":: [Warning] :: conda: {e}"));
                        return o.with_code(1);
                    }
                },
            };
            let mut rows = base;
            let changed = condacatalog::pin_hashes(&previous, &mut rows, &[]);
            let added = condacatalog::add_newer(&mut rows, &p);
            let path = ctx.root.join(conda::WIN_TABLE);
            let tmp = ctx.root.join(format!("{}.tmp", conda::WIN_TABLE));
            let text = condacatalog::render(&rows, "update", &condacatalog::now());
            if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
                let _ = std::fs::remove_file(&tmp);
                o.out(format!("pyenv: cannot write {}: {e}", path.display()));
                return o.with_code(1);
            }
            let installable = rows
                .iter()
                .filter(|r| {
                    matches!(
                        r.verdict,
                        conda::Verdict::Ok(_) | conda::Verdict::OkLatest(_)
                    )
                })
                .count();
            o.out(format!(
                ":: [Info] ::  conda: {installable} installable versions ({added} new)"
            ));
            if !changed.is_empty() {
                // D8: a warning, exit 0: the user can't act on it; a release can.
                o.out(format!(
                    ":: [Warning] :: conda: the publisher changed {} after rpyenv pinned them; they won't install until an rpyenv release carries the reviewed hash",
                    changed.join(" ")
                ));
            }
        }
    }
    o.out(format!(
        ":: [Info] ::  Scanned {pages} pages and found {} installers.",
        rows.len()
    ));
    o
}
