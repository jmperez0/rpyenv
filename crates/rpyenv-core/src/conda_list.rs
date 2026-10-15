//! Upstream's conda rehash hook (pyenv.d/rehash/conda.bash, M7b design §5): when any version
//! or env has `bin/conda`, the names in conda.d/default.list (conda's bundled system tools:
//! `curl`, `sed`, `clear`, …) get no shim, so the system's copies keep working.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

/// Vendored byte for byte by ci/sync_python_build.py; pyenv's MIT license is beside it.
static LIST: &str = include_str!("../../pyenv/python-build/rehash/conda.d/default.list");

/// The list after upstream's `sed 's/#.*$//; /^[[:space:]]*$/d'`, split into words; each is
/// compared whole (upstream's `case` pattern; `[` alone matches itself).
pub fn names() -> &'static BTreeSet<&'static str> {
    static NAMES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        LIST.lines()
            .map(|l| l.split('#').next().unwrap_or(""))
            .flat_map(str::split_whitespace)
            .collect()
    })
}

/// Upstream's `conda_exists`: `versions/*/bin/conda` or `versions/*/envs/*/bin/conda`
/// exists, dot folders included (`dotglob`). Existence only, as the glob tests it.
pub fn conda_exists(versions_dir: &Path) -> bool {
    let has = |d: &Path| d.join("bin").join("conda").symlink_metadata().is_ok();
    let Ok(rd) = std::fs::read_dir(versions_dir) else {
        return false;
    };
    rd.filter_map(Result::ok).map(|e| e.path()).any(|v| {
        has(&v)
            || std::fs::read_dir(v.join("envs"))
                .map(|envs| envs.filter_map(Result::ok).any(|e| has(&e.path())))
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_upstreams_minus_comments() {
        let n = names();
        assert!(n.contains("curl") && n.contains("clear") && n.contains("["));
        assert!(!n.iter().any(|w| w.starts_with('#') || w.is_empty()));
        assert_eq!(
            n.len(),
            177,
            "pyenv 2.8.9's list; update this when the sync changes it"
        );
    }

    #[test]
    fn conda_in_an_env_or_a_dot_folder_triggers_the_list() {
        for path in ["v/bin/conda", "v/envs/e/bin/conda", ".old-v/bin/conda"] {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap(); // existence, not the execute bit
            assert!(conda_exists(d.path()), "{path}");
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("3.12.0/bin")).unwrap();
        assert!(!conda_exists(d.path()));
    }
}
