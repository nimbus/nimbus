# NDS3 cycle 56 - vm dynamic import missing-flag parity

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-vm-dynamic-import-callback-missing-flag.js` was dynamically
promoted for node22 and node24.

Gate movement:

- node22: 46 -> 45 gaps, 98.10% pass rate
- node24: 53 -> 52 gaps, 97.84% pass rate

Deno fork tag: `v2.8.3-nimbus.10` (`3dcd871146`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

Node defers the missing `--experimental-vm-modules` error until a vm dynamic
import is actually invoked. When a user supplies `importModuleDynamically` but
the flag is absent, Node rejects with
`ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING_FLAG` without calling the user callback.

Deno's `node:vm` polyfill always registered and invoked the supplied callback,
so the fixture's `common.mustNotCall()` callback produced `ERR_ASSERTION`
instead of the Node error code.

The fork fix keeps this in `deno_node` and does not touch V8/rusty_v8:

- adds `ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING_FLAG` with Node's message
- uses Deno's internal Node-option snapshot helper to test
  `--experimental-vm-modules`
- when the flag is absent, registers a deferred dynamic-import callback that
  rejects with the missing-flag error instead of invoking the user callback

## Verification

Local-fork focused probes before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-dynamic-import-callback-missing-flag.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-dynamic-import-callback-missing-flag.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Immutable-tag focused probes after repinning to `v2.8.3-nimbus.10`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-dynamic-import-callback-missing-flag.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-dynamic-import-callback-missing-flag.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle56_vm_dynamic_import_missing_flag_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle56-vm-dynamic-import-missing-flag-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle56-vm-dynamic-import-missing-flag-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 844 filtered out
```

Focused regression guards on the published tag:

```bash
cargo test -p nimbus-runtime --lib vm_hasasyncgraph_batch -- --nocapture
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 845 filtered out

cargo test -p nimbus-runtime --lib vm_cacheddata_batch -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 844 filtered out

cargo test -p nimbus-runtime --lib vm_errors_batch -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 844 filtered out
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated counts:

```text
node22 45 98.10
node24 52 97.84
```

Checks:

```bash
cargo fmt --all --check
# pass

git diff --check
# pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries

rg -n 'paths = \["/Users/jack/src/github.com/nimbus/deno|v2\.8\.3-nimbus\.9' .cargo/config.toml Cargo.toml Cargo.lock
# no matches

bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is not yet 0/0.
```
