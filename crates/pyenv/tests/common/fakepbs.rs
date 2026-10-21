//! A fake python-build-standalone `install_only` tarball for tier-1 tests (M8d): top folder
//! `python/`, `bin/python3.13` answers `-c` (exit 0, or 1 for `import <missing>`), `-m venv
//! [--without-pip] <dir>` (copies itself to `<dir>/bin/python`) and `-I -m ensurepip` (exit 0),
//! links `bin/python` and `bin/python3`, and the three files D18 rewrites, with `clang` and
//! `/install`. Unix only.

use std::io::Write;

pub fn python_script(missing: &[&str]) -> String {
    let mut fail = String::new();
    for m in missing {
        fail.push_str(&format!("[ \"$2\" = \"import {m}\" ] && exit 1; "));
    }
    format!(
        "#!/bin/sh\ncase \"$1\" in\n  -c) {fail}exit 0;;\n  -m) if [ \"$2\" = venv ]; then d=\"$3\"; [ \"$d\" = --without-pip ] && d=\"$4\"; mkdir -p \"$d/bin\" && cp \"$0\" \"$d/bin/python\"; exit 0; fi;;\nesac\nexit 0\n"
    )
}

fn file(b: &mut tar::Builder<Vec<u8>>, path: &str, body: &[u8], mode: u32) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_size(body.len() as u64);
    h.set_mode(mode);
    b.append_data(&mut h, format!("python/{path}"), body)
        .unwrap();
}

fn link(b: &mut tar::Builder<Vec<u8>>, path: &str, target: &str) {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    h.set_mode(0o777);
    b.append_link(&mut h, format!("python/{path}"), target)
        .unwrap();
}

/// `with_sysconfig` false leaves out `_sysconfigdata_*.py` (a changed layout).
pub fn tarball_gz(missing: &[&str], with_sysconfig: bool) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    file(
        &mut b,
        "bin/python3.13",
        python_script(missing).as_bytes(),
        0o755,
    );
    file(
        &mut b,
        "bin/python3.13-config",
        b"#!/bin/sh\nprefix_build=\"/install\"\n",
        0o755,
    );
    file(
        &mut b,
        "lib/python3.13/config-3.13-x86_64-linux-gnu/Makefile",
        b"CC=\t\tclang -pthread\nprefix=\t\t/install\n",
        0o644,
    );
    if with_sysconfig {
        file(
            &mut b,
            "lib/python3.13/_sysconfigdata__linux_x86_64-linux-gnu.py",
            b"build_time_vars = {'CC': 'clang -pthread', 'CXX': 'clang++ -pthread', 'prefix': '/install', 'INCLUDEPY': '/install/include/python3.13'}\n",
            0o644,
        );
    }
    file(&mut b, "lib/python3.13/os.py", b"# os\n", 0o644);
    link(&mut b, "bin/python", "python3.13");
    link(&mut b, "bin/python3", "python3.13");
    let tar_bytes = b.into_inner().unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&tar_bytes).unwrap();
    enc.finish().unwrap()
}
