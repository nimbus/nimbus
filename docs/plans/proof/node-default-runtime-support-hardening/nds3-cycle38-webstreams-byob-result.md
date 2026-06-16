# NDS3 cycle 38 result - WebStreams BYOB invalid-state errors

Date: 2026-06-13

## Scope

Fixed and promoted the required fixture on both default lanes:

- `test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js`
  (node22, node24)

Fork state:

- Deno fork change: `nimbus/deno` commit `7a0edfb282` tagged
  `v2.8.3-nimbus.6`.
- rusty_v8 unchanged at `v149.4.0-nimbus.1`.
- Nimbus is repinned to immutable `nimbus/deno` `v2.8.3-nimbus.6`.
- No local Cargo path override remains.

Nimbus changes:

- `Cargo.toml` and `Cargo.lock` now use `v2.8.3-nimbus.6` for the Deno-family
  git dependencies.
- A non-ignored cycle-38 guard promotes the BYOB fixture for node22 and node24.

## Root Cause

The fixture validates Node's error-code shape for invalid BYOB stream states:
detached BYOB views and zero-length BYOB reads must reject or throw `TypeError`
with `code: "ERR_INVALID_STATE"`. Deno's WebStreams implementation produced the
right `TypeError` name but did not attach the Node-compatible `code` property on
those invalid-state paths.

Pre-fix focused censuses:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal:
+ actual - expected
-   code: 'ERR_INVALID_STATE',
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 827 filtered out; finished in 2.03s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal:
+ actual - expected
-   code: 'ERR_INVALID_STATE',
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 827 filtered out; finished in 1.92s
```

## Fork Fix

Changed `ext/web/06_streams.js` in `nimbus/deno` to create invalid-state
`TypeError`s with `code = "ERR_INVALID_STATE"` for:

- `ReadableStreamBYOBReader.read()` zero-length/detached view rejections.
- `ReadableStreamBYOBRequest.respond()` invalidated/detached request throws.
- `ReadableStreamBYOBRequest.respondWithNewView()` invalidated/detached request
  throws.

Local-fork proof used a temporary Cargo path override:

```toml
paths = ["/Users/jack/src/github.com/nimbus/deno/ext/web"]
```

and rebuilt only `deno_web`:

```bash
cargo clean -p deno_web
```

```text
Removed 138 files, 224.3MiB total
```

Local-fork focused censuses:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 827 filtered out; finished in 2.04s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 827 filtered out; finished in 1.95s
```

Published fork tag:

```bash
cd /Users/jack/src/github.com/nimbus/deno
git add ext/web/06_streams.js
git commit -m "node(webstreams): attach invalid-state codes to BYOB errors"
git tag v2.8.3-nimbus.6
git push origin HEAD
git push origin v2.8.3-nimbus.6
```

```text
[nimbus/v2.8.3 7a0edfb282] node(webstreams): attach invalid-state codes to BYOB errors
 1 file changed, 19 insertions(+), 7 deletions(-)
7ec6b93296..7a0edfb282  HEAD -> nimbus/v2.8.3
* [new tag]               v2.8.3-nimbus.6 -> v2.8.3-nimbus.6
```

Nimbus repin:

```bash
perl -pi -e 's/v2\.8\.3-nimbus\.5/v2.8.3-nimbus.6/g' Cargo.toml
cargo update -p deno_web
cargo clean -p deno_web
```

```text
Locking 40 packages to latest compatible versions
Removing deno_web v0.282.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.5#7ec6b932)
Adding deno_web v0.282.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.6#7a0edfb2)
Removed 524 files, 116.5MiB total
```

The full lock update moved the Deno-family git source from
`v2.8.3-nimbus.5#7ec6b932...` to `v2.8.3-nimbus.6#7a0edfb282...`.

## Proof

Focused post-repin censuses on the published tag:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 827 filtered out; finished in 2.06s
```

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 827 filtered out; finished in 1.99s
```

Promoted non-ignored guards:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib \
  cycle38_webstreams_byob -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle38-webstreams-byob-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle38-webstreams-byob-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 827 filtered out; finished in 3.96s
```

Formatting:

```bash
cargo fmt --all --check
```

```text
passed with no diff
```

Generated pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do \
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; \
done
```

Generated checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all --check
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
```

```text
classifications node20/node22/node24/node26 are up to date
validated node-compat watchpoint catalog: 134 entries
node default support posture: pass
node required-surface blocker inventory: pass
```

Generated posture after classification sync and evidence regeneration:

```text
node22 required gaps: 61
node22 required pass rate: 97.43
node24 required gaps: 71
node24 required pass rate: 97.06
unique required fixtures remaining: 73
```

## Regression Notes

Because the fork edit touches shared `deno_web` WebStreams code, I attempted the
existing node22 promoted Streams/WebStreams broad batch:

```bash
gtimeout -s KILL 360 cargo test -p nimbus-runtime --lib \
  node22_supported_lane_executes_streams_web_platform_promoted_batch_fixture \
  -- --nocapture
```

```text
node_compat node22-supported-lane-executes-streams-web-platform-promoted-batch node22 summary: selected=67, passed=63, skipped=0, failed=4
test/parallel/test-blob-file-backed.js ... expected code 'ERR_INVALID_STATE', actual code 25
test/parallel/test-webstream-encoding-inspect.js ... inspect formatting mismatch
test/parallel/test-webstream-readable-from.js ... missing ERR_ARG_NOT_ITERABLE
test/parallel/test-webstreams-abort-controller.js ... JavaScript execution has been terminated
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 828 filtered out; finished in 187.66s
```

Those four failures are outside the BYOB invalid-state paths changed in the fork
and should be treated as pre-existing broad-batch debt unless a later run proves
otherwise. They are not used as cycle-38 promotion evidence; the promotion
evidence is the focused post-repin census and the non-ignored cycle-38 guard
above.

## Guardrails

- No V8 or rusty_v8 source changes.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Temporary Cargo path override was removed before promotion/commit.
- Temporary scratch `nds_probe` include/file was removed before promotion.
- The fixture was removed from the node22/node24 required blocker inventory only
  by `classifications.py sync` after non-ignored green guards landed.
- PR #10 remains draft; the gate is still red and honest.
