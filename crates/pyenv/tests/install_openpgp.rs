//! The OpenPGP verifier (plan M2b Task 1): real python.org signature, the pinned keyring, and
//! the policy rpgp itself lacks (expiry, revocation, subkeys).

use pgp::types::KeyDetails;
use pyenv::install::openpgp::{keyring, parse_keyring, verify_detached, verify_python_org, PINNED};
use std::path::{Path, PathBuf};

fn fx(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn key_file(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("keys")
            .join(name),
    )
    .unwrap()
}

const TOOLS: &str = "msi/tools-3.10.11-amd64.msi";
const DOWER: &str = "7ED10B6531D7C8E1BC296021FC624643487034E5";

#[test]
fn the_embedded_keyring_is_exactly_the_three_pinned_keys() {
    let (keys, errors) = parse_keyring(&keyring());
    assert!(errors.is_empty(), "{errors:?}");
    let mut got: Vec<String> = keys
        .iter()
        .map(|k| format!("{:X}", k.fingerprint()))
        .collect();
    got.sort();
    let mut want: Vec<String> = PINNED.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn a_real_python_org_msi_verifies_as_steve_dower() {
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    assert_eq!(verify_python_org(&fx(TOOLS), &asc).as_deref(), Ok(DOWER));
}

fn tampered(edit: impl Fn(&mut Vec<u8>)) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = std::fs::read(fx(TOOLS)).unwrap();
    edit(&mut bytes);
    let p = dir.path().join("tools.msi");
    std::fs::write(&p, bytes).unwrap();
    (dir, p)
}

#[test]
fn one_flipped_byte_fails() {
    let (_d, p) = tampered(|b| b[0x8000] ^= 1);
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    let e = verify_python_org(&p, &asc).unwrap_err();
    assert!(e.starts_with("bad signature"), "{e}");
}

#[test]
fn one_appended_byte_fails() {
    let (_d, p) = tampered(|b| b.push(0));
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    assert!(verify_python_org(&p, &asc)
        .unwrap_err()
        .starts_with("bad signature"));
}

#[test]
fn a_keyring_without_the_signer_fails() {
    let ring = key_file("martin_v_loewis.asc") + "\n" + &key_file("anthony_baxter.asc");
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    let e = verify_detached(&fx(TOOLS), &asc, &ring).unwrap_err();
    assert!(e.starts_with("no key in the keyring matches"), "{e}");
}

#[test]
fn garbage_is_not_a_signature() {
    let e = verify_python_org(&fx(TOOLS), b"not a signature").unwrap_err();
    assert!(e.starts_with("bad signature armor"), "{e}");
}

fn synthetic(sig: &str, key: &str) -> Result<String, String> {
    let asc = std::fs::read(fx(&format!("openpgp/{sig}"))).unwrap();
    let ring = std::fs::read_to_string(fx(&format!("openpgp/{key}"))).unwrap();
    verify_detached(&fx("openpgp/data.bin"), &asc, &ring)
}

#[test]
fn a_signature_made_while_the_key_was_valid_stays_valid_after_it_expires() {
    assert_eq!(
        synthetic("expiring_sig_before.asc", "expiring_pubkey.asc").as_deref(),
        Ok("7832A9A8CBEB323CDC4667FB15C5BDFBB6467F16")
    );
}

#[test]
fn a_signature_made_after_the_key_expired_fails() {
    let e = synthetic("expiring_sig_after.asc", "expiring_pubkey.asc").unwrap_err();
    assert!(e.contains("expired"), "{e}");
}

#[test]
fn a_revoked_key_fails() {
    let e = synthetic("revoked_sig.asc", "revoked_pubkey.asc").unwrap_err();
    assert!(e.contains("is revoked"), "{e}");
}

#[test]
fn a_signing_subkey_reports_its_primary_key() {
    assert_eq!(
        synthetic("subkey_sig.asc", "subkey_pubkey.asc").as_deref(),
        Ok("52324C2D3F5163AE5D2C704B0E3C5FEAE0D23397")
    );
}

#[test]
fn verify_python_org_refuses_a_key_that_is_not_pinned() {
    // A valid signature by a key outside the embedded keyring never matches an issuer there.
    let asc = std::fs::read(fx("openpgp/subkey_sig.asc")).unwrap();
    let e = verify_python_org(&fx("openpgp/data.bin"), &asc).unwrap_err();
    assert!(e.starts_with("no key in the keyring matches"), "{e}");
}
