# CPython release-signing keys (Windows MSIs)

rpyenv checks every python.org MSI it installs against its detached OpenPGP signature
(`<file>.asc`), using only the three keys in this folder (spec §9.1). rpyenv is never the hash
authority: these keys are the release managers' own, and the signatures are python.org's.

Every installable Windows MSI signature on python.org was swept on 2026-10-03: 757 `.asc`
files, covering every single-file MSI from 2.4 to 3.4.4 and `core.msi.asc` in every
component-MSI folder from 3.5 to 3.13. Each signature's issuer was read with rpgp 0.21.0, and
exactly three keys signed all 757:

| File | Full fingerprint | Owner | Signs | Signatures | Source |
|---|---|---|---|---|---|
| `steve_dower.asc` | `7ED10B6531D7C8E1BC296021FC624643487034E5` | Steve Dower (Python Release Signing), RSA 4096, created 2015-04-06, no expiry | component MSIs 3.5.0–3.13.16 (99 version folders, pre-releases included); single MSIs 2.7.10–2.7.18 and their rc1s (17 versions) | 582 (SHA-256) | `https://keybase.io/stevedower/pgp_keys.asc?fingerprint=7ed10b6531d7c8e1bc296021fc624643487034e5`, linked as the "Windows binaries" key from `https://www.python.org/downloads/metadata/pgp/` |
| `martin_v_loewis.asc` | `CBC547978A3964D14B9AB36A6AF053F07D9DC8D2` | Martin v. Löwis, DSA, created 2002-12-18, no expiry | single MSIs 2.5.2–3.4.4, pre-releases included (83 versions) | 166 (SHA-1) | exported, by full fingerprint, from python.org's own former `https://www.python.org/static/files/pubkeys.txt` as archived at `http://web.archive.org/web/20190210175522id_/https://www.python.org/static/files/pubkeys.txt` (python.org now returns 404 for that file and for the link it gives for this key) |
| `anthony_baxter.asc` | `531F072D39700991925FED0C0EDDC5F26A45C816` | Anthony Baxter, DSA | single MSIs 2.4, 2.4.1–2.4.4, 2.5, 2.5.1 (7 versions) | 9 (SHA-1) | exported the same way, from the same archived `pubkeys.txt` |

Rules (enforced by `crates/pyenv/src/install/openpgp.rs` and its tests):

- **Pin full fingerprints.** The archived `pubkeys.txt` also holds short-ID impostor keys,
  for example `BA749AC731BE5A28A65446C02056FF2E487034E5` ("Totally Legit Signing Key",
  colliding with Dower's short ID `487034E5`) and an RSA "Martin v. Löwis"
  `CA599DBFA7022A2CDE6B5F879E63E87F7D9DC8D2`. Never ship that file as a keyring. A test checks
  that these three files hold exactly the three fingerprints above, and nothing else.
- **The signatures carry only 64-bit issuer key IDs** (`FC624643487034E5`, `6AF053F07D9DC8D2`,
  `0EDDC5F26A45C816`), not fingerprints. They are matched only against these three keys.
- **SHA-1 and DSA are accepted** for the Löwis and Baxter signatures. They are the only
  signatures python.org publishes for 2.4–3.4 (decision M2b-D3).
- **Expiry and revocation are checked by rpyenv**, because rpgp 0.21.0 checks neither
  (measured in the M2b spike with a synthetic expired key).

Verified 2026-10-03 with the policy code in the M2b spike:
- 2.4.4 and 2.5 (Baxter);
- 3.4.4 amd64 (Löwis);
- 2.7.18 amd64 and 3.10.11 amd64 `core.msi` (Dower).

Without Baxter's key, 2.4.4 failed with "no key in the keyring matches the signature issuer".
