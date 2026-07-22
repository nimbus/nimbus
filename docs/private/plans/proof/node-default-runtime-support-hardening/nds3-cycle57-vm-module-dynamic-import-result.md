# NDS3 cycle 57 - vm module dynamic import parity

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-vm-module-dynamic-import.js` was dynamically promoted for
node22 and node24.

Gate movement:

- node22: 45 -> 44 gaps, 98.14% pass rate
- node24: 52 -> 51 gaps, 97.88% pass rate

Deno fork tag: `v2.8.3-nimbus.11` (`aeca0a905a`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

The official fixture requests `--experimental-vm-modules`. Nimbus' fixture
option allowlist dropped that flag, and Deno's Node option parser did not model
it, so the `node:vm` polyfill treated callback-backed dynamic import as if the
flag were absent and rejected with
`ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING_FLAG`.

After the flag layer, Deno's custom dynamic-import callback path also differed
from Node: callback import attributes were not normalized to a null-prototype
object, the callback did not receive Node's `"evaluation"` phase argument, and a
non-Module callback result resolved instead of rejecting with
`ERR_VM_MODULE_NOT_MODULE`.

The fork fix stays in `deno_node` and does not touch V8/rusty_v8:

- parses `--experimental-vm-modules` / `--no-experimental-vm-modules` in
  `internal_binding/node_options.ts`
- passes null-prototype import attributes plus the `"evaluation"` phase to
  custom VM dynamic-import callbacks
- rejects invalid callback results with `ERR_VM_MODULE_NOT_MODULE`

Nimbus' harness allowlist now forwards the official fixture flag into
`process.execArgv` / Deno's Node option snapshot.

## Verification

Initial immutable-tag census on `v2.8.3-nimbus.10`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-dynamic-import.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
# TypeError [ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING_FLAG]
```

Local-fork focused probes before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-dynamic-import.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-dynamic-import.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Immutable-tag focused probes after repinning to `v2.8.3-nimbus.11`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-dynamic-import.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-dynamic-import.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common:test/fixtures" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle57_vm_module_dynamic_import_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle57-vm-module-dynamic-import-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle57-vm-module-dynamic-import-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out
```

Focused regression guards on the published tag:

```bash
cargo test -p nimbus-runtime --lib cycle56_vm_dynamic_import_missing_flag_batch -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out

cargo test -p nimbus-runtime --lib cycle24_vm_errors_batch -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out

cargo test -p nimbus-runtime --lib cycle19_vm_cacheddata_batch -- --nocapture
# node22 summary: selected=1, passed=1, skipped=0, failed=0
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 846 filtered out

cargo test -p nimbus-runtime --lib cycle18_vm_hasasyncgraph_batch -- --nocapture
# node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 847 filtered out
```

Harness option checks:

```bash
cargo test -p nimbus-runtime --lib fixture_requested_node_options -- --nocapture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 847 filtered out

cargo test -p nimbus-runtime --lib node_compat_fixture_node_options_exposes_preserve_symlinks_flags -- --nocapture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 847 filtered out
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py >/dev/null
```

Generated counts:

```text
node22 44 98.14
node24 51 97.88
unique remaining required fixtures: 53
```

Checks:

```bash
cargo fmt --all --check
# pass

git diff --check
# pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries

rg -n 'paths = \["/Users/jack/src/github.com/nimbus/deno|v2\.8\.3-nimbus\.10|git\+file' .cargo/config.toml Cargo.toml Cargo.lock
# no matches

bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=44 / node24=51, not 0/0.
```
