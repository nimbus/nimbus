# NDS3 Cycle 84 - WebCrypto wrapKey/unwrapKey

Date: 2026-06-14

## Scope

Promoted `test/parallel/test-webcrypto-wrap-unwrap.js` in both required lanes.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.33`
(`f9c030165f crypto: align Node wrap key edge cases`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.32` to `v2.8.3-nimbus.33`.

## Fork Fix

`ext/crypto/00_crypto.js` now matches Node's wrap/unwrap fixture expectations for:

- wrapKey/unwrapKey key-algorithm mismatch and key-usage error messages
- AES-KW wrapping of JWK data padded to an 8-byte boundary before wrapping
- EC exportKey wrong-format/wrong-key-type failures reported as `NotSupportedError`
  with Node-compatible format/type messages

## Nimbus Harness Fix

`test/parallel/test-webcrypto-wrap-unwrap.js` now has a fixture-specific finite
slow budget in the node compatibility harness. The official fixture runs a broad
WebCrypto matrix; Node22's older fixture is larger, and Node24 sits close to the
default 35s wall-clock budget on loaded hosts. The promotion still runs the
official fixture and requires `passed=1, skipped=0, failed=0`.

## Dynamic Proof

Local fork override proof before publishing:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Published-tag proof after removing the local Cargo path override and repinning
to `v2.8.3-nimbus.33`:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-webcrypto-wrap-unwrap.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures/crypto:test/fixtures/keys:test/fixtures/webcrypto" CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe --locked -- --ignored --nocapture
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 894 filtered out; finished in 27.69s

$ /opt/homebrew/bin/gtimeout -s KILL 160 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-webcrypto-wrap-unwrap.js" NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures/crypto:test/fixtures/keys" CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_probe --locked -- --ignored --nocapture
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 894 filtered out; finished in 85.27s
```

Promotion guard:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 260 env CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib cycle84_webcrypto_wrap_unwrap --locked -- --nocapture
node_compat node22-supported-lane-executes-cycle84-webcrypto-wrap-unwrap-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle84-webcrypto-wrap-unwrap-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 894 filtered out; finished in 132.80s
```

Harness budget guard:

```text
$ /opt/homebrew/bin/gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib node_compat_harness_wall_clock_timeout_tracks_fixture_runtime_budget --locked -- --nocapture
test runtime::tests::node_compat::node_compat_harness_wall_clock_timeout_tracks_fixture_runtime_budget ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 895 filtered out; finished in 0.00s
```

Formatting:

```text
$ cargo fmt --all --check
```

passed with no output.

## Regenerated Counts

After `classifications.py sync --lane all` and the standard status/dashboard/
trends/publish_evidence/default_support_posture/required_surface_blockers
regeneration pipeline:

```text
node22 gaps = 11, pass_rate_percent = 99.53
node24 gaps = 19, pass_rate_percent = 99.21
```

The gate remains red and honest; this cycle only removes the wrap/unwrap fixture
from both required lanes.
