//! A fake PyPy binary tarball for tier-1 tests (M7a): `bin/pypy3.11` answers like fakebuild's
//! interpreter (`-c "import X"`; `-I -m ensurepip` writes `bin/pip3.11`), `bin/pypy3` and
//! `bin/python3.11` link to it, `bin/libpypy3.11-c.so` is executable, and
//! `lib/pypy3.11/site-packages/README` stands in for the library. Unix only.

use std::io::Write;

const PYPY: &str = "#!/bin/sh\ncase \"$1\" in\n  -c) exit 0;;\n  -I) if [ \"$2\" = \"-m\" ] && [ \"$3\" = \"ensurepip\" ]; then\n        d=$(dirname \"$0\"); printf '#!%s\\n' \"$d/pypy3.11\" > \"$d/pip3.11\"; chmod 755 \"$d/pip3.11\"; exit 0\n      fi;;\nesac\nexit 0\n";

fn file(b: &mut tar::Builder<Vec<u8>>, top: &str, path: &str, body: &[u8], mode: u32) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_size(body.len() as u64);
    h.set_mode(mode);
    b.append_data(&mut h, format!("{top}/{path}"), body)
        .unwrap();
}

fn link(b: &mut tar::Builder<Vec<u8>>, top: &str, path: &str, target: &str) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    h.set_mode(0o777);
    b.append_link(&mut h, format!("{top}/{path}"), target)
        .unwrap();
}

/// `<top>.tar.bz2` bytes. With `real_pypy` false, `bin/pypy3.11` is a link to a missing file,
/// so `bin/` has no regular `pypy*` executable.
pub fn tarball_bz2(top: &str, real_pypy: bool) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    if real_pypy {
        file(&mut b, top, "bin/pypy3.11", PYPY.as_bytes(), 0o755);
    } else {
        link(&mut b, top, "bin/pypy3.11", "missing");
    }
    link(&mut b, top, "bin/pypy3", "pypy3.11");
    link(&mut b, top, "bin/python3.11", "pypy3.11");
    file(&mut b, top, "bin/libpypy3.11-c.so", b"ELF", 0o755);
    file(
        &mut b,
        top,
        "lib/pypy3.11/site-packages/README",
        b"library\n",
        0o644,
    );
    let tar_bytes = b.into_inner().unwrap();
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    enc.write_all(&tar_bytes).unwrap();
    enc.finish().unwrap()
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
