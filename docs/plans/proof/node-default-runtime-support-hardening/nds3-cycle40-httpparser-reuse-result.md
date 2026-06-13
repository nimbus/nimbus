# NDS3 cycle 40 result - HTTP parser async resource lifecycle

Date: 2026-06-13

## Scope

Promoted the required fixture on both default lanes:

- `test/async-hooks/test-httpparser-reuse.js` (node22, node24)

Fork state:

- Deno fork advanced from `v2.8.3-nimbus.6` (`7a0edfb282`) to
  `v2.8.3-nimbus.7` (`0e5617ac62`).
- rusty_v8 unchanged at `v149.4.0-nimbus.1`.
- No local Cargo path override remains.

Nimbus changes:

- Repinned all Deno-family workspace dependencies to `v2.8.3-nimbus.7`.
- Added a non-ignored cycle-40 guard for node22 and node24.

## Root Cause

The fixture verifies that async hook resource objects emitted while reusing HTTP
parsers and TCP handles are not themselves reused, and that parser provider
types match Node's native split:

- request parser: `HTTPINCOMINGMESSAGE`
- response parser: `HTTPCLIENTREQUEST`

Deno's JS HTTP parser wrapper had two mismatches:

- The server-side parser was initialized with `{}`, so it did not carry a
  Node-shaped server async resource.
- The parser wrapper derived its async resource type from the JS resource object
  rather than from the parser mode. Node's `node_http_parser.cc` derives the
  provider from `HTTP_REQUEST` vs `HTTP_RESPONSE`.

After fixing those, the next assertion exposed native handle resource reuse:
Deno re-emitted `init` for the same `TCPWrap` object when a handle was reused.
The fix keeps first-time handle init resources unchanged and emits a fresh
`ReusedHandle` wrapper for repeated native handle init events.

Pre-fix focused census:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/async-hooks/test-httpparser-reuse.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:
2 !== 0
at Timeout.verify (.../test/async-hooks/test-httpparser-reuse.js:67:10)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 831 filtered out; finished in 2.26s
```

## Proof

Local-fork focused censuses:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/async-hooks/test-httpparser-reuse.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 831 filtered out; finished in 2.26s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/async-hooks/test-httpparser-reuse.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 831 filtered out; finished in 2.17s
```

Published fork:

```bash
cd /Users/jack/src/github.com/nimbus/deno
git commit -m "node(http): align parser async resource lifecycle"
git tag v2.8.3-nimbus.7
git push origin nimbus/v2.8.3
git push origin v2.8.3-nimbus.7
```

```text
[nimbus/v2.8.3 0e5617ac62] node(http): align parser async resource lifecycle
To github.com:nimbus/deno.git
   7a0edfb282..0e5617ac62  nimbus/v2.8.3 -> nimbus/v2.8.3
 * [new tag]               v2.8.3-nimbus.7 -> v2.8.3-nimbus.7
```

Repinned-tag focused censuses:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/async-hooks/test-httpparser-reuse.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 831 filtered out; finished in 2.31s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/async-hooks/test-httpparser-reuse.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 831 filtered out; finished in 2.24s
```

Promoted non-ignored guard:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib \
  cycle40_httpparser_reuse -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle40-httpparser-reuse-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle40-httpparser-reuse-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 831 filtered out; finished in 4.44s
```

Regeneration and lightweight checks:

```bash
cargo fmt --all --check
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do \
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; \
done
```

Generated posture after cycle 40:

```text
node22 59 97.51
node24 69 97.14
```

Note: a narrow `deno fmt --check` on the changed Deno polyfill files reported
large pre-existing formatter churn in generated/IIFE-shaped files, so it was not
used as proof for this fork patch. `git diff --check` was clean, and the focused
Cargo rebuilds above proved the changed snapshot executes correctly.

## Result

`test-httpparser-reuse.js` is no longer a `v8_isolate_required` gap in either
default lane. Gate remains red and honest:

- node22: 59 gaps
- node24: 69 gaps
