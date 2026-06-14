# NDS3 Cycle 86 - DES3-wrap provider boundary

Date: 2026-06-14

## Scope

Reclassified `test/parallel/test-crypto-des3-wrap.js` out of the
`v8_isolate_required` denominator in both the Node22 and Node24 required lanes.

No `nimbus/deno` fork code changed in this cycle. No V8 or `rusty_v8` code
changed. No upstream Node fixture or checker was edited.

The fork/pin baseline stayed:

- `nimbus/deno` `v2.8.3-nimbus.34`
  (`1dfa4fd884 crypto: align WebCrypto AES parity`)
- `nimbus/rusty_v8` `v149.4.0-nimbus.1`

## Source-Confirmed Disposition

The official Node22 and Node24 fixtures are identical for this case. Before
asserting any DES3-wrap behavior, the fixture asks Node's native crypto provider
for the available cipher inventory:

```js
const ciphers = crypto.getCiphers();

if (!ciphers.includes('des3-wrap'))
  common.skip('des3-wrap cipher is not available');
```

Only if the host provider advertises `des3-wrap` does the fixture continue into
`createCipheriv()` / `createDecipheriv()` wrap/unwrap checks.

This is OpenSSL/native-provider composition, not a portable V8-isolate
Application API guarantee. Cycle 86 therefore maps the watchpoint to the same
`upstream_or_platform_boundary` disposition used for other source-confirmed
provider/platform fixtures while leaving the ignored watchpoint available as a
future tripwire.

## Dynamic Evidence

Cycle 85's broad crypto probe already retained the required diagnostics that
exercise this fixture on the current `v2.8.3-nimbus.34` / `v149.4.0-nimbus.1`
baseline.

Node22:

```text
target/node-compat-diagnostics/nds3-cycle85-crypto-probe/batch/node22__nds_probe_crypto_node22__summary.json
selected=3, passed=1, skipped=1, failed=1
skipped_paths includes test/parallel/test-crypto-des3-wrap.js
```

Node24:

```text
target/node-compat-diagnostics/nds3-cycle85-crypto-probe/batch/node24__nds_probe_crypto_node24__summary.json
selected=8, passed=0, skipped=1, failed=7
skipped_paths includes test/parallel/test-crypto-des3-wrap.js
```

Both lanes reached the fixture's own `common.skip("des3-wrap cipher is not
available")` path. No implementation promotion was inferred from this skip; the
skip is supporting dynamic evidence for the source-confirmed provider-boundary
classification.

## Regeneration And Checks

The classification/posture pipeline was regenerated with:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Generator checks:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --preserve-existing --check
node20.json is up to date
node22.json is up to date
node24.json is up to date
node26.json is up to date

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass

$ cargo fmt --all --check
pass
```

The aggregate verifier remains red, as expected:

```text
$ bash scripts/verify-node-default-runtime-support-hardening.sh
Summary: 13 passed / 21 failed
```

The remaining failures are still gate/support-evidence closeout failures. The
gate-critical step 9 is still red because nonzero required gaps remain.

## Regenerated Counts

After the regenerated posture:

```text
node22 gaps = 10, pass_rate_percent = 99.58
node24 gaps = 17, pass_rate_percent = 99.29
```

The gate remains red and honest; this cycle removes one Node22 and one Node24
V8-isolate-required gap without changing runtime behavior.
