//! Windows launch fidelity (spec §5.3), with real shims.
#![cfg(windows)]

mod common;
use common::*;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;

fn v(s: &str) -> &OsStr {
    OsStr::new(s)
}

/// Raw command-line tails that cmd.exe or a split-and-requote round trip would change.
const RAW: [&str; 13] = [
    r#"a b"#,
    r#""a b""#,
    r#""""#,
    r#"x^y 100% %USERNAME% a&b !x!"#,
    r#""say \"hi\"""#,
    r#""C:\path\\""#,
    r#""abc"#,
    r#"a""b"#,
    "ñ ü 漢",
    r#"\\server\share"#,
    "  lead",
    r#"a\\\"b"#,
    r#"a|b <c >d"#,
];

/// Review focus 1: through the shim, the child parses exactly what it parses run directly.
#[test]
fn win_shim_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    f.rehash();
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let shim = f
            .shim_command("python", &env)
            .raw_arg(raw)
            .output()
            .unwrap();
        assert_eq!(arg_lines(&shim), arg_lines(&direct), "raw tail {raw:?}");
    }
}

#[test]
fn win_exec_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let exec = f
            .command(&built("pyenv"), &env)
            .args(["exec", "python"])
            .raw_arg(raw)
            .output()
            .unwrap();
        assert_eq!(arg_lines(&exec), arg_lines(&direct), "raw tail {raw:?}");
    }
}
