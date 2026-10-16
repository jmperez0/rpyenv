//! A fake conda installer for M7c's Windows tests: by its own file name it is
//! - `*-Windows-x86_64.exe`: the NSIS installer. Reads the raw command line as NSIS does
//!   (`GetCommandLineW`; `/D=` is everything after the last ` /D=`, M7c design §3) and checks
//!   `/S` first and that the `/D=` tail is not quoted; appends `args: <raw tail>`,
//!   `marker: yes|no` (`<D>\.rpyenv-incomplete`), `tos: …` to `%FAKECONDA_LOG%`; exits
//!   `%FAKECONDA_EXIT%` before writing anything when set; else writes the tree (Task 1's
//!   measurement): `python.exe` and `Scripts\conda.exe` (copies of itself), `conda-meta\`
//!   records for python and conda, empty `envs\` and `Library\bin\`.
//! - `python.exe`: logs `python: <args>`, exits `%FAKECONDA_PYTHON_EXIT%` (default 0).
//! - `conda.exe`: `--version` prints `conda 25.1.1`; `install …` logs `conda: <args>`, exits
//!   `%FAKECONDA_INSTALL_EXIT%` (default 0).
//! - with `%FAKECONDA_SLEEP%` (seconds), the installer sleeps after writing (Ctrl+C tests).
use std::io::Write;

fn log(line: &str) {
    if let Some(p) = std::env::var_os("FAKECONDA_LOG") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .unwrap();
        writeln!(f, "{line}").unwrap();
    }
}

fn code(var: &str) -> i32 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The command line after the program name, exactly as the parent wrote it.
#[cfg(windows)]
fn raw_tail() -> String {
    // SAFETY: GetCommandLineW returns this process's NUL-terminated command line, valid for
    // the process's life.
    let line = unsafe {
        let p = windows_sys::Win32::System::Environment::GetCommandLineW();
        let len = (0..).take_while(|&i| *p.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    };
    let rest = match line.strip_prefix('"') {
        Some(r) => r.split_once('"').map_or("", |(_, t)| t),
        None => line.split_once(' ').map_or("", |(_, t)| t),
    };
    rest.trim_start().to_string()
}

#[cfg(not(windows))]
fn raw_tail() -> String {
    std::env::args().skip(1).collect::<Vec<_>>().join(" ")
}

fn main() {
    let me = std::env::current_exe().unwrap();
    let name = me
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if name == "python.exe" {
        log(&format!("python: {}", args.join(" ")));
        std::process::exit(code("FAKECONDA_PYTHON_EXIT"));
    }
    if name == "conda.exe" {
        if args.first().map(String::as_str) == Some("--version") {
            println!("conda 25.1.1");
            return;
        }
        log(&format!("conda: {}", args.join(" ")));
        std::process::exit(code("FAKECONDA_INSTALL_EXIT"));
    }
    // The installer: NSIS takes everything after the last ` /D=` as the folder.
    let tail = raw_tail();
    let d = tail
        .rfind(" /D=")
        .map(|i| tail[i + 4..].to_string())
        .filter(|d| !d.starts_with('"') && tail.starts_with("/S "));
    let Some(d) = d.map(std::path::PathBuf::from) else {
        log(&format!("bad args: {tail}"));
        std::process::exit(2);
    };
    log(&format!("args: {tail}"));
    log(&format!(
        "marker: {}",
        if d.join(".rpyenv-incomplete").exists() {
            "yes"
        } else {
            "no"
        }
    ));
    log(&format!(
        "tos: {}",
        std::env::var("CONDA_PLUGINS_AUTO_ACCEPT_TOS").unwrap_or_else(|_| "unset".into())
    ));
    if std::env::var_os("FAKECONDA_EXIT").is_some() {
        std::process::exit(code("FAKECONDA_EXIT"));
    }
    for sub in ["conda-meta", "envs", "Library\\bin", "Scripts"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    std::fs::copy(&me, d.join("python.exe")).unwrap();
    std::fs::copy(&me, d.join("Scripts").join("conda.exe")).unwrap();
    std::fs::write(
        d.join("conda-meta").join("python-3.12.0-0.json"),
        r#"{"name":"python","version":"3.12.0","files":["python.exe"]}"#,
    )
    .unwrap();
    std::fs::write(
        d.join("conda-meta").join("conda-25.1.1-0.json"),
        r#"{"name":"conda","version":"25.1.1","files":["Scripts/conda.exe","Lib/site-packages/conda/__init__.py"]}"#,
    )
    .unwrap();
    std::fs::write(d.join("conda-meta").join("history"), "+install\n").unwrap();
    if let Some(s) = std::env::var("FAKECONDA_SLEEP")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        std::thread::sleep(std::time::Duration::from_secs(s));
    }
}
