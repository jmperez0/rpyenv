//! A fake `Python-<v>` source tarball for tier-1 build tests (spec §12.5): `configure`
//! records its arguments and flags (`lib/rpyenv-config.txt`), `make` records the flags in its
//! own environment (`lib/rpyenv-make.txt`), `make install` honors DESTDIR, and the stand-in
//! interpreter answers `-c "import X"` and `-m ensurepip`. Unix only: it needs `sh` and `make`.

use std::io::Write;

const CONFIGURE: &str = r#"#!/bin/sh
prefix=
for a in "$@"; do case "$a" in --prefix=*) prefix="${a#--prefix=}";; esac; done
{ printf 'args:'; for a in "$@"; do printf ' %s' "$a"; done; printf '\n'
  printf 'CFLAGS=%s\nCPPFLAGS=%s\nLDFLAGS=%s\nLIBS=%s\n' "$CFLAGS" "$CPPFLAGS" "$LDFLAGS" "$LIBS"
  printf 'CFLAGS_SET=%s\n' "${CFLAGS+yes}"; } > rpyenv-config.txt
if [ -n "$FAKE_CONFIGURE_FAIL" ]; then echo 'configure: error: no acceptable C compiler found in $PATH'; exit 1; fi
printf 'all:\n\t@sleep $${FAKE_MAKE_SLEEP:-0}\n\t@env | grep -E "^(CFLAGS|CPPFLAGS|LDFLAGS|LIBS)=" | sort > rpyenv-make.txt\ninstall:\n\tmkdir -p "$(DESTDIR)%s/bin" "$(DESTDIR)%s/lib"\n\tcp python3.12 "$(DESTDIR)%s/bin/python3.12"\n\tchmod 755 "$(DESTDIR)%s/bin/python3.12"\n\tcp rpyenv-config.txt "$(DESTDIR)%s/lib/rpyenv-config.txt"\n\tcp rpyenv-make.txt "$(DESTDIR)%s/lib/rpyenv-make.txt"\n' "$prefix" "$prefix" "$prefix" "$prefix" "$prefix" "$prefix" > Makefile
"#;

const PYTHON: &str = r#"#!/bin/sh
case "$1" in
  -c) mod="${2#import }"
      for m in $FAKE_PY_MISSING; do
        if [ "$m" = "$mod" ]; then echo "ModuleNotFoundError: No module named '_$mod'" >&2; exit 1; fi
      done
      exit 0;;
  -m) if [ "$2" = "pip" ]; then
        printf '%s\n' "$*" >> "${FAKE_PIP_LOG:-/dev/null}"
        [ -n "$FAKE_PIP_FAIL" ] && exit 1
        exit 0
      fi;;
  -I|-s)
      if [ "$2" = "-m" ] && [ "$3" = "ensurepip" ]; then
        sleep "${FAKE_PIP_SLEEP:-0}"
        [ -n "$FAKE_PY_NO_PIP" ] && exit 1
        d=$(dirname "$0"); printf '#!%s\n' "$d/python3.12" > "$d/pip3.12"; chmod 755 "$d/pip3.12"; exit 0
      fi;;
esac
exit 0
"#;

/// `Python-<version>.tar.gz` bytes: `configure`, `python3.12`, `README`, `Tools/gdb/libpython.py`.
pub fn tarball(version: &str) -> Vec<u8> {
    let top = format!("Python-{version}");
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (name, body, mode) in [
        ("configure", CONFIGURE, 0o755),
        ("python3.12", PYTHON, 0o755),
        ("README", "original\n", 0o644),
        ("Tools/gdb/libpython.py", "# gdb\n", 0o644),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(mode);
        b.append_data(&mut h, format!("{top}/{name}"), body.as_bytes())
            .unwrap();
    }
    let gz = b.into_inner().unwrap();
    let mut out = gz.finish().unwrap();
    out.flush().unwrap();
    out
}

/// A patch that changes README's one line, in python-build's built-in patch layout.
pub const README_PATCH: &str = "--- README\n+++ README\n@@ -1 +1 @@\n-original\n+patched\n";
