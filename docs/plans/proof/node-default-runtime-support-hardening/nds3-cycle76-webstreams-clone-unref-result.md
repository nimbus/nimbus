# NDS3 cycle 76: WebStreams transfer bridge unref parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: `v2.8.3-nimbus.25` (`374f9844106ca04c294f70efd277d523561804c8`)

## Fixture

- `test/parallel/test-webstreams-clone-unref.js` (node22 + node24)

The official fixture transfers a `ReadableStream` and `WritableStream` through
`structuredClone(..., { transfer })`, asserts the cloned WebStreams preserve
their brands, and then relies on process/runtime liveness to settle. Before this
cycle both required lanes timed out at the harness wall-clock limit.

## Root Cause

Deno's WebStreams transfer code in `ext/web/06_streams.js` creates internal
`MessagePort` bridge pairs for cross-realm readable/writable stream transfer.
Those ports start unrefed, but `addEventListener("message", ...)` in
`ext/web/13_message_port.js` refs the port. The transferred stream bridge ports
therefore kept the isolate alive even after the fixture's assertions completed.

## Fork Fix

Changed the Nimbus Deno fork so both cross-realm stream bridge setup paths call
`port.unref()` after `port.start()`:

- `setUpCrossRealmTransformReadable(stream, port)`
- `setUpCrossRealmTransformWritable(stream, port)`

Published fork commit/tag:

```text
374f984410 web: unref transferred stream bridge ports
v2.8.3-nimbus.25
```

No fixture/checker was edited. No derived posture JSON was hand-edited. The
change preserves sandbox boundaries: it only removes internal bridge-port
liveness, and does not add host process, signal, subprocess, filesystem, or
network authority.

## Dynamic Proof

Scratch probe was added temporarily as `nds_probe` and removed before this
checkpoint.

Local Deno path proof before tagging:

```text
node_compat nds-probe node24 -> test/parallel/test-webstreams-clone-unref.js
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 -> test/parallel/test-webstreams-clone-unref.js
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Repinned immutable-tag rebuild proof after publishing `v2.8.3-nimbus.25` and
removing the local Cargo path override:

```bash
cargo clean -p deno_web
cargo test -p nimbus-runtime --lib nds_probe --no-run
```

Result:

```text
Compiling deno_web v0.282.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.25#374f9844)
Finished `test` profile [unoptimized + debuginfo] target(s)
```

Repinned node24 proof:

```text
node_compat nds-probe node24 -> test/parallel/test-webstreams-clone-unref.js
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 880 filtered out; finished in 2.02s
```

Repinned node22 proof:

```text
node_compat nds-probe node22 -> test/parallel/test-webstreams-clone-unref.js
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 880 filtered out; finished in 1.93s
```

## Promotion Guard

Added
`crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle76_webstreams_clone_unref.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard after removing the scratch probe:

```bash
cargo test -p nimbus-runtime --lib cycle76_webstreams_clone_unref -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle76-webstreams-clone-unref-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle76-webstreams-clone-unref-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 880 filtered out; finished in 4.06s
```

## Regression Checks

Existing WebStreams BYOB guard:

```text
cycle38_webstreams_byob:
node22 summary: selected=1, passed=1, skipped=0, failed=0
node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured
```

Focused WebStreams transform-member probe:

```text
NIMBUS_RECENSUS_FIXTURE='test/parallel/test-whatwg-webstreams-transform-stream-members.js'
NIMBUS_RECENSUS_LANE=node24
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 1.90s
```

A broad `cycle13_w1_common_batch` check was also attempted. It failed on the
unrelated `test/parallel/test-v8-string-is-one-byte-representation.js` fixture in
both lanes, while all other selected fixtures passed. That failure is not a
WebStreams regression signal.

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/tmp/nds-$s.log
done
```

Generated posture after regeneration:

```text
node22 v8_isolate_required.gaps = 20, pass_rate_percent = 99.15
node24 v8_isolate_required.gaps = 26, pass_rate_percent = 98.92
unique required fixtures across node22/node24 = 27
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 21, pass_rate_percent = 99.11
node24 v8_isolate_required.gaps = 27, pass_rate_percent = 98.88
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- Removed the temporary local Deno Cargo path override before repinning.
- Verified `.cargo/config.toml` has no `paths =` override:

```bash
rg -n '^paths =' .cargo/config.toml
```

Result: no output.

- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at the published tag:

```text
git status --short --branch
## nimbus/v2.8.3

git describe --tags --exact-match
v2.8.3-nimbus.25

git rev-parse --short=10 HEAD
374f984410
```

- `git diff --check` passed with no output.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still fails
because the regenerated posture is node22=20 / node24=26, not 0/0. The remaining
failures are the known private plan/proof and closeout rows that unblock only
when the gate reaches literal green.
