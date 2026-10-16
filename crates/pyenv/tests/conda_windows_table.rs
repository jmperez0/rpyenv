use pyenv::install::conda::{self, Verdict};
use std::fs;

/// Serialises the tests that set `RPYENV_TEST_CONDA_TABLES`, a process-wide variable.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const ROW: &str = "miniforge3-26.7.2-0\tWindows-x86_64\tok Miniforge3-26.7.2-0-Windows-x86_64 https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe#1111111111111111111111111111111111111111111111111111111111111111 miniconda verify_py3\n";

fn table(time: &str, by: &str, extra: &str) -> String {
    format!("# generated {time} by {by}\n# <name>\\t<arch>\\t<verdict>\n{ROW}{extra}")
}

#[test]
fn generated_reads_time_and_source() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        conda::generated(&table("2026-10-16T09:30:00Z", "ci", "")),
        Some(("2026-10-16T09:30:00Z".into(), "ci".into()))
    );
    assert_eq!(conda::generated("# <name>\n"), None);
    assert_eq!(conda::generated("# generated yesterday by ci\n"), None);
}

#[test]
fn a_windows_row_must_be_windows_and_from_a_publisher() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rows = conda::parse_windows(&table("2026-10-16T09:30:00Z", "ci", ""), true).unwrap();
    assert!(matches!(rows[0].verdict, Verdict::Ok(_)));
    let linux = ROW.replace("Windows-x86_64\t", "Linux-x86_64\t");
    assert!(conda::parse_windows(
        &table("2026-10-16T09:30:00Z", "ci", "").replace(ROW, &linux),
        true
    )
    .is_err());
    let foreign = ROW.replace(
        "https://github.com/conda-forge/miniforge/",
        "https://example.com/",
    );
    assert!(conda::parse_windows(&foreign, true).is_err());
    assert!(conda::parse_windows(&foreign, false).is_ok());
}

/// Review focus 4 (spec D8): a shipped `ok` row wins over a newer root copy's row for the
/// same name; the root copy adds names, and replaces a shipped row that isn't installable.
#[test]
fn shipped_rows_win_and_the_root_copy_adds_names() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let (root, shipped) = (t.path().join("root"), t.path().join("shipped"));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&shipped).unwrap();
    let other_hash = ROW.replace(&"1".repeat(64), &"2".repeat(64));
    let newer_name = ROW.replace("26.7.2-0", "27.1.0-0");
    fs::write(
        shipped.join("windows.tsv"),
        table(
            "2026-01-01T00:00:00Z",
            "ci",
            "anaconda-1.5.1\tWindows-x86_64\tno-binary Anaconda\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(conda::WIN_TABLE),
        format!("# generated 2026-10-16T09:30:00Z by update\n{other_hash}{newer_name}anaconda-1.5.1\tWindows-x86_64\tok Anaconda-1.5.1-Windows-x86_64 https://repo.anaconda.com/archive/Anaconda-1.5.1-Windows-x86_64.exe#{} anaconda verify_py3\n", "3".repeat(64)),
    ).unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", &shipped);
    let w = conda::windows_table(&root).unwrap();
    let url = |n: &str| match &w.rows.iter().find(|r| r.name == n).unwrap().verdict {
        Verdict::Ok(i) => i.url.clone(),
        v => panic!("{n}: {v:?}"),
    };
    assert!(
        url("miniforge3-26.7.2-0").ends_with(&"1".repeat(64)),
        "the shipped hash wins"
    );
    assert!(
        url("miniforge3-27.1.0-0").ends_with(&"1".repeat(64)),
        "the root copy adds a name"
    );
    assert!(
        url("anaconda-1.5.1").ends_with(&"3".repeat(64)),
        "a root ok row replaces a shipped no-binary"
    );
    assert_eq!(w.rows.len(), 3);
}

/// Review focus 4: a truncated root copy is ignored with a warning naming it.
#[test]
fn an_unreadable_copy_is_ignored_with_a_warning() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let (root, shipped) = (t.path().join("root"), t.path().join("shipped"));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&shipped).unwrap();
    fs::write(
        root.join(conda::WIN_TABLE),
        "# generated 2027-01-01T00:00:00Z by update\nminiforge3-1\tWindows-x86_64\tok trunc",
    )
    .unwrap();
    fs::write(
        shipped.join("windows.tsv"),
        table("2026-10-16T09:30:00Z", "ci", ""),
    )
    .unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", &shipped);
    let w = conda::windows_table(&root).unwrap();
    assert_eq!(w.rows.len(), 1);
    assert_eq!(w.warnings.len(), 1);
    assert!(w.warnings[0].contains(conda::WIN_TABLE), "{:?}", w.warnings);
}

#[test]
fn no_copy_at_all_names_pyenv_update() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", t.path().join("none"));
    let e = conda::windows_table(t.path()).unwrap_err();
    assert_eq!(e, "rpyenv has no conda version table; run pyenv update");
}
