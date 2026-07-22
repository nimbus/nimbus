# NDS3 cycle 77: duplicate promise settle multipleResolves parity

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: `v2.8.3-nimbus.26` (`b5cb5b4d95e88b4eed5836a495605e05b53e288a`)

## Fixture

- `test/parallel/test-promise-swallowed-event.js` (node22 + node24)

The official fixture listens for Node's deprecated `process` `multipleResolves`
event and expects four duplicate promise settle notifications in order:
reject, resolve, resolve, reject. Before this cycle both required lanes ran the
fixture but observed zero handler calls.

## Root Cause

The Deno fork received V8 duplicate promise settle callbacks
(`PromiseRejectAfterResolved` and `PromiseResolveAfterResolved`) in
`deno_core`, but only used promise-rejection callbacks for unhandled rejection
bookkeeping. It did not preserve duplicate settle callbacks long enough for the
Node process polyfill to emit Node's `multipleResolves` event.

## Fork Fix

Changed the Nimbus Deno fork so duplicate settle callbacks are queued in
`ExceptionState`, drained into JavaScript during promise rejection processing,
and forwarded by the Node process polyfill as `process.emit("multipleResolves",
type, promise, reason)`. The polyfill emits the Node-compatible `DEP0160`
deprecation warning once when a listener handles the event.

Touched fork files:

- `libs/core/runtime/exception_state.rs`
- `libs/core/runtime/bindings.rs`
- `libs/core/ops_builtin.rs`
- `libs/core/ops_builtin_v8.rs`
- `libs/core/01_core.js`
- `libs/core/core.d.ts`
- `ext/node/polyfills/process.ts`

Published fork commit/tag:

```text
b5cb5b4d95 node: emit multipleResolves for duplicate promise settles
v2.8.3-nimbus.26
```

No fixture/checker was edited. No derived posture JSON was hand-edited. The
change preserves sandbox boundaries: it exposes isolate-local V8 promise state
to the already-loaded process polyfill and grants no host process, signal,
subprocess, filesystem, or network authority.

## Dynamic Proof

Scratch probe was added temporarily as `nds_probe` and removed before this
checkpoint.

Local Deno path proof before tagging:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 2.02s

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 1.91s
```

Adjacent deprecation-warning fixture proof after switching from
`util.deprecate()` to direct `process.emitWarning()`:

```text
NIMBUS_RECENSUS_FIXTURE='test/parallel/test-warn-multipleResolves.mjs'
NIMBUS_RECENSUS_LANE=node24
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out
```

Repinned immutable-tag rebuild proof after publishing `v2.8.3-nimbus.26` and
removing the local Cargo path override:

```bash
cargo clean -p deno_core -p deno_node
cargo test -p nimbus-runtime --lib nds_probe --no-run
```

Result:

```text
Removed 1066 files, 646.7MiB total
Compiling deno_core v0.404.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.26#b5cb5b4d)
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.26#b5cb5b4d)
Finished `test` profile [unoptimized + debuginfo] target(s) in 37.89s
```

Repinned node24 proof:

```text
(node:0) [DEP0160] DeprecationWarning: The multipleResolves event has been deprecated.
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 2.03s
```

Repinned node22 proof:

```text
(node:0) [DEP0160] DeprecationWarning: The multipleResolves event has been deprecated.
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 1.95s
```

Repinned adjacent warning proof:

```text
node_compat nds-probe node24 -> test/parallel/test-warn-multipleResolves.mjs
(node:0) [DEP0160] DeprecationWarning: The multipleResolves event has been deprecated.
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 1.94s
```

## Promotion Guard

Added
`crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle77_multiple_resolves.rs`
and included it from `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.

Final non-ignored promotion guard after removing the scratch probe:

```bash
cargo test -p nimbus-runtime --lib cycle77_multiple_resolves -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle77-multiple-resolves-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle77-multiple-resolves-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 882 filtered out; finished in 3.90s
```

## Regression Note

The adjacent `test/parallel/test-warn-multipleResolves.mjs` fixture dynamically
passes on the repinned tag, proving the warning path introduced for this event.
It was not promoted in this cycle because it is not part of the required gap set.

A focused `test/parallel/test-timers-timeout-promisified.js` probe failed on the
pre-existing harness/event-loop pending-promise assertion, not on
`multipleResolves`; it was not treated as a regression for this fork change.

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Generated posture after regeneration:

```text
node22 v8_isolate_required.gaps = 19, pass_rate_percent = 99.2
node24 v8_isolate_required.gaps = 25, pass_rate_percent = 98.96
unique required fixtures across node22/node24 = 26
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 20, pass_rate_percent = 99.15
node24 v8_isolate_required.gaps = 26, pass_rate_percent = 98.92
```

## Cleanup

- Removed scratch `nds_probe.rs` and its temporary `mod.rs` include.
- Removed the temporary local Deno Cargo path override before repinning.
- Restored unrelated Cargo.lock `itertools` dependency churn introduced by
  `cargo update -p deno_core`; the final lockfile diff is only the Deno tag/hash
  repin.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at the published tag:

```text
git status --short --branch
## nimbus/v2.8.3

git describe --tags --exact-match
v2.8.3-nimbus.26

git rev-parse --short=10 HEAD
b5cb5b4d95
```

- `git diff --check` passed with no output.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still
fails because the regenerated posture is node22=19 / node24=25, not 0/0. The
remaining failures are the known
private-plan/proof and closeout rows that unblock only when the gate reaches
literal green.
