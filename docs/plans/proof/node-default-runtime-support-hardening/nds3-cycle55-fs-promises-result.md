# NDS3 cycle 55 - fs.promises file handle parity

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-fs-promises.js` was dynamically promoted for node22 and
node24.

Gate movement:

- node22: 47 -> 46 gaps, 98.06% pass rate
- node24: 54 -> 53 gaps, 97.79% pass rate

Deno fork tag: `v2.8.3-nimbus.9` (`d0f50db81e`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

The fixture exposed several composed gaps:

- `assert.rejects()` stack labels diverged by official Node lane:
  node22 expects `Function.rejects`, while node24 expects `ok.rejects`.
- `FileHandle.read(0, 0, 0, 0)` did not reject with
  `ERR_INVALID_ARG_TYPE`.
- `FileHandle` internal receiver assertions surfaced public
  `ERR_ASSERTION` instead of internal `ERR_INTERNAL_ASSERTION`.
- The runtime-local bridge exposed no `Deno.chown`/`chownSync` for Node's
  no-op `chown(path, process.getuid(), process.getgid())` path.
- The existing `fs.promises.lchmod()` shim opened the symlink and chmodded the
  handle, so `lstat(link).mode` did not observe the requested symlink mode on
  macOS.

The fix keeps the sandbox boundary intact:

- Deno's assert polyfill uses `ok` as the default callable name and lets Nimbus'
  node22 harness mark `assert.rejects()` stacks with the lane-correct receiver
  label.
- Deno's `FileHandle.read()` validates primitive first arguments as buffers,
  and `internal/fs/handle.ts` uses Node's internal assertion helper for
  FileHandle receiver guards.
- Nimbus runtime-local `Deno.chown` treats only `-1` or the runtime process
  UID/GID as virtual no-op ownership, and rejects real ownership changes with
  `EPERM`.
- Nimbus runtime-local `Deno.lchmod` targets the symlink entry itself through
  `ensure_write_link_path()` and macOS `lchmod`, without following the final
  symlink component.

## Verification

Local-fork focused probes before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-promises.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-promises.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Immutable-tag focused probes after repinning to `v2.8.3-nimbus.9`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-promises.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-promises.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle55_fs_promises -- --nocapture
# node_compat node22-default-lane-executes-cycle55-fs-promises-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle55-fs-promises-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 842 filtered out
```

Focused regression guards:

```bash
cargo test -p nimbus-runtime --lib cycle54_fs_junction -- --nocapture
# node22 summary: selected=2, passed=2, skipped=0, failed=0
# node24 summary: selected=2, passed=2, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 842 filtered out

cargo test -p nimbus-runtime --lib cycle33_assert -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 843 filtered out

cargo test -p nimbus-runtime --lib cycle34_assert_deep -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 842 filtered out

cargo test -p nimbus-runtime --lib cycle32_assert_calltracker -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 842 filtered out
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Checks:

```bash
cargo fmt --all --check
# pass

git diff --check
# pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries

rg -n 'paths = \["/Users/jack/src/github.com/nimbus/deno|v2\.8\.3-nimbus\.8' .cargo/config.toml Cargo.toml Cargo.lock
# no matches

bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is not yet 0/0.
```

Generated counts:

```text
node22 46 98.06
node24 53 97.79
```
