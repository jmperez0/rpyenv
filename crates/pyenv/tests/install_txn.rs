//! Extraction safety (review focus 1) and the install transaction (spec §9.3, review focus
//! 2 and 5).

use pyenv::install::archive::{extract, Kind};
use pyenv::install::txn::{is_complete, Txn, MARKER};
use std::path::Path;

fn tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (name, data) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        // Raw name bytes: `append_data` would refuse `..` before the extractor can.
        let raw = &mut h.as_old_mut().name;
        raw[..name.len()].copy_from_slice(name.as_bytes());
        h.set_cksum();
        b.append(&h, *data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

#[cfg(unix)]
fn tgz_with_symlink(link: &str, target: &str, then_file: &str) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    b.append_link(&mut h, link, target).unwrap();
    let mut f = tar::Header::new_gnu();
    f.set_size(1);
    f.set_mode(0o644);
    b.append_data(&mut f, then_file, &b"x"[..]).unwrap();
    b.into_inner().unwrap().finish().unwrap()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn the_top_directory_is_renamed_to_the_package_name() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar.gz",
        &tgz(&[("Other-1.0/configure", b"#!/bin/sh\n")]),
    );
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    let out = extract(&a, Kind::Gz, &build, "Python-3.12.0").unwrap();
    assert_eq!(out, build.join("Python-3.12.0"));
    assert!(out.join("configure").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(out.join("configure"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0o111
        );
    }
}

#[test]
fn an_archive_without_a_top_directory_still_gets_the_package_name() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar.gz",
        &tgz(&[("configure", b"x"), ("README", b"y")]),
    );
    let out = extract(&a, Kind::Gz, d.path(), "pkg-1.0").unwrap();
    assert!(out.join("configure").is_file() && out.join("README").is_file());
}

// allowlist D-72
#[test]
fn parent_and_absolute_paths_are_refused() {
    for evil in ["../evil", "/tmp/rpyenv-evil-abs", "a/../../evil"] {
        let d = tempfile::tempdir().unwrap();
        let build = d.path().join("build");
        std::fs::create_dir(&build).unwrap();
        let a = write(
            d.path(),
            "a.tar.gz",
            &tgz(&[("ok/file", b"1"), (evil, b"2")]),
        );
        let err = extract(&a, Kind::Gz, &build, "pkg").unwrap_err();
        assert!(err.contains("unsafe path"), "{evil}: {err}");
        assert!(!d.path().join("evil").exists() && !Path::new("/tmp/rpyenv-evil-abs").exists());
    }
}

/// The package name is checked again where it is joined to the build directory (review
/// I2). Every escape here points inside the tempdir, at canaries.
#[test]
fn a_package_name_that_leaves_the_build_directory_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    std::fs::create_dir(d.path().join("victim")).unwrap();
    std::fs::write(d.path().join("victim/canary"), "c").unwrap();
    std::fs::create_dir(d.path().join("abs")).unwrap();
    std::fs::write(d.path().join("abs/canary"), "c").unwrap();
    std::fs::write(build.join("canary"), "c").unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz(&[("top/f", b"1")]));
    let abs = d.path().join("abs").display().to_string();
    for name in ["../victim", abs.as_str(), "..", ".", "a/b", ""] {
        let err = extract(&a, Kind::Gz, &build, name).unwrap_err();
        assert!(err.contains("invalid package name"), "{name}: {err}");
    }
    assert!(d.path().join("victim/canary").is_file());
    assert!(d.path().join("abs/canary").is_file());
    assert!(build.join("canary").is_file());
    assert!(a.is_file());
}

// allowlist D-72
#[cfg(unix)]
#[test]
fn a_symlink_that_leaves_the_tree_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    let a = write(
        d.path(),
        "a.tar.gz",
        &tgz_with_symlink("top/out", "../../outside", "top/out/f"),
    );
    let err = extract(&a, Kind::Gz, &build, "pkg").unwrap_err();
    assert!(err.contains("unsafe link"), "{err}");
    assert!(!d.path().join("outside").exists());
}

#[test]
fn staging_lives_inside_versions_and_commit_leaves_only_the_version() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    assert_eq!(
        t.stage_dir().parent().unwrap(),
        versions,
        "review focus 2: same filesystem"
    );
    let staged = t.stage_dir().join("prefix");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    t.place(&staged).unwrap();
    assert!(t.target().join(MARKER).is_file());
    assert!(!is_complete(&t.target()));
    t.commit().unwrap();
    let names: Vec<String> = std::fs::read_dir(&versions)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["3.12.0".to_string()]);
    assert!(is_complete(&versions.join("3.12.0")));
}

// allowlist D-58
#[test]
fn dropping_without_commit_restores_the_previous_version() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    std::fs::write(versions.join("3.12.0/bin/old"), "old").unwrap();
    {
        let mut t = Txn::begin(&versions, "3.12.0").unwrap();
        let staged = t.stage_dir().join("p");
        std::fs::create_dir_all(staged.join("bin")).unwrap();
        std::fs::write(staged.join("bin/new"), "new").unwrap();
        t.place(&staged).unwrap();
        assert!(versions.join("3.12.0/bin/new").is_file());
    }
    assert!(versions.join("3.12.0/bin/old").is_file());
    assert!(!versions.join("3.12.0/bin/new").exists());
    let names: Vec<String> = std::fs::read_dir(&versions)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["3.12.0".to_string()],
        "no .tmp, .old or .lock left"
    );
}

/// Replacing a version keeps pyenv-virtualenv's `envs/` and the user's site-packages
/// entries, as upstream's build over the old tree does (review I1).
// allowlist D-58
#[test]
fn commit_carries_over_envs_and_site_packages_entries() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    let old = versions.join("3.12.0");
    std::fs::create_dir_all(old.join("bin")).unwrap();
    std::fs::create_dir_all(old.join("envs/myenv")).unwrap();
    std::fs::write(old.join("envs/myenv/pyvenv.cfg"), "home = x\n").unwrap();
    let old_sp = old.join("lib/python3.12/site-packages");
    std::fs::create_dir_all(&old_sp).unwrap();
    std::fs::write(old_sp.join("userpkg.py"), "user").unwrap();
    std::fs::write(old_sp.join("pip.py"), "old pip").unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    let staged = t.stage_dir().join("p");
    let new_sp = staged.join("lib/python3.12/site-packages");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    std::fs::create_dir_all(&new_sp).unwrap();
    std::fs::write(new_sp.join("pip.py"), "new pip").unwrap();
    t.place(&staged).unwrap();
    t.commit().unwrap();
    let target = versions.join("3.12.0");
    assert_eq!(
        std::fs::read_to_string(target.join("envs/myenv/pyvenv.cfg")).unwrap(),
        "home = x\n"
    );
    let sp = target.join("lib/python3.12/site-packages");
    assert_eq!(
        std::fs::read_to_string(sp.join("userpkg.py")).unwrap(),
        "user"
    );
    assert_eq!(
        std::fs::read_to_string(sp.join("pip.py")).unwrap(),
        "new pip",
        "an entry in both keeps the new tree's copy"
    );
    assert!(!versions.join(".old-3.12.0").exists());
    assert!(is_complete(&target));
}

/// Scripts pip put in `bin/` are carried over; entries in both trees keep the new tree's
/// copy, and a symlink moves as a link (review follow-up F1b).
// allowlist D-58
#[test]
fn commit_carries_over_bin_entries_the_new_tree_lacks() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    let old = versions.join("3.12.0");
    std::fs::create_dir_all(old.join("bin")).unwrap();
    std::fs::write(old.join("bin/black"), "#!/x/bin/python3.12\n").unwrap();
    std::fs::write(old.join("bin/python3.12"), "old python").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("black", old.join("bin/black-link")).unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    let staged = t.stage_dir().join("p");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    std::fs::write(staged.join("bin/python3.12"), "new python").unwrap();
    t.place(&staged).unwrap();
    t.commit().unwrap();
    let bin = versions.join("3.12.0/bin");
    assert_eq!(
        std::fs::read_to_string(bin.join("black")).unwrap(),
        "#!/x/bin/python3.12\n"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("python3.12")).unwrap(),
        "new python"
    );
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(bin.join("black-link")).unwrap(),
        std::path::PathBuf::from("black")
    );
    assert!(!versions.join(".old-3.12.0").exists());
}

/// The Windows layout (`Lib/site-packages`, for M2b) is carried over too, and a
/// site-packages directory the new tree lacks is created.
#[test]
fn commit_carries_over_the_windows_site_packages_layout() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    let old = versions.join("3.12.0");
    std::fs::create_dir_all(old.join("bin")).unwrap();
    std::fs::create_dir_all(old.join("Lib/site-packages/userpkg")).unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    let staged = t.stage_dir().join("p");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    t.place(&staged).unwrap();
    t.commit().unwrap();
    assert!(versions.join("3.12.0/Lib/site-packages/userpkg").is_dir());
    assert!(!versions.join(".old-3.12.0").exists());
}

/// A carry-over that fails loses nothing: the previous tree stays at `.old-<name>` and the
/// commit still completes.
#[test]
fn a_failed_carry_over_keeps_the_previous_installation() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    let old = versions.join("3.12.0");
    std::fs::create_dir_all(old.join("bin")).unwrap();
    std::fs::create_dir_all(old.join("lib/python3.12/site-packages")).unwrap();
    std::fs::write(old.join("lib/python3.12/site-packages/userpkg.py"), "user").unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    let staged = t.stage_dir().join("p");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    // A file where the new tree's site-packages directory would be: nothing can move in.
    std::fs::create_dir_all(staged.join("lib/python3.12")).unwrap();
    std::fs::write(staged.join("lib/python3.12/site-packages"), "in the way").unwrap();
    t.place(&staged).unwrap();
    t.commit().unwrap();
    assert!(is_complete(&versions.join("3.12.0")));
    assert_eq!(
        std::fs::read_to_string(
            versions.join(".old-3.12.0/lib/python3.12/site-packages/userpkg.py")
        )
        .unwrap(),
        "user"
    );
}

/// A process killed during the carry-over (after the marker was removed) leaves a complete
/// target and `.old`: the next `begin` finishes the carry-over before dropping `.old`.
#[test]
fn begin_finishes_an_interrupted_carry_over() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join(".old-3.12.0/envs/e")).unwrap();
    std::fs::write(versions.join(".old-3.12.0/envs/e/pyvenv.cfg"), "c").unwrap();
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    let t = Txn::begin(&versions, "3.12.0").unwrap();
    assert!(versions.join("3.12.0/envs/e/pyvenv.cfg").is_file());
    assert!(!versions.join(".old-3.12.0").exists());
    drop(t);
}

/// The version name must be one directory under `versions/` (review I2); nothing is
/// created before the check.
#[test]
fn begin_refuses_a_name_that_is_not_one_plain_component() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    std::fs::create_dir(root.path().join("x")).unwrap();
    std::fs::write(root.path().join("x/canary"), "c").unwrap();
    for name in ["../x", "..", ".", "a/b", ""] {
        let err = Txn::begin(&versions, name).err().unwrap();
        assert_eq!(err, format!("pyenv: invalid version name: {name}"));
    }
    assert!(root.path().join("x/canary").is_file());
    assert!(!root.path().join(".locks").exists());
    assert_eq!(std::fs::read_dir(&versions).unwrap().count(), 0);
}

#[test]
fn dropping_a_fresh_install_removes_it() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    {
        let mut t = Txn::begin(&versions, "3.12.0").unwrap();
        let staged = t.stage_dir().join("p");
        std::fs::create_dir_all(&staged).unwrap();
        t.place(&staged).unwrap();
    }
    assert_eq!(std::fs::read_dir(&versions).unwrap().count(), 0);
}

// allowlist D-58
#[test]
fn a_second_install_of_the_same_name_is_refused_while_the_first_runs() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    let _first = Txn::begin(&versions, "3.12.0").unwrap();
    let err = Txn::begin(&versions, "3.12.0").err().unwrap();
    assert_eq!(
        err,
        format!(
            "pyenv: another install of 3.12.0 is in progress ({})",
            root.path().join(".locks").join("install-3.12.0").display()
        )
    );
    assert!(
        Txn::begin(&versions, "3.11.0").is_ok(),
        "other names are independent"
    );
}

#[test]
fn a_leftover_lock_file_without_a_holder_is_reused() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    let locks = root.path().join(".locks");
    std::fs::create_dir(&locks).unwrap();
    std::fs::write(
        locks.join("install-3.12.0"),
        "4194305
",
    )
    .unwrap();
    assert!(Txn::begin(&versions, "3.12.0").is_ok());
}

#[test]
fn a_failed_marker_write_changes_nothing_and_keeps_the_old_version() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    std::fs::write(versions.join("3.12.0/bin/old"), "old").unwrap();
    {
        let mut t = Txn::begin(&versions, "3.12.0").unwrap();
        // A file where a directory is expected: the marker cannot be written inside it.
        let staged = t.stage_dir().join("not-a-dir");
        std::fs::write(&staged, "x").unwrap();
        assert!(t.place(&staged).is_err());
        assert!(!t.placed());
        assert!(versions.join("3.12.0/bin/old").is_file());
    }
    assert!(versions.join("3.12.0/bin/old").is_file());
    assert!(!versions.join("3.12.0").join(MARKER).exists());
    let names: Vec<String> = std::fs::read_dir(&versions)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["3.12.0".to_string()]);
}

#[test]
fn begin_puts_back_a_version_set_aside_by_a_killed_install() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join(".old-3.12.0/bin")).unwrap();
    let t = Txn::begin(&versions, "3.12.0").unwrap();
    assert!(versions.join("3.12.0/bin").is_dir());
    assert!(!versions.join(".old-3.12.0").exists());
    drop(t);
}

#[test]
fn begin_drops_a_leftover_old_copy_when_the_target_is_complete() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join(".old-3.12.0/bin")).unwrap();
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    let t = Txn::begin(&versions, "3.12.0").unwrap();
    assert!(!versions.join(".old-3.12.0").exists());
    assert!(is_complete(&versions.join("3.12.0")));
    drop(t);
}

#[test]
fn begin_restores_the_old_copy_over_an_incomplete_target() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join(".old-3.12.0/bin")).unwrap();
    std::fs::write(versions.join(".old-3.12.0/bin/old"), "old").unwrap();
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    std::fs::write(versions.join("3.12.0").join(MARKER), "").unwrap();
    let t = Txn::begin(&versions, "3.12.0").unwrap();
    assert!(versions.join("3.12.0/bin/old").is_file());
    assert!(!versions.join("3.12.0").join(MARKER).exists());
    assert!(!versions.join(".old-3.12.0").exists());
    drop(t);
}

#[test]
fn place_refuses_to_overwrite_an_existing_old_copy() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    std::fs::create_dir_all(versions.join(".old-3.12.0/bin")).unwrap();
    std::fs::write(versions.join(".old-3.12.0/bin/keep"), "k").unwrap();
    let staged = t.stage_dir().join("p");
    std::fs::create_dir_all(&staged).unwrap();
    assert!(t.place(&staged).is_err());
    assert!(versions.join(".old-3.12.0/bin/keep").is_file());
    assert!(!t.placed());
}

// Unix only: Windows deletes a file where `remove_dir_all` expects a directory.
#[cfg(unix)]
#[test]
fn a_late_extraction_failure_leaves_no_scratch_directory() {
    let d = tempfile::tempdir().unwrap();
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    // `pkg` is a file, so the final rename into place fails after extraction succeeded.
    std::fs::write(build.join("pkg"), "in the way").unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz(&[("top/f", b"1")]));
    assert!(extract(&a, Kind::Gz, &build, "pkg").is_err());
    let names: Vec<String> = std::fs::read_dir(&build)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["pkg".to_string()]);
}

// A lone top-level symlink to a directory is not the tree: it is checked with
// `symlink_metadata`, so `pkg` stays a real directory that holds the link.
// allowlist D-72
#[cfg(unix)]
#[test]
fn a_lone_top_level_symlink_is_not_taken_for_the_tree() {
    let d = tempfile::tempdir().unwrap();
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    b.append_link(&mut h, "top", ".").unwrap();
    let a = write(
        d.path(),
        "a.tar.gz",
        &b.into_inner().unwrap().finish().unwrap(),
    );
    let out = extract(&a, Kind::Gz, &build, "pkg").unwrap();
    let m = std::fs::symlink_metadata(&out).unwrap();
    assert!(m.is_dir() && !m.file_type().is_symlink(), "{out:?}");
    assert!(std::fs::symlink_metadata(out.join("top"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn a_single_top_level_file_goes_inside_the_package_directory() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz(&[("only.txt", b"x")]));
    let out = extract(&a, Kind::Gz, d.path(), "pkg").unwrap();
    assert!(out.join("only.txt").is_file());
}

#[cfg(unix)]
fn tgz_with_hardlink(target: &str) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut f = tar::Header::new_gnu();
    f.set_size(1);
    f.set_mode(0o644);
    b.append_data(&mut f, "top/real", &b"x"[..]).unwrap();
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Link);
    h.set_size(0);
    b.append_link(&mut h, "top/hard", target).unwrap();
    b.into_inner().unwrap().finish().unwrap()
}

// allowlist D-72
#[cfg(unix)]
#[test]
fn a_hard_link_inside_the_tree_extracts() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz_with_hardlink("top/real"));
    let out = extract(&a, Kind::Gz, d.path(), "pkg").unwrap();
    assert_eq!(std::fs::read(out.join("hard")).unwrap(), b"x");
}

// allowlist D-72
#[cfg(unix)]
#[test]
fn a_hard_link_to_a_path_outside_the_tree_is_refused() {
    for target in ["../../etc/passwd", "/etc/passwd"] {
        let d = tempfile::tempdir().unwrap();
        let build = d.path().join("build");
        std::fs::create_dir(&build).unwrap();
        let a = write(d.path(), "a.tar.gz", &tgz_with_hardlink(target));
        let err = extract(&a, Kind::Gz, &build, "pkg").unwrap_err();
        assert!(err.contains("unsafe link"), "{target}: {err}");
    }
}

// allowlist D-72
#[cfg(unix)]
#[test]
fn setuid_and_setgid_bits_are_not_extracted() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut h = tar::Header::new_gnu();
    h.set_size(1);
    h.set_mode(0o6755);
    b.append_data(&mut h, "top/suid", &b"x"[..]).unwrap();
    let bytes = b.into_inner().unwrap().finish().unwrap();
    let a = write(d.path(), "a.tar.gz", &bytes);
    let out = extract(&a, Kind::Gz, d.path(), "pkg").unwrap();
    let mode = std::fs::metadata(out.join("suid"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755);
}
