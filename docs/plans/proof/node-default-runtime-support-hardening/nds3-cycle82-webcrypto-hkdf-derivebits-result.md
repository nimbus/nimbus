# NDS3 Cycle 82 - WebCrypto HKDF deriveBits/deriveKey

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-webcrypto-derivebits-hkdf.js` in both required
lanes.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.31`
(`4a54d34322 crypto: align HKDF deriveBits parity`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.30` to `v2.8.3-nimbus.31`.

## Fork Fix

`ext/crypto/00_crypto.js` now matches Node's HKDF fixture expectations for:

- zero-length HKDF `deriveBits()` returning an empty `ArrayBuffer`
- HKDF missing `salt` / `info` / `hash` tagged as `ERR_MISSING_OPTION`
- null and non-byte-aligned KDF length error text
- deriveBits/deriveKey base-key usage and key-algorithm mismatch text
- AES-OCB derived-key length normalization for Node24's OpenSSL 3 fixture branch

## Dynamic Proof

Local fork override proof before publishing:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
```

Published-tag proof after removing the local Cargo path override and repinning
to `v2.8.3-nimbus.31`:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
```

Promotion guard:

```text
$ CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib cycle82_webcrypto_hkdf_derivebits -- --nocapture
node_compat node22-supported-lane-executes-cycle82-webcrypto-hkdf-derivebits-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle82-webcrypto-hkdf-derivebits-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 891 filtered out; finished in 4.28s
```

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 13, pass_rate_percent = 99.45
node24 gaps = 21, pass_rate_percent = 99.13
```

The gate remains red and honest; this cycle only removes the HKDF fixture from
both required lanes.
