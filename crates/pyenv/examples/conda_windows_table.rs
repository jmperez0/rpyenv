//! CI's Windows conda table (M7c design §4.2): `cargo run -p pyenv --example
//! conda_windows_table -- <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]`. Reads
//! the publishers, rebuilds <windows.tsv> from <linux.tsv>'s x86_64 rows, keeps every pinned
//! hash (§4.2 step 6) except the rows a maintainer accepts (§4.2 step 7, D8).
//! Exit 0: written. 1: written, but some rows changed hash or stopped installing (named on
//! stderr, starting `changed hash, kept the pinned one:` / `no longer installable:`).
//! 2: the publishers couldn't be read; nothing written.
use pyenv::install::{conda, condacatalog as cc, fetch::Fetcher};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, accept): (Vec<String>, Vec<String>) =
        match args.iter().position(|a| a == "--accept") {
            Some(i) => (
                args[..i].to_vec(),
                args.get(i + 1)
                    .map(|v| v.split(',').map(str::to_string).collect())
                    .unwrap_or_default(),
            ),
            None => (args.clone(), Vec::new()),
        };
    let [linux_path, win_path] = paths.as_slice() else {
        eprintln!(
            "usage: conda_windows_table <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]"
        );
        std::process::exit(2);
    };
    let linux = conda::parse_linux(&std::fs::read_to_string(linux_path).expect("linux.tsv"))
        .expect("linux.tsv parses");
    let previous = std::fs::read_to_string(win_path)
        .ok()
        .map(|t| conda::parse_windows(&t, true).expect("the previous windows.tsv parses"))
        .unwrap_or_default();
    let published = match cc::read_published(&Fetcher::direct(), &cc::sources()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2)
        }
    };
    let mut rows = cc::from_linux(&linux, &published);
    let unknown: Vec<&String> = accept
        .iter()
        .filter(|a| !previous.iter().any(|r| &r.name == *a))
        .collect();
    if !unknown.is_empty() {
        eprintln!("--accept names rows the previous table doesn't have: {unknown:?}");
        std::process::exit(2);
    }
    let changed = cc::pin_hashes(&previous, &mut rows, &accept);
    if !accept.is_empty() {
        println!(
            "accepted the publisher's current hash for: {}",
            accept.join(" ")
        )
    }
    let lost = cc::lost(&previous, &rows);
    std::fs::write(win_path, cc::render(&rows, "ci", &cc::now())).expect("write windows.tsv");
    let mut count = std::collections::BTreeMap::new();
    for r in &rows {
        let k = match r.verdict {
            conda::Verdict::Ok(_) => "ok",
            conda::Verdict::OkLatest(_) => "ok-latest",
            conda::Verdict::NoBinary(_) => "no-binary",
            conda::Verdict::Unsupported(_) => "unsupported",
        };
        *count.entry(k).or_insert(0) += 1;
    }
    println!("{} rows: {count:?}", rows.len());
    if !changed.is_empty() {
        eprintln!("changed hash, kept the pinned one: {}", changed.join(" "))
    }
    if !lost.is_empty() {
        eprintln!("no longer installable: {}", lost.join(" "))
    }
    if !changed.is_empty() || !lost.is_empty() {
        std::process::exit(1)
    }
}
