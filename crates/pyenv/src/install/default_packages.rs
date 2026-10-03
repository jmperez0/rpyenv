//! The pyenv-default-packages plugin's behavior, built in (spec §9.3): after a successful
//! install, `pip install -r $PYENV_ROOT/default-packages` in the new version. It runs pip in the
//! installed directory, so an alias gets the packages too (plan Decision 5).

use std::path::Path;
use std::process::Command;

/// None on success or when there is no file; otherwise the error line for stderr.
pub fn run(root: &Path, prefix: &Path) -> Option<String> {
    let file = root.join("default-packages");
    if !file.is_file() {
        return None;
    }
    let bin = prefix.join("bin");
    let python = ["python", "python3"]
        .iter()
        .map(|n| bin.join(n))
        .find(|p| p.exists())
        .or_else(|| {
            std::fs::read_dir(&bin)
                .ok()?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("python") && n.ends_with(|c: char| c.is_ascii_digit())
                    })
                })
                .max()
        });
    let ok = python.is_some_and(|python| {
        Command::new(&python)
            .args(["-m", "pip", "install", "-r"])
            .arg(&file)
            .status()
            .is_ok_and(|s| s.success())
    });
    (!ok).then(|| {
        format!(
            "pyenv: error installing packages from  `{}'",
            file.display()
        )
    })
}
