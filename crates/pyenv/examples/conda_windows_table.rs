//! CI's Windows conda table (M7c design §4.2): `cargo run -p pyenv --example
//! conda_windows_table -- <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]`. Reads
//! the publishers, rebuilds <windows.tsv> from <linux.tsv>'s x86_64 rows, keeps every pinned
//! hash (§4.2 step 6) except the rows a maintainer accepts (§4.2 step 7, D8).
//! When no row differs from <windows.tsv>'s, the file is left alone (its `# generated` line
//! too) and `windows.tsv unchanged` is printed (final review I2).
//! Exit 0: written or unchanged. 1: as 0, but some rows changed hash or stopped installing
//! (named on stderr, starting `changed hash, kept the pinned one:` / `no longer installable:`).
//! 2: bad usage, an unreadable or unparseable previous table, or the publishers couldn't be
//! read; nothing written.
use pyenv::install::{conda, condacatalog as cc, fetch::Fetcher};

fn die(msg: String) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
}

fn main() {
    const USAGE: &str =
        "usage: conda_windows_table <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]";
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, accept): (Vec<String>, Vec<String>) =
        match args.iter().position(|a| a == "--accept") {
            Some(i) => match args.get(i + 1).filter(|v| !v.is_empty()) {
                Some(v) => (
                    args[..i].to_vec(),
                    v.split(',').map(str::to_string).collect(),
                ),
                None => die(USAGE.to_string()),
            },
            None => (args.clone(), Vec::new()),
        };
    let [linux_path, win_path] = paths.as_slice() else {
        die(USAGE.to_string())
    };
    let linux = std::fs::read_to_string(linux_path)
        .map_err(|e| format!("{linux_path}: {e}"))
        .and_then(|t| conda::parse_linux(&t).map_err(|e| format!("{linux_path}: {e}")))
        .unwrap_or_else(|e| die(e));
    // Only a missing file means "no previous table"; one that is there but unreadable or
    // unparseable stops the run, or its pinned hashes would be silently dropped.
    let (previous, previous_text) = match std::fs::read_to_string(win_path) {
        Ok(t) => (
            conda::parse_windows(&t, true).unwrap_or_else(|e| {
                die(format!("{win_path}: the previous table doesn't parse: {e}"))
            }),
            t,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), String::new()),
        Err(e) => die(format!("{win_path}: {e}")),
    };
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
    let text = cc::render(&rows, "ci", &cc::now());
    if cc::same_rows(&previous_text, &text) {
        // Only the stamp would differ: no weekly timestamp-only PR (final review I2).
        println!("windows.tsv unchanged");
    } else {
        std::fs::write(win_path, text).expect("write windows.tsv");
    }
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
