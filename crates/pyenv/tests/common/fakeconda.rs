//! A fake conda installer for tier-1 tests (M7b): run as `bash <it> -f -b -p <prefix>`, it
//! records its arguments, whether `<prefix>/.rpyenv-incomplete` exists, and
//! CONDA_PLUGINS_AUTO_ACCEPT_TOS into `$FAKECONDA_LOG` (when set), then writes `bin/python<X.Y>`
//! (answers `-c …` with exit 0), `bin/python` (a link), `bin/conda` (`--version` prints
//! `conda 25.1.1`; `install …` appends its arguments to the log and exits `$FAKECONDA_INSTALL_EXIT`,
//! default 0) and `conda-meta/history` and an empty `envs/` (as real installers do). It exits `$FAKECONDA_EXIT` (default 0) before writing
//! anything when that is set. Unix only.

pub fn installer(py: &str) -> String {
    format!(
        r#"#!/bin/bash
log=${{FAKECONDA_LOG:-/dev/null}}
prefix=""
while [ $# -gt 0 ]; do
  case "$1" in -p) prefix=$2; shift 2;; *) shift;; esac
done
{{ echo "args: -f -b -p $prefix"; [ -e "$prefix/.rpyenv-incomplete" ] && echo "marker: yes" || echo "marker: no"
   echo "tos: ${{CONDA_PLUGINS_AUTO_ACCEPT_TOS:-unset}}"; }} >> "$log"
[ -n "${{FAKECONDA_EXIT:-}}" ] && exit "$FAKECONDA_EXIT"
mkdir -p "$prefix/bin" "$prefix/conda-meta" "$prefix/envs"
printf '#!/bin/sh\nexit 0\n' > "$prefix/bin/python{py}"
chmod 755 "$prefix/bin/python{py}"
ln -sf "python{py}" "$prefix/bin/python"
cat > "$prefix/bin/conda" <<'EOF'
#!/bin/sh
case "$1" in
  --version) echo "conda 25.1.1";;
  install) echo "conda: $*" >> "${{FAKECONDA_LOG:-/dev/null}}"; exit "${{FAKECONDA_INSTALL_EXIT:-0}}";;
esac
EOF
chmod 755 "$prefix/bin/conda"
echo "+install" > "$prefix/conda-meta/history"
"#
    )
}
