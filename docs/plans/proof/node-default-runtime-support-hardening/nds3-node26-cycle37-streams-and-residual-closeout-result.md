# NDS3 Node26 Cycle 37 Streams And Residual Closeout Result

Date: 2026-06-16
Branch: `codex/node-default-runtime-support-hardening`
PR: `https://github.com/nimbus/nimbus/pull/10` (draft)
Nimbus worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Deno worktree: `/Users/jack/src/github.com/nimbus/deno`

## Scope

This checkpoint closes the Node26 required-surface residual burn-down that was
left after cycle 36. It does not undraft or merge PR #10. It also does not
claim the aggregate NDS closeout verifier is green: the required support posture
is green for Node22, Node24, and Node26, but the aggregate verifier still has
private proof/plan closeout predicates to satisfy.

## Fork Work

Deno fork commit:

- commit: `6c97d4aed7b901a38db9b01ccbb3b8a3935c840a`
- tag: `v2.8.3-nimbus.70`
- tag object: `0fec6d7c425aa5edd34e3f77bc00594ca5920c86`
- branch: `nimbus/v2.8.3`

Remote verification:

```console
git -C /Users/jack/src/github.com/nimbus/deno ls-remote origin refs/heads/nimbus/v2.8.3 refs/tags/v2.8.3-nimbus.70
```

Observed:

```console
6c97d4aed7b901a38db9b01ccbb3b8a3935c840a	refs/heads/nimbus/v2.8.3
0fec6d7c425aa5edd34e3f77bc00594ca5920c86	refs/tags/v2.8.3-nimbus.70
```

Fork changes:

- `ext/node/polyfills/internal/streams/readable.js`: gates the Node26 readable
  buffer fast path on `process.versions.node`, preserving Node26 behavior while
  restoring Node22/Node24 stream chunking semantics.
- `ext/web/06_streams.js`: removes the Nimbus cross-realm WebStreams
  `MessagePort.unref()` calls so cloned stream transfer keeps the event loop
  alive until the stream settles.

Nimbus was repinned from `v2.8.3-nimbus.68` to `v2.8.3-nimbus.70` in
`Cargo.toml` and `Cargo.lock`.

## Local Deno Proof

These runs used the canonical local Deno worktree through Cargo path override
while proving the fork edits.

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-local-version-gate-node22-promoted1 \
  cargo --config 'paths=["/Users/jack/src/github.com/nimbus/deno"]' test -p nimbus-runtime --lib node22_supported_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-local-version-gate-node22-promoted1/batch/node22__node22_supported_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `67`
- passed: `67`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-local-version-gate-node24-promoted1 \
  cargo --config 'paths=["/Users/jack/src/github.com/nimbus/deno"]' test -p nimbus-runtime --lib node24_default_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-local-version-gate-node24-promoted1/batch/node24__node24_default_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `87`
- passed: `87`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-local-version-gate-node26-promoted1 \
  cargo --config 'paths=["/Users/jack/src/github.com/nimbus/deno"]' test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-local-version-gate-node26-promoted1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `120`
- passed: `120`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-unpromoted-surface-residual-local-streams1 \
  cargo --config 'paths=["/Users/jack/src/github.com/nimbus/deno"]' test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-unpromoted-surface-residual-local-streams1/batch/node26__node26_current_lane_unpromoted_surface_required_gap_watchpoint__summary.json`
- selected: `2`
- passed: `2`
- skipped: `0`
- failed: `0`

## Immutable Tag Proof

After publishing `v2.8.3-nimbus.70`, Nimbus was repinned to the immutable tag
and rerun without a local Cargo path override.

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-tag70-node26-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-tag70-node26-promoted1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `120`
- passed: `120`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-unpromoted-surface-residual-tag70-streams1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-unpromoted-surface-residual-tag70-streams1/batch/node26__node26_current_lane_unpromoted_surface_required_gap_watchpoint__summary.json`
- selected: `2`
- passed: `2`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-tag70-node22-promoted1 \
  cargo test -p nimbus-runtime --lib node22_supported_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-tag70-node22-promoted1/batch/node22__node22_supported_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `67`
- passed: `67`
- skipped: `0`
- failed: `0`

```console
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle37-streams-tag70-node24-promoted1 \
  cargo test -p nimbus-runtime --lib node24_default_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Observed summary:

- artifact: `/private/tmp/nds-node26-cycle37-streams-tag70-node24-promoted1/batch/node24__node24_default_lane_executes_streams_web_platform_promoted_batch__summary.json`
- selected: `87`
- passed: `87`
- skipped: `0`
- failed: `0`

## Generated Posture

Regeneration command:

```console
python3 -B scripts/runtime/node/classifications.py sync --lane node26
python3 -B scripts/runtime/node/watchpoints.py sync
python3 -B scripts/runtime/node/status.py
python3 -B scripts/runtime/node/dashboard.py
python3 -B scripts/runtime/node/trends.py
python3 -B scripts/runtime/node/publish_evidence.py
python3 -B scripts/runtime/node/default_support_posture.py
python3 -B scripts/runtime/node/required_surface_blockers.py
```

Current `docs/architecture/runtime/node-default-support-posture.json` required
surface metrics:

```json
{
  "node22": {
    "gaps": 0,
    "pass_rate_percent": 100.0,
    "passed": 2363,
    "total": 2363
  },
  "node24": {
    "gaps": 0,
    "pass_rate_percent": 100.0,
    "passed": 2400,
    "total": 2400
  },
  "node26": {
    "gaps": 0,
    "pass_rate_percent": 100.0,
    "passed": 2092,
    "total": 2092
  }
}
```

## Aggregate Verifier

Command:

```console
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Observed:

```console
Mode: public generated-evidence gate (private NDS proof plan not present)
Summary: 9 passed, 1 skipped, 0 failed
```

The default verifier mode now matches a clean CI checkout: it checks committed
public generated evidence and explicitly skips the private proof-row audit
because `docs/private/` is ignored. The public generated-evidence gate checks
Node22, Node24, and Node26 required-surface posture, the required-surface
blocker inventory, package registry breadth, required canary gaps, generated
public docs, release-train drift, and CI/nightly wiring.

Strict private proof audit command:

```console
NIMBUS_NDS_STRICT_PRIVATE_PROOFS=1 bash scripts/verify-node-default-runtime-support-hardening.sh
```

Observed in this checkout:

```console
Summary: 14 passed, 20 failed
```

The strict private proof audit still fails because the private plan/proof
closeout files are absent from this worktree. This is intentional: the default
CI-visible gate cannot depend on ignored private files, while the strict mode
remains available for an owner checkout that has the private proof corpus.

## Residual Risks

- PR #10 remains draft and must stay draft until the aggregate NDS gate and
  branch CI are honestly green.
- The verifier script currently checks ignored `docs/private/` proof files even
  though the GitHub workflow invokes it from a clean checkout. The next closeout
  pass must resolve that control-plane mismatch without weakening generated
  posture checks or hand-editing JSON.
- `measure_ah.sh` and local scratch/census files remain untracked and must not
  be staged with `git add -A`.
