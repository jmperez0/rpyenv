//! Embeds the vendored python-build data (plan M2a, Decision 1): definitions as text, the
//! patches as one gzip-compressed tar.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let share = manifest.join("python-build").join("share");
    println!("cargo::rerun-if-changed=python-build");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let mut names: Vec<String> = fs::read_dir(&share)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut defs = String::from("pub static DEFINITIONS: &[(&str, &str)] = &[\n");
    for n in &names {
        defs.push_str(&format!(
            "    ({n:?}, include_str!({:?})),\n",
            share.join(n)
        ));
    }
    defs.push_str("];\n");
    // UPSTREAM is `pyenv <commit> <version>`, written by ci/sync_python_build.py.
    let upstream = fs::read_to_string(manifest.join("python-build").join("UPSTREAM")).unwrap();
    let version = upstream
        .split_whitespace()
        .nth(2)
        .expect("python-build/UPSTREAM: `pyenv <commit> <version>`");
    defs.push_str(&format!(
        "/// The python-build release the vendored definitions come from (UPSTREAM).\npub const UPSTREAM_VERSION: &str = {version:?};\n"
    ));
    fs::write(out.join("defs.rs"), defs).unwrap();

    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let mut b = tar::Builder::new(gz);
    let patches = share.join("patches");
    add_dir(&mut b, &patches, Path::new(""));
    let bytes = b.into_inner().unwrap().finish().unwrap();
    fs::File::create(out.join("patches.tar.gz"))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}

fn add_dir(b: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>, dir: &Path, rel: &Path) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let r = rel.join(e.file_name());
        if p.is_dir() {
            add_dir(b, &p, &r);
        } else {
            b.append_path_with_name(&p, &r).unwrap();
        }
    }
}
