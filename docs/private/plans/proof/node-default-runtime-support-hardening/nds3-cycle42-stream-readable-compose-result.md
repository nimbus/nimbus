# NDS3 Cycle 42: Readable compose stream parity

Date: 2026-06-13

## Scope

- Fixture: `test/parallel/test-stream-readable-compose.js`
- Lanes: node24
- Owner: nimbus/deno `ext/node`
- Deno fork: published `v2.8.3-nimbus.8` (`2ebf5b82b5`)
- rusty_v8 fork: unchanged, pinned to `v149.4.0-nimbus.1`

## Root Cause

The fixture exposed two Deno stream parity gaps.

First, `Readable.prototype.compose()` was installed through Deno's generic
`streamReturningOperators` wrapper. That wrapper applied `Readable.from(...)` to
the internal compose result, so `Readable.from([...]).compose(...)` returned a
plain Readable facade over the composed Duplex. The fixture expects the direct
composed Duplex surface, including `writable === false`.

Second, nested compose error propagation could stall. Deno's
`internal/streams/compose.js` used a custom `readable`/`read()` pump for Node
stream tails instead of Node's current `data`/`pause()`/`resume()` pump, and
`internal/streams/from.js` replaced `_destroy` without calling the destroy hook
supplied by `Duplex.from(async generator)`. The async generator transform was not
reliably aborted/unblocked, so the nested `assert.rejects(...).then(mustCall())`
case did not settle before the fixture's call checks.

Cycle 42 aligns those pieces with Node's current stream implementation:

- `stream.ts` installs `Readable.prototype.compose()` directly and exports
  `Stream.compose` from the internal static compose function.
- `operators.js` no longer treats `compose` as a stream-returning operator.
- `compose.js` uses Node's Node-stream tail pump.
- `from.js` preserves and calls the original `_destroy` before closing the
  iterator, matching Node's async-generator teardown path.

## Focused Proof

Local Deno-path probe after the fork fix:

```text
cargo clean -p deno_node
Removed 541 files, 361.1MiB total

set -o pipefail; cargo test -p nimbus-runtime --lib nds_probe --no-run 2>&1 | grep -iE 'error\[|Finished|warning:|failed to'
Finished `test` profile [unoptimized + debuginfo] target(s) in 14.23s

gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-stream-readable-compose.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 835 filtered out; finished in 2.02s
```

Deno fork integrity before publishing:

```text
git diff --check
deno fmt --check ext/node/polyfills/internal/streams/compose.js ext/node/polyfills/internal/streams/from.js ext/node/polyfills/internal/streams/operators.js ext/node/polyfills/stream.ts
Checked 4 files
```

Published tag:

```text
git commit -m "node(stream): align readable compose parity"
[nimbus/v2.8.3 2ebf5b82b5] node(stream): align readable compose parity

git tag v2.8.3-nimbus.8
git push origin HEAD
0e5617ac62..2ebf5b82b5  HEAD -> nimbus/v2.8.3

git push origin v2.8.3-nimbus.8
* [new tag]               v2.8.3-nimbus.8 -> v2.8.3-nimbus.8
```

Nimbus repin to the published tag:

```text
cargo update -p deno_node
Removing deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.7#0e5617ac)
Adding deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.8#2ebf5b82)
```

Published-tag focused rebuild and probe:

```text
cargo clean -p deno_node
Removed 541 files, 361.1MiB total

set -o pipefail; cargo test -p nimbus-runtime --lib nds_probe --no-run 2>&1 | grep -iE 'error\[|Finished|warning:|failed to'
Finished `test` profile [unoptimized + debuginfo] target(s) in 36.13s

gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-stream-readable-compose.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 835 filtered out; finished in 2.05s
```

Promoted non-ignored guard:

```text
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle42_stream_readable_compose -- --nocapture
node_compat node24-default-lane-executes-cycle42-stream-readable-compose-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 835 filtered out; finished in 2.02s
```

## Regeneration Checks

```text
cargo fmt --all --check
```

Passed with no output.

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
validated node-compat watchpoint catalog: 134 entries
```

Generated posture after regeneration:

```text
node22 58 97.56
node24 67 97.22
```

## Notes

- The temporary `.cargo/config.toml` path override was removed before the
  published-tag proof.
- The scratch `nds_probe` file/include was removed before promotion.
- No V8 or rusty_v8 changes were made.
