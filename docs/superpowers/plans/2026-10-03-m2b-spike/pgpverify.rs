//! Detached OpenPGP signature verification with rpgp (`pgp` crate), pure Rust.
//!
//! rpgp's `Signature::verify` is cryptographic only: it does not look at key expiry,
//! revocation, key flags or binding signatures (read in pgp 0.21.0 source, and measured with
//! the synthetic expired key in REPORT.md). The policy below is ours:
//! * the issuer (fingerprint or key-id subpacket) must be the primary key or a subkey of a
//!   key in `keys_armored` (the caller pins the keyring, so trust = membership);
//! * the key must have at least one valid self-certification (third-party certifications are
//!   ignored; `verify_bindings` is NOT used because it fails on them), and a signing subkey
//!   a valid binding signature;
//! * a signing subkey must carry the sign flag in its binding signature;
//! * the signature must be a binary-document signature (type 0x00);
//! * the signature must have been made before the key (and subkey) expired, if it expires;
//!   a signature made while the key was valid stays valid after the key expires;
//! * a key that carries any revocation signature is rejected.
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey, SignedPublicSubKey};
use pgp::packet::{Signature, SignatureType};
use pgp::types::Tag;
use pgp::types::KeyDetails;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

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
        match SignedPublicKey::from_armor_many(keys_armored[st..end].as_bytes()) {
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

/// Verify `asc` (an armored detached signature) over the file `data`, against the keys in
/// `keys_armored` (one or more armored public keys). Returns the PRIMARY key fingerprint
/// (uppercase hex) of the signer.
pub fn verify_detached(data: &Path, asc: &[u8], keys_armored: &str) -> Result<String, String> {
    let (sig, _headers) =
        DetachedSignature::from_armor_single(asc).map_err(|e| format!("bad signature armor: {e}"))?;
    let sig = sig.signature;
    match sig.typ() {
        Some(SignatureType::Binary) => {}
        other => return Err(format!("not a binary-document signature: {other:?}")),
    }
    let sig_time = sig.created().ok_or("signature has no creation time")?.as_secs() as u64;

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
            sig.issuer_fingerprint().iter().map(|f| format!("{f:X}")).collect::<Vec<_>>(),
            sig.issuer_key_id().iter().map(|i| format!("{i:?}")).collect::<Vec<_>>()
        ));
    }

    let mut last_err = String::new();
    for c in candidates {
        let open = || File::open(data).map(BufReader::new).map_err(|e| format!("open {}: {e}", data.display()));
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
        for s in primary.details.direct_signatures.iter().filter(|s| issuer_matches(s, pk)) {
            if s.verify_key(pk).is_ok() {
                self_sigs.push(s);
            }
        }
        if self_sigs.is_empty() {
            return Err(format!("key {} has no valid self-signature", fpr_hex(primary)));
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
                return Err(format!("signature made at {sig_time}, after key {} expired at {exp}", fpr_hex(primary)));
            }
        }
        if let Signer::Sub(_, sk) = c {
            let bindings: Vec<&Signature> =
                sk.signatures.iter().filter(|b| b.verify_subkey_binding(pk, &sk.key).is_ok()).collect();
            if bindings.is_empty() {
                return Err(format!("subkey {} has no valid binding signature", fpr_hex(sk)));
            }
            if !bindings.iter().any(|b| b.key_flags().sign()) {
                return Err(format!("subkey {} is not a signing subkey", fpr_hex(sk)));
            }
            if let Some(exp) = expiry_from(sk.key.created_at().as_secs(), &bindings) {
                if sig_time >= exp {
                    return Err(format!("signature made after subkey {} expired", fpr_hex(sk)));
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

/// rpgp's raw verdict, without the policy above (for the measurements only).
pub fn verify_detached_raw(data: &Path, asc: &[u8], keys_armored: &str) -> Result<String, String> {
    let (sig, _) = DetachedSignature::from_armor_single(asc).map_err(|e| e.to_string())?;
    let mut last = "no matching key".to_string();
    for k in parse_keyring(keys_armored).0 {
        let r = File::open(data).map_err(|e| e.to_string())?;
        match sig.signature.verify(&k, BufReader::new(r)) {
            Ok(()) => return Ok(fpr_hex(&k)),
            Err(e) => last = e.to_string(),
        }
        for sk in &k.public_subkeys {
            let r = File::open(data).map_err(|e| e.to_string())?;
            if sig.signature.verify(sk, BufReader::new(r)).is_ok() {
                return Ok(fpr_hex(&k));
            }
        }
    }
    Err(last)
}
