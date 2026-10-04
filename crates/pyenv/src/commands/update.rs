//! `pyenv update [--ignore]` (pyenv-win flavor; docs/parity/pyenv-win-m2-reference.md
//! "update"): rewrites `<root>\.versions_cache.xml` from python.org directly (spec §9.1,
//! plan M2b Decision 3).

use crate::install::fetch::{Fetcher, TextError};
use crate::install::wincatalog::write_db;
use crate::install::winsource::{
    banner, base, catalog, folder_rows, index_zips, listing_names, mirror, version_folders,
};
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
    if let Err(e) = write_db(&ctx.root, &rows) {
        o.out(format!(
            "pyenv: cannot write {}: {e}",
            ctx.root.join(crate::install::wincatalog::DB_NAME).display()
        ));
        return o.with_code(1);
    }
    o.out(format!(
        ":: [Info] ::  Scanned {pages} pages and found {} installers.",
        rows.len()
    ));
    o
}
