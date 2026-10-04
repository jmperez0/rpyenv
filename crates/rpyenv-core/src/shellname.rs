//! Which shell `pyenv init` and `pyenv shell` talk to (spec §7).

use crate::flavor::Flavor;

/// The code a shell name selects. Upstream's `pyenv-sh-shell`, `pyenv-sh-rehash` and most of
/// `pyenv-init` know fish, pwsh, and POSIX for every other name; `pyenv-init`'s function
/// header adds a Korn-shell variant (libexec/pyenv-init:554-563). Windows adds cmd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Posix,
    Ksh,
    Fish,
    Pwsh,
    Cmd,
}

/// The family of a normalized shell name. Upstream matches names exactly, so on Linux
/// `powershell` and `cmd` are POSIX names. On Windows `powershell` is Windows PowerShell
/// 5.1, which runs the same code as `pwsh`, and `cmd` is cmd.exe.
pub fn family(name: &str, flavor: Flavor) -> Family {
    match (name, flavor) {
        ("fish", _) => Family::Fish,
        ("pwsh", _) | ("powershell", Flavor::PyenvWin) => Family::Pwsh,
        ("ksh" | "ksh93" | "mksh", _) => Family::Ksh,
        ("cmd", Flavor::PyenvWin) => Family::Cmd,
        _ => Family::Posix,
    }
}

/// `basename "${PYENV_SHELL:-$SHELL}"` (libexec/pyenv-sh-shell:33): an empty
/// `PYENV_SHELL` counts as unset.
pub fn from_env(pyenv_shell: Option<&str>, shell: Option<&str>) -> String {
    let v = pyenv_shell
        .filter(|s| !s.is_empty())
        .or(shell)
        .unwrap_or("");
    basename(v).to_string()
}

/// POSIX `basename`: trailing slashes go first, and `/` alone stays `/`.
fn basename(s: &str) -> &str {
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        return if s.is_empty() { "" } else { "/" };
    }
    t.rsplit('/').next().unwrap_or(t)
}

/// `pyenv init`'s detection from the parent's command line, NULs already turned into spaces
/// (libexec/pyenv-init:62-66): up to the first space, one leading `-` removed, `$SHELL` when
/// that leaves nothing, then after the last `/` and before the first `-`.
pub fn from_parent_cmdline(cmdline: &str, shell: Option<&str>) -> String {
    let first = cmdline.split(' ').next().unwrap_or("");
    let first = first.strip_prefix('-').unwrap_or(first);
    let s = if first.is_empty() {
        shell.unwrap_or("")
    } else {
        first
    };
    let s = s.rsplit('/').next().unwrap_or("");
    s.split('-').next().unwrap_or("").to_string()
}

/// The parent's command line with NULs as spaces: `/proc/<ppid>/cmdline`, else
/// `ps p <ppid> -o args=` (libexec/pyenv-init:57-61). Empty when both fail.
#[cfg(unix)]
pub fn parent_cmdline() -> String {
    let ppid = std::os::unix::process::parent_id();
    if let Ok(bytes) = std::fs::read(format!("/proc/{ppid}/cmdline")) {
        return String::from_utf8_lossy(&bytes).replace('\0', " ");
    }
    std::process::Command::new("ps")
        .args(["p", &ppid.to_string(), "-o", "args="])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default()
}

/// The shells rpyenv supports on Windows, by executable name.
const WINDOWS_SHELLS: [&str; 7] = ["cmd", "powershell", "pwsh", "bash", "sh", "zsh", "fish"];

/// A Windows executable or shell name as a supported shell name: `PWSH.EXE` → `pwsh`.
pub fn windows_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    WINDOWS_SHELLS.contains(&stem).then(|| stem.to_string())
}

/// The shell on Windows when integration isn't loaded (spec §7, amended 2026-10-04): the
/// parent process's image when it names a supported shell, else `PYENV_SHELL`, else none.
/// Only `pyenv init` sets `PYENV_SHELL`, so a child shell may inherit a value that names
/// its ancestor; that is why the parent comes first.
pub fn windows_shell(parent_image: Option<&str>, pyenv_shell: Option<&str>) -> Option<String> {
    parent_image
        .and_then(windows_name)
        .or_else(|| pyenv_shell.and_then(|s| windows_name(basename(s))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M3L "Shell detection": parent `argv[0]` values observed with `exec -a`, plus the
    /// `$SHELL` fallback. NULs are already spaces, so every command line ends in one.
    #[test]
    fn parent_cmdline_detection_matches_upstream() {
        for (cmdline, shell, want) in [
            ("bash ", None, "bash"),
            ("-bash ", None, "bash"),
            ("/usr/bin/bash ", None, "bash"),
            ("-zsh ", None, "zsh"),
            ("/usr/local/bin/zsh-5.9 ", None, "zsh"),
            ("bash-5.3 ", None, "bash"),
            ("fish ", None, "fish"),
            ("-pwsh ", None, "pwsh"),
            ("ksh93 ", None, "ksh93"),
            ("sh /tmp/script.sh ", None, "sh"),
            ("xonsh.py ", None, "xonsh.py"),
            ("/usr/bin/zsh -l ", None, "zsh"),
            ("- ", Some("/opt/x/fish"), "fish"),
            ("", Some("/opt/x/zsh"), "zsh"),
            ("", None, ""),
        ] {
            assert_eq!(from_parent_cmdline(cmdline, shell), want, "{cmdline:?}");
        }
    }

    /// `basename "${PYENV_SHELL:-$SHELL}"`: an empty PYENV_SHELL counts as unset.
    #[test]
    fn env_shell_is_the_basename_of_pyenv_shell_or_shell() {
        assert_eq!(from_env(Some("/usr/bin/pwsh"), Some("/bin/bash")), "pwsh");
        assert_eq!(from_env(Some(""), Some("/usr/bin/fish")), "fish");
        assert_eq!(from_env(None, Some("/bin/zsh/")), "zsh");
        assert_eq!(from_env(None, None), "");
    }

    /// Upstream matches exact names: on Linux `powershell` and `cmd` are POSIX names.
    #[test]
    fn families_by_flavor() {
        use Flavor::{Pyenv, PyenvWin};
        assert_eq!(family("fish", Pyenv), Family::Fish);
        assert_eq!(family("pwsh", Pyenv), Family::Pwsh);
        assert_eq!(family("powershell", Pyenv), Family::Posix);
        assert_eq!(family("powershell", PyenvWin), Family::Pwsh);
        assert_eq!(family("mksh", Pyenv), Family::Ksh);
        assert_eq!(family("cmd", Pyenv), Family::Posix);
        assert_eq!(family("cmd", PyenvWin), Family::Cmd);
        assert_eq!(family("nu", Pyenv), Family::Posix);
    }

    /// Decision 1: the parent wins over PYENV_SHELL; a parent that isn't a shell falls back.
    #[test]
    fn windows_order_is_parent_then_pyenv_shell() {
        assert_eq!(windows_name("PWSH.EXE").as_deref(), Some("pwsh"));
        assert_eq!(windows_name("Code.exe"), None);
        assert_eq!(
            windows_shell(Some("cmd.exe"), Some("pwsh")).as_deref(),
            Some("cmd")
        );
        assert_eq!(
            windows_shell(Some("python.exe"), Some("pwsh")).as_deref(),
            Some("pwsh")
        );
        assert_eq!(windows_shell(Some("python.exe"), Some("nu")), None);
        assert_eq!(windows_shell(None, None), None);
    }
}
