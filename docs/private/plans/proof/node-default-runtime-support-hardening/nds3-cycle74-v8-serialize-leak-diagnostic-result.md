# NDS3 cycle 74: v8 serialize leak diagnostic reclassification

Date: 2026-06-14

Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork pin: unchanged at `v2.8.3-nimbus.24`

## Fixture

- `test/parallel/test-v8-serialize-leak.js` (node22 + node24)

The upstream fixture source is a host-process leak/GC diagnostic:

```js
// Flags: --expose-gc
const { gcUntil } = require('../common/gc');
const v8 = require('v8');
const before = process.memoryUsage.rss();

for (let i = 0; i < 1000000; i++) {
  v8.serialize('');
}

await gcUntil('RSS should go down', () => {
  const after = process.memoryUsage.rss();
  return after < before * 10;
});
```

It does not assert ordinary `v8.serialize()` functional behavior. It requires
explicit GC exposure, performs one million serializations, and polls host-process
RSS until a leak threshold converges. That is a diagnostic host system-resource
surface, not required default Application API behavior in the multi-tenant V8
isolate.

## Dynamic Diagnostics

No fresh scratch probe was needed for this cycle because current diagnostics
already existed for both required lanes.

Node24 diagnostic:

```text
target/node-compat/diagnostics/general/node24__test_parallel_test_v8_serialize_leak_js.json
outcome = runtime_error
elapsed_ms = 3219
detail = runtime JavaScript error: Cannot evaluate dynamically imported module, because JavaScript execution has been terminated
```

Node24 batch summary:

```text
target/node-compat/diagnostics/batch/node24__sweep_node24_test_parallel_test_v8_serialize_leak_js__summary.json
selected=1, passed=0, skipped=0, failed=1
```

Node22 diagnostic:

```text
target/node-compat/diagnostics/general/node22__test_parallel_test_v8_serialize_leak_js.json
outcome = runtime_error
elapsed_ms = 3386
detail = runtime JavaScript error: Cannot evaluate dynamically imported module, because JavaScript execution has been terminated
```

Node22 batch summary:

```text
target/node-compat/diagnostics/batch/node22__sweep_node22_test_parallel_test_v8_serialize_leak_js__summary.json
selected=1, passed=0, skipped=0, failed=1
```

## Disposition

Reclassified in `scripts/runtime/node/default_support_posture.py`:

- denominator: `diagnostic_only_non_isolate`
- reason code: `host_owned_system_resource_surface`
- shim classification: `diagnostic_stub`

No fixture/checker was edited. No derived posture JSON was hand-edited.

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
node22 v8_isolate_required.gaps = 22, pass_rate_percent = 99.07
node24 v8_isolate_required.gaps = 28, pass_rate_percent = 98.83
```

Before this cycle, the generated posture was:

```text
node22 v8_isolate_required.gaps = 23, pass_rate_percent = 99.03
node24 v8_isolate_required.gaps = 29, pass_rate_percent = 98.79
```

## Cleanup

- No scratch probe was added in this cycle.
- No local Deno path override was used.
- Verified `/Users/jack/src/github.com/nimbus/deno` is clean at
  `v2.8.3-nimbus.24`.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: red, as expected. Summary was `13 passed, 21 failed`; step 9 still fails
because the regenerated posture is node22=22 / node24=28, not 0/0. This checkout
also reports private-plan/proof closeout failures because those private proof
files are not present in the worktree.
