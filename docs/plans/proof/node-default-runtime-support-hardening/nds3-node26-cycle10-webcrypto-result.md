# NDS3 Node26 Cycle 10: WebCrypto Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the WebCrypto
surface. It adds a Node26 ignored broad WebCrypto watchpoint, promotes only the
fixture paths that were dynamically green in that broad run, and leaves every
skip or failure counted as a gap. No Deno fork changes, rusty_v8 changes,
fixture edits, checker edits, or generated false-green JSON hand edits were
made.

Before this wave, Node26 `v8_isolate_required` posture was `684` gaps /
`68.89%`.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_webcrypto_required_gap_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures and one self-skip.
- Fixture summary: `selected=44`, `passed=15`, `skipped=1`, `failed=28`.
- Skipped fixture: `test/parallel/test-webcrypto-derivebits-argon2.js`
  (`Argon2 requires OpenSSL >= 3.2`).
- Summary artifact:
  `/private/tmp/nds-node26-webcrypto-wave1/batch/node26__node26_current_lane_webcrypto_required_gap_watchpoint__summary.json`

## Promoted Fixtures

The 15 broad-batch passes were added to `WEBCRYPTO_PROMOTED_NODE26_PATHS` and
enforced by `node26_current_lane_executes_webcrypto_promoted_batch_fixture`.

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-webcrypto-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_webcrypto_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 938 filtered out`.
- Fixture summary: `selected=15`, `passed=15`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-webcrypto-promote1/batch/node26__node26_current_lane_executes_webcrypto_promoted_batch__summary.json`

Promoted fixture paths:

- `test/parallel/test-webcrypto-aead-decrypt-detached-buffer.js`
- `test/parallel/test-webcrypto-deduplicate-usages.js`
- `test/parallel/test-webcrypto-derivebits.js`
- `test/parallel/test-webcrypto-derivekey.js`
- `test/parallel/test-webcrypto-encrypt-decrypt-aes.js`
- `test/parallel/test-webcrypto-encrypt-decrypt-rsa.js`
- `test/parallel/test-webcrypto-encrypt-decrypt.js`
- `test/parallel/test-webcrypto-export-import.js`
- `test/parallel/test-webcrypto-getRandomValues.js`
- `test/parallel/test-webcrypto-keygen-kmac.js`
- `test/parallel/test-webcrypto-keygen.js`
- `test/parallel/test-webcrypto-random.js`
- `test/parallel/test-webcrypto-sign-verify-kmac.js`
- `test/parallel/test-webcrypto-sign-verify.js`
- `test/parallel/test-webcrypto-wrap-unwrap.js`

## Failure Grouping

Non-promoted WebCrypto broad failures/skips:

- CryptoKey hidden-slot and brand-reflection failures:
  `test-webcrypto-constructors.js`,
  `test-webcrypto-cryptokey-brand-check.js`,
  `test-webcrypto-cryptokey-clone-transfer.js`,
  `test-webcrypto-cryptokey-hidden-slots.js`, and
  `test-webcrypto-cryptokey-no-own-symbols.js`.
- Unsupported or not-yet-wired newer algorithms: TurboSHAKE, ML-KEM, ML-DSA,
  and Argon2. Argon2 self-skipped because this host/runtime reports OpenSSL
  below the fixture's required version.
- Node26 WebCrypto error-shape or algorithm-detail mismatches in CFRG/ECDH/HKDF
  derive bits/keys, digest, ChaCha20-Poly1305, EC/RSA import-export,
  `get-public-key`, ECDSA/EdDSA/HMAC/RSA sign/verify, and internal-slot
  fixtures.
- `test/parallel/test-webcrypto-promise-prototype-pollution.mjs`: ES module
  import of the CommonJS `../common/crypto.js` helper does not expose the
  expected `hasOpenSSL` named export.

## Generated Evidence

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Results:

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 145 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `[]`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `None`
- `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`: warnings `None`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `669` gaps, `69.58%`

The Node26 count moved from `684` gaps / `68.89%` to `669` gaps /
`69.58%`, burning 15 required-surface gaps in this wave.

The untracked public selector mirror
`docs/architecture/runtime/node-default-support-posture.{json,md}` was
refreshed from the generated private posture after regeneration and remains
unstaged.

## Next Node26 Work

The broad WebCrypto run leaves a useful failure map, but the next highest-yield
implementation waves remain outside this checkpoint: the `stream/iter` Deno
fork cluster, the `.mjs` CommonJS named-export interop cluster visible in
`test-webcrypto-promise-prototype-pollution.mjs` and the fs.cp family, and the
remaining fs/stream/process clusters. Continue with broad ignored batches and
promote only dynamically green subsets.
