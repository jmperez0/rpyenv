//! `pyenv latest` (docs/parity/pyenv-m1-reference.md "pyenv latest", pyenv-m2-reference.md
//! "pyenv latest — what M2 adds"). Linux flavor only until M2b.
#![cfg(unix)]

mod common;
use common::Fixture;

#[test]
fn installed_mode_picks_the_newest_matching_version() {
    let f = Fixture::new();
    f.version("3.12.1")
        .version("3.12.10")
        .version("3.12.9")
        .version("3.13.0");
    let r = f.pyenv(&["latest", "3.12"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("3.12.10\n", "", 0)
    );
}

// allowlist D-57
#[test]
fn installed_mode_ignores_the_installers_staging() {
    let f = Fixture::new();
    f.version("3.12.1").version(".tmp-3.12.20");
    assert_eq!(f.pyenv(&["latest", "3.12"]).stdout, "3.12.1\n");
}

// A staging name matches a prefix of its own text, so this fails if staging names are not
// hidden (the case above passes either way: `.tmp-3.12.20` does not start with `3.12`).
// allowlist D-57
#[test]
fn a_staging_name_is_not_a_match_for_its_own_prefix() {
    let f = Fixture::new();
    f.version("3.12.1").version(".old-3.12.30");
    let r = f.pyenv(&["latest", ".old"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        (
            "",
            "pyenv: no installed versions match the prefix `.old'\n",
            1
        )
    );
}

#[test]
fn known_mode_reads_the_vendored_definitions() {
    let f = Fixture::new();
    for (prefix, want) in [
        ("3.12", "3.12.14"),
        ("3.1", "3.1.5"),
        ("3.14t", "3.14.7t"),
        ("3t", "3.15.0rc2t"),
        ("2", "2.7.18"),
        ("3.12-dev", "3.12-dev"),
    ] {
        let r = f.pyenv(&["latest", "-k", prefix]);
        assert_eq!(
            (r.stdout.as_str(), r.code),
            (format!("{want}\n").as_str(), 0),
            "{prefix}"
        );
    }
}

#[test]
fn a_failed_match_reports_or_falls_back() {
    let f = Fixture::new();
    let r = f.pyenv(&["latest", "-k", "3.15"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("", "pyenv: no known versions match the prefix `3.15'\n", 1)
    );
    let r = f.pyenv(&["latest", "3.15"]);
    assert_eq!(
        r.stderr,
        "pyenv: no installed versions match the prefix `3.15'\n"
    );
    let r = f.pyenv(&["latest", "-b", "3.15"]);
    assert_eq!((r.stdout.as_str(), r.code), ("3.15\n", 1));
    let r = f.pyenv(&["latest", "-f", "3.15"]);
    assert_eq!((r.stdout.as_str(), r.code), ("3.15\n", 0));
    let r = f.pyenv(&["latest", "-k", "-q", "3.12"]);
    assert_eq!(
        r.stderr, "pyenv: no known versions match the prefix `-q'\n",
        "no -q option exists"
    );
}

#[test]
fn standalone_known_mode_does_not_see_plugin_definitions() {
    let f = Fixture::new();
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.12.99"),
        "x\n",
    );
    assert_eq!(f.pyenv(&["latest", "-k", "3.12"]).stdout, "3.12.14\n");
}

#[test]
fn no_prefix_prints_an_empty_line_in_installed_mode() {
    let f = Fixture::new();
    let r = f.pyenv(&["latest"]);
    assert_eq!((r.stdout.as_str(), r.code), ("\n", 0));
}

#[test]
fn help_matches_upstream() {
    let f = Fixture::new();
    let r = f.pyenv(&["help", "latest"]);
    assert_eq!(r.stdout, "Usage: pyenv latest [-k|--known] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -b/--bypass     (internal) On a resolution failure, do not print an error message\n                  but rather print the argument unchanged\n  -f/--force      (internal) Same as -b but also do not return a failure exit code\n\n");
    assert_eq!(
        f.pyenv(&["help", "--usage", "latest"]).stdout,
        "Usage: pyenv latest [-k|--known] <prefix>\n"
    );
}
