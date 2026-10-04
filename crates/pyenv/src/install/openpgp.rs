//! Detached OpenPGP signatures on python.org's MSIs (spec §9.1), checked against the three
//! release-signing keys in `crates/pyenv/keys/` (provenance in its README). Every MSI signature
//! python.org publishes, 757 swept on 2026-10-03, is by one of them.
//!
//! rpgp's `Signature::verify` is cryptographic only: it checks no expiry, revocation, key flags
//! or binding signatures (read in pgp 0.21.0, and measured with a synthetic expired key). The
//! policy below is rpyenv's:
//! * the issuer (fingerprint or key-id subpacket) must be the primary key or a subkey of a key in
//!   the keyring (the keyring is pinned, so trust is membership);
//! * the key needs at least one valid self-certification (third-party certifications are
//!   ignored; `verify_bindings` is not used because it fails on them), and a signing subkey a
//!   valid binding with the sign flag;
//! * only binary-document signatures (type 0x00) are accepted;
//! * a signature over an MD5 or RIPEMD-160 digest is refused;
//! * the signature must predate the key's (and subkey's) expiry, if any: a signature made while
//!   the key was valid stays valid after it expires;
//! * a key with a valid self-revocation is rejected.
//!
//! SHA-1 and DSA are accepted: they are what the Löwis and Baxter keys sign 2.4–3.4 with.

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey, SignedPublicSubKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::packet::{Signature, SignatureType};
use pgp::types::KeyDetails;
use pgp::types::Tag;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Full fingerprints of the embedded keys: Steve Dower, Martin v. Löwis, Anthony Baxter.
pub const PINNED: [&str; 3] = [
    "7ED10B6531D7C8E1BC296021FC624643487034E5",
    "CBC547978A3964D14B9AB36A6AF053F07D9DC8D2",
    "531F072D39700991925FED0C0EDDC5F26A45C816",
];

const KEYS: [&str; 3] = [
    include_str!("../../keys/steve_dower.asc"),
    include_str!("../../keys/martin_v_loewis.asc"),
    include_str!("../../keys/anthony_baxter.asc"),
];

/// The embedded keyring: one armored block per key.
pub fn keyring() -> String {
    KEYS.join(
        "
",
    )
}

/// `verify_detached` against the embedded keyring, and the signer must be pinned.
pub fn verify_python_org(data: &Path, asc: &[u8]) -> Result<String, String> {
    let signer = verify_detached(data, asc, &keyring())?;
    if PINNED.contains(&signer.as_str()) {
        Ok(signer)
    } else {
        Err(format!(
            "signed by {signer}, which is not a CPython release-signing key"
        ))
    }
}

fn fpr_hex(k: &impl KeyDetails) -> String {
    format!("{:X}", k.fingerprint())
}

/// Seconds since epoch at which a key expires, from the newest self-signature that states one.
fn expiry_from(created: u32, sigs: &[&Signature]) -> Option<u64> {
    let newest = sigs
        .iter()
        .filter(|s| s.created().is_some())
        .max_by_key(|s| s.created().map(|t| t.as_secs()).unwrap_or(0))?;
    newest
        .key_expiration_time()
        .filter(|d| d.as_secs() != 0)
        .map(|d| created as u64 + d.as_secs() as u64)
}

fn issuer_matches(sig: &Signature, k: &impl KeyDetails) -> bool {
    let fps = sig.issuer_fingerprint();
    let ids = sig.issuer_key_id();
    fps.iter().any(|f| **f == k.fingerprint()) || ids.iter().any(|i| **i == k.legacy_key_id())
}

/// Parse every public key in an armored keyring. `from_armor_many` reads ONE armor block
/// (which may hold many keys); a keyring made by concatenating armored keys has several
/// blocks (measured: the second block was silently ignored), so split on the BEGIN line first.
pub fn parse_keyring(keys_armored: &str) -> (Vec<SignedPublicKey>, Vec<String>) {
    let mut keys = Vec::new();
    let mut parse_errors = Vec::new();
    const BEGIN: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----";
    let starts: Vec<usize> = keys_armored.match_indices(BEGIN).map(|(i, _)| i).collect();
    for (n, &st) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(keys_armored.len());
        match SignedPublicKey::from_armor_many(&keys_armored.as_bytes()[st..end]) {
            Ok((iter, _)) => {
                for k in iter {
                    match k {
                        Ok(k) => keys.push(k),
                        Err(e) => parse_errors.push(e.to_string()),
                    }
                }
            }
            Err(e) => parse_errors.push(e.to_string()),
        }
    }
    (keys, parse_errors)
}

enum Signer<'a> {
    Primary(&'a SignedPublicKey),
    Sub(&'a SignedPublicKey, &'a SignedPublicSubKey),
}

/// Digests a signature may not use: MD5 (never accepted, spec §9.3) and RIPEMD-160.
fn weak_hash(alg: HashAlgorithm) -> bool {
    matches!(alg, HashAlgorithm::Md5 | HashAlgorithm::Ripemd160)
}

/// Verify `asc` (an armored detached signature) over the file `data`, against the keys in
/// `keys_armored` (one or more armored public keys). Returns the PRIMARY key fingerprint
/// (uppercase hex) of the signer.
pub fn verify_detached(data: &Path, asc: &[u8], keys_armored: &str) -> Result<String, String> {
    let (sig, _headers) = DetachedSignature::from_armor_single(asc)
        .map_err(|e| format!("bad signature armor: {e}"))?;
    let sig = sig.signature;
    match sig.typ() {
        Some(SignatureType::Binary) => {}
        other => return Err(format!("not a binary-document signature: {other:?}")),
    }
    if let Some(alg) = sig.hash_alg().filter(|a| weak_hash(*a)) {
        return Err(format!("signature uses a weak hash: {alg}"));
    }
    let sig_time = sig
        .created()
        .ok_or("signature has no creation time")?
        .as_secs() as u64;

    let (keys, parse_errors) = parse_keyring(keys_armored);
    if keys.is_empty() {
        return Err(format!("no usable keys in keyring ({parse_errors:?})"));
    }

    let mut candidates = Vec::new();
    for k in &keys {
        if issuer_matches(&sig, k) {
            candidates.push(Signer::Primary(k));
        }
        for sk in &k.public_subkeys {
            if issuer_matches(&sig, sk) {
                candidates.push(Signer::Sub(k, sk));
            }
        }
    }
    if candidates.is_empty() {
        return Err(format!(
            "no key in the keyring matches the signature issuer {:?} / {:?}",
            sig.issuer_fingerprint()
                .iter()
                .map(|f| format!("{f:X}"))
                .collect::<Vec<_>>(),
            sig.issuer_key_id()
                .iter()
                .map(|i| format!("{i:?}"))
                .collect::<Vec<_>>()
        ));
    }

    let mut last_err = String::new();
    for c in candidates {
        let open = || {
            File::open(data)
                .map(BufReader::new)
                .map_err(|e| format!("open {}: {e}", data.display()))
        };
        let (primary, res) = match c {
            Signer::Primary(k) => (k, sig.verify(k, open()?)),
            Signer::Sub(k, sk) => (k, sig.verify(sk, open()?)),
        };
        if let Err(e) = res {
            last_err = format!("bad signature: {e}");
            continue;
        }
        // Cryptographically valid. Now the key policy.
        // Only self-signatures count. (`SignedPublicKey::verify_bindings` verifies EVERY
        // certification as a self-signature, so it fails on keys that carry third-party
        // certifications, e.g. Steve Dower's key as served in python.org's old pubkeys.txt.)
        let pk = &primary.primary_key;
        let mut self_sigs: Vec<&Signature> = Vec::new();
        for u in &primary.details.users {
            for s in u.signatures.iter().filter(|s| issuer_matches(s, pk)) {
                if s.verify_certification(pk, Tag::UserId, &u.id).is_ok() {
                    self_sigs.push(s);
                }
            }
        }
        for s in primary
            .details
            .direct_signatures
            .iter()
            .filter(|s| issuer_matches(s, pk))
        {
            if s.verify_key(pk).is_ok() {
                self_sigs.push(s);
            }
        }
        if self_sigs.is_empty() {
            return Err(format!(
                "key {} has no valid self-signature",
                fpr_hex(primary)
            ));
        }
        let revoked = primary
            .details
            .revocation_signatures
            .iter()
            .any(|s| issuer_matches(s, pk) && s.verify_key(pk).is_ok());
        if revoked {
            return Err(format!("key {} is revoked", fpr_hex(primary)));
        }
        if let Some(exp) = expiry_from(pk.created_at().as_secs(), &self_sigs) {
            if sig_time >= exp {
                return Err(format!(
                    "signature made at {sig_time}, after key {} expired at {exp}",
                    fpr_hex(primary)
                ));
            }
        }
        if let Signer::Sub(_, sk) = c {
            let bindings: Vec<&Signature> = sk
                .signatures
                .iter()
                .filter(|b| b.verify_subkey_binding(pk, &sk.key).is_ok())
                .collect();
            if bindings.is_empty() {
                return Err(format!(
                    "subkey {} has no valid binding signature",
                    fpr_hex(sk)
                ));
            }
            if !bindings.iter().any(|b| b.key_flags().sign()) {
                return Err(format!("subkey {} is not a signing subkey", fpr_hex(sk)));
            }
            if let Some(exp) = expiry_from(sk.key.created_at().as_secs(), &bindings) {
                if sig_time >= exp {
                    return Err(format!(
                        "signature made after subkey {} expired",
                        fpr_hex(sk)
                    ));
                }
            }
        }
        if (sig_time as u32) < pk.created_at().as_secs() {
            return Err("signature predates its key".into());
        }
        return Ok(fpr_hex(primary));
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::weak_hash;
    use pgp::crypto::hash::HashAlgorithm;

    /// MD5 is never accepted (spec §9.3), nor RIPEMD-160; SHA-1 is, for the Löwis and Baxter keys
    /// (Decision 5), and SHA-2 for Dower's.
    #[test]
    fn md5_and_ripemd160_signatures_are_weak() {
        assert!(weak_hash(HashAlgorithm::Md5));
        assert!(weak_hash(HashAlgorithm::Ripemd160));
        for ok in [
            HashAlgorithm::Sha1,
            HashAlgorithm::Sha256,
            HashAlgorithm::Sha384,
            HashAlgorithm::Sha512,
        ] {
            assert!(!weak_hash(ok), "{ok:?}");
        }
    }
}
