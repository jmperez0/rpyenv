# Synthetic OpenPGP fixtures

These are throwaway test keys, not CPython's. Every file was made with GnuPG 2.4.9 in a scratch
`GNUPGHOME` on 2026-10-03, and every signature covers `data.bin`.

| Files | Key | Verdict rpyenv must give (checked with the M2b spike's verifier) |
|---|---|---|
| `expiring_pubkey.asc` with `expiring_sig_before.asc` | `7832A9A8CBEB323CDC4667FB15C5BDFBB6467F16`: created 2020-01-01, expires 2021-01-01 (`--faked-system-time`) | OK: the signature dates from 2020-06, while the key was valid |
| `expiring_pubkey.asc` with `expiring_sig_after.asc` | the same key | Error: the signature dates from 2022-06, after expiry. rpgp 0.21.0 alone accepts it |
| `revoked_pubkey.asc` with `revoked_sig.asc` | `EE4A1DA1A724D20C73D9E6D1E337CC15CA8F6A32`: ed25519, with its own revocation certificate imported | Error: the key is revoked |
| `subkey_pubkey.asc` with `subkey_sig.asc` | primary `52324C2D3F5163AE5D2C704B0E3C5FEAE0D23397` (certify only), signing subkey `0D70DE5E4E090883DCB409BE5E8D79A1B6AF1FC9` | OK, reporting the primary fingerprint |
