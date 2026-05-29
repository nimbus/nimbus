# Node Compat Cron Greening Plan (NCG)

Status: `superseded by docs/plans/node-default-runtime-support-hardening-plan.md`
Owner: `node-compat`
Verifier: `scripts/verify-node-compat-cron-greening.sh` (scaffolded in NCG0)
Baseline proof: `docs/plans/proof/node-compat-cron-greening/ncg0-baseline.md`

Supersession note: this plan is intentionally narrow. It greens the historical
Node Compatibility cron foundation failures, but it does not define or prove
that Node24 is a well-supported product default. The broader active control
plane is `docs/plans/node-default-runtime-support-hardening-plan.md`, which
subsumes the NCG foundation-slice work in NDS2.

## Why this plan exists

The scheduled `Node Compatibility` workflow
(`.github/workflows/node-compat-nightly.yml`, cron `0 7 * * *`) has
been **red every single day for at least 10 consecutive runs** —
since `9d20bc9f` on 2026-05-14 through `23eb430e` on 2026-05-23. The
failures are not infrastructure (no transient `Bad credentials`, no
runner pin drift, no sccache key churn); they are real semantic
deltas between the upstream Node fixture trees and the current
Nimbus Node-compat surface.

Latest failing cron run (`26328664800`, HEAD `23eb430e`):

| Slice × Lane | Result |
|---|---|
| `core-semantics:assert-and-buffer-foundation` × node20 | passed: 8, failed: 0 |
| `core-semantics:assert-and-buffer-foundation` × node22 | passed: 10, failed: 0 |
| `core-semantics:assert-and-buffer-foundation` × node24 | passed: 10, failed: 0 |
| `process-and-timing:process-foundation` × node20 | passed: 9, failed: 0 |
| `process-and-timing:process-foundation` × node22 | passed: 10, failed: 0 |
| **`process-and-timing:process-foundation` × node24** | **passed: 9, failed: 1** |
| `streams-and-local-io:os-tty-readline-foundation` × node20 | passed: 10, failed: 0 |
| `streams-and-local-io:os-tty-readline-foundation` × node22 | passed: 10, failed: 0 |
| `streams-and-local-io:os-tty-readline-foundation` × node24 | passed: 10, failed: 0 |
| `networking:dns-net-foundation` × node20 | passed: 10, failed: 0 |
| `networking:dns-net-foundation` × node22 | passed: 10, failed: 0 |
| `networking:dns-net-foundation` × node24 | passed: 9, failed: 0 |
| **`loader-context:module-and-async-foundation` × node20** | **passed: 6, failed: 4** |
| **`loader-context:module-and-async-foundation` × node22** | **passed: 6, failed: 4** |
| **`loader-context:module-and-async-foundation` × node24** | **passed: 6, failed: 4** |

Net: **13 fixture failures** across the 15 slice×lane cells, all
concentrated in two foundation slices:

1. `process-and-timing:process-foundation` × **node24 only** —
   `test/parallel/test-process-features.js` (1 fixture).
2. `loader-context:module-and-async-foundation` × **all three lanes** —
   the same 4 of 10 fixtures fail on each lane (NCG2 will name them
   precisely from the slice JSON).

## The actual gap mechanisms

### Process foundation, node24

`crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
contains `seedNodeProcessFeatures()` which explicitly deletes
`openssl_is_boringssl` from `process.features`. That matches
upstream node20 / node22 (neither lane's `test-process-features.js`
lists `openssl_is_boringssl` in `expectedKeys`). But the node24
fixture **does** list it:

```js
// node_compat_fixtures/node24/test/parallel/test-process-features.js
const expectedKeys = new Map([
  ['inspector', ['boolean']],
  ['debug', ['boolean']],
  ['uv', ['boolean']],
  ['ipv6', ['boolean']],
  ['openssl_is_boringssl', ['boolean']],   // ← node24 only
  ['tls_alpn', ['boolean']],
  ...
]);
assert.deepStrictEqual(actualKeys, new Set(expectedKeys.keys()));
```

The `deepStrictEqual` set comparison is strict — adding
`openssl_is_boringssl=false` unconditionally would un-break node24
but break node22 (its expected set excludes the key). The single
Node22-shaped contract cannot satisfy both simultaneously; the
bootstrap must become **lane-aware**.

### Loader-context foundation, all lanes

The 10 module-and-async-foundation fixtures are:

```
test/parallel/test-module-builtin.js
test/parallel/test-module-cache.js
test/parallel/test-module-children.js
test/parallel/test-module-create-require.js
test/parallel/test-module-create-require-multibyte.js
test/parallel/test-module-isBuiltin.js
test/parallel/test-module-loading-deprecated.js
test/parallel/test-module-nodemodulepaths.js
test/parallel/test-module-relative-lookup.js
test/parallel/test-module-version.js
```

6 pass and 4 fail identically across node20/22/24 — the consistent
across-lane pattern means the failures are **Nimbus runtime
behavior**, not Node-version drift. The cron summary line does not
break out which 4; the per-fixture verdict is in the
`target/node-compat/reports/loader-context__module-and-async-foundation__<lane>.json`
artifact emitted by `scripts/runtime/node/report.sh`, and the
workflow's `target/node-compat/` artifact upload preserves it.
NCG2 names the 4 from a local slice run and classifies each.

## Scope

In scope:

- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
  (lane-aware `seedNodeProcessFeatures()`).
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/`
  (no manifest changes — the failing fixtures stay seeded; we close
  them, we do not quarantine them).
- `crates/nimbus-runtime/src/runtime/tests/node/cases/` (add or
  amend watchpoint tests as fixtures are resolved).
- `crates/nimbus-runtime/src/runtime/` (the runtime ops and builtins
  the 4 loader-context fixtures exercise — to be specified by NCG2).
- `scripts/runtime/node/report.sh` (set `NIMBUS_NODE_LANE` per-lane
  invocation so the bootstrap can read it).
- `docs/architecture/runtime/node-lts-compat/failures/loader-context.md`
  and `process-and-timing.md` (failure inventory closeout).
- `docs/architecture/runtime/node-lts-compat/node-lts-compat-summary.md`
  (promote a new "Foundation-slice gating contract" section).
- `scripts/verify-node-compat-cron-greening.sh` (this plan's verifier).
- Routing entries in `docs/plans/README.md` and `CLAUDE.md`.
- `nimbus/deno` fork commits IF — and only if — NCG2 classifies any
  failing fixture as fork-dependent. Fork bumps follow the existing
  documented workflow (CLAUDE.md, Node-compat plan family): unpin
  Nimbus → prove against canonical worktree at
  `~/src/github.com/nimbus/deno` → publish fork commit/tag/push →
  repin Nimbus → rerun verification.

Out of scope:

- Promoting new fixtures into the foundation slice manifests (this
  plan greens the *currently manifested* foundation; expanding the
  denominator is the next-generation Node-compat plan).
- Carried slice (non-foundation) divergences — already classified in
  the failure inventory and not gating the cron's foundation summary
  lines.
- Tooling-canary expansion (NLC10 closed those; this plan does not
  touch `tests/runtime/node/tooling-canaries/`).
- Node26+ harness scale work (NLC10 deferred).
- Oracle artifact cataloging changes.
- Workflow exit-code-on-fail behavior — the cron stays hard-fail.
  The point of this plan is to make it green, not to silence it.
- CI plan acceleration (CC, CM, CA, CW — already closed).

## Authorization model

This plan is **not pre-authorized for autonomous push-to-main**.
The CW/CA/CC/CM autonomy applies to the four named CI plans only.
NCG touches the runtime bootstrap and the `nimbus/deno` fork (if
NCG4 fires), both higher-risk surfaces than CI workflow edits. Each
NCG commit lands behind a regular conversation gate unless the user
explicitly extends the authorization. Fork commits in `nimbus/deno`
require explicit user authorization per-tag.

## Ledger

| NCG | Description | Status |
|-----|-------------|--------|
| NCG0 | Scaffold this plan, scaffold `scripts/verify-node-compat-cron-greening.sh` with the 10 completion-gate conditions (each starts FAIL except the ones already satisfied: plan exists, routing entries present), add routing entries to `docs/plans/README.md` + `CLAUDE.md`. Capture baseline proof at `docs/plans/proof/node-compat-cron-greening/ncg0-baseline.md` with: the 15-cell slice×lane matrix above, the last 10 Node Compatibility cron runs (all failures), and the bootstrap snippet that drops `openssl_is_boringssl`. | pending |
| NCG1 | Lane-aware `seedNodeProcessFeatures()` in `node22_runtime_bootstrap.js`. Replace the unconditional `delete features.openssl_is_boringssl` with a lane gate: read the active lane from `Deno.env.get("NIMBUS_NODE_LANE")` (fall back to parsing the major of `nodeProcess.version`), and when lane === `"node24"` emit `features.openssl_is_boringssl = features.openssl_is_boringssl === true`; otherwise delete. Update `scripts/runtime/node/report.sh` to export `NIMBUS_NODE_LANE=<lane>` for each lane invocation. Add a `node24_process_features_watchpoint` test in `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_core.rs` mirroring the existing `node20_*` and `node22_*` ones. Update the existing watchpoint comment block to reflect lane-aware behavior. | pending |
| NCG2 | Triage the 4 failing `loader-context:module-and-async-foundation` fixtures. Locally run `bash scripts/runtime/node/report.sh --family loader-context --slice module-and-async-foundation --capture-live --output-root target/node-compat-ncg2` for each lane (node20/22/24), name the 4 failing fixtures from the per-lane JSON reports, and classify each failing fixture into exactly one of: (a) **bootstrap-shim** — closable by adding/amending a `node22_runtime_bootstrap.js` shim; (b) **runtime-op** — closable by amending a `crates/nimbus-runtime/src/runtime/ops/` op; (c) **fork-bump** — requires a `nimbus/deno` change before Nimbus can shim it; (d) **explicit-divergence** — requires a watchpoint exit in `cases/watchpoints_module.rs` because the upstream fixture is materially incompatible with Nimbus' policy (must include a written justification, NOT a convenience escape). Capture proof at `docs/plans/proof/node-compat-cron-greening/ncg2-loader-context-triage.md` listing each fixture, classification, and the specific in-Nimbus code path (with line numbers) that's responsible. | pending |
| NCG3 | Close the (a) and (b) fixtures from NCG2. For each, land the fix in the same commit as the watchpoint update in `docs/architecture/runtime/node-lts-compat/failures/loader-context.md` that flips the fixture's status from `in_progress`/`carried` to `resolved`. No fixture in this lane lands without an accompanying watchpoint case in `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_module.rs` (or whichever file the existing module/cjs watchpoints live in) that pins the fix against regression. | pending |
| NCG4 | Close the (c) fixtures from NCG2 via `nimbus/deno` fork bumps. Requires explicit user authorization to publish fork commits. For each: temporarily unpin Nimbus from the published `nimbus/deno` tag → point the Deno-family Cargo deps at `~/src/github.com/nimbus/deno` worktree → make the fix in the canonical worktree → verify the fix against Nimbus with `make test-rust-runtime` and the affected slice replay → commit+tag+push in `~/src/github.com/nimbus/deno` → repin Nimbus' `Cargo.toml` + `Cargo.lock` to the new published tag → rerun verification. Capture proof at `docs/plans/proof/node-compat-cron-greening/ncg4-deno-fork-bumps.md` with each published fork SHA, the Nimbus repin commit SHA, and the slice replay output. Skip entire NCG4 if NCG2 classifies zero fixtures as (c). | pending |
| NCG5 | Close the (d) fixtures from NCG2 via watchpoint exits with written justification. Each (d) fixture lands a `#[ignore = "..."]` watchpoint in `cases/watchpoints_module.rs` mirroring the existing `node20_process_features_watchpoint` pattern. The `#[ignore]` message must cite the upstream commit / Node version delta that motivates the watchpoint, NOT the convenience of skipping. Update `failures/loader-context.md` to reflect the watchpoint status. (Soft expectation: ≤ 1 fixture should land here. If NCG2 classifies more than 1 as (d), pause and re-triage — the foundation slice should not have multiple intentional divergences.) | pending |
| NCG6 | Confirm the next-after-merge `Node Compatibility` cron run on `main` is `success`. The cron fires on schedule `0 7 * * *` so this step is gated on a wall-clock-day; for tighter latency a `workflow_dispatch` trigger satisfies the same condition. Verifier condition 8 (latest cron run on `main` is `success`) flips from FAIL to PASS at this point. Capture the green run id at `docs/plans/proof/node-compat-cron-greening/ncg6-green-cron-run.md`. | pending |
| NCG7 | Closeout. Flip every ledger row to `done`. Append Execution Log with real SHAs. Move plan to `docs/plans/archive/node-compat-cron-greening-plan.md`. Promote `docs/architecture/runtime/node-lts-compat/node-lts-compat-summary.md` with a "Foundation-slice gating contract" section that names the foundation slices, the lane-aware bootstrap contract from NCG1, and the watchpoint policy (any divergence requires a watchpoint test + failure-inventory entry — no silent quarantine). Update routing in `docs/plans/README.md` + `CLAUDE.md` to point at the archived path. Verifier's `plan_file()` helper accepts both active and archived paths from day one. | pending |

## Completion Gate

`bash scripts/verify-node-compat-cron-greening.sh` exits 0 with
summary line `10 passed, 0 failed`. The 10 conditions:

1. **Plan file exists** — either `docs/plans/node-compat-cron-greening-plan.md`
   (active) or `docs/plans/archive/node-compat-cron-greening-plan.md`
   (archived). Accepts either so closeout doesn't require a verifier
   change.
2. **Routing entry exists in `CLAUDE.md`** — substring match on
   `node-compat-cron-greening`.
3. **NCG0 baseline proof exists** at
   `docs/plans/proof/node-compat-cron-greening/ncg0-baseline.md`.
4. **Lane-aware `process.features` bootstrap landed** — regex match
   in `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
   for a conditional emit of `openssl_is_boringssl` keyed on
   `NIMBUS_NODE_LANE` or equivalent lane signal. Negative match:
   no unconditional `delete features.openssl_is_boringssl` outside
   the non-`node24` branch.
5. **`scripts/runtime/node/report.sh` propagates the lane** —
   regex match for `NIMBUS_NODE_LANE` export keyed on the lane axis.
6. **NCG2 triage proof exists and names the 4 failing fixtures
   with classifications** — regex match for `(a)|(b)|(c)|(d)` in
   `docs/plans/proof/node-compat-cron-greening/ncg2-loader-context-triage.md`.
7. **Failure inventory does not list any foundation-slice fixture
   as `in_progress` or `carried` post-closeout** — substring scan of
   `docs/architecture/runtime/node-lts-compat/failures/loader-context.md`
   for the 4 fixture filenames named in NCG2; each must appear
   under a `resolved` or `watchpoint` status, never under
   `in_progress`/`carried`.
8. **Latest `Node Compatibility` cron run on `main` is `success`** —
   `gh run list --workflow="Node Compatibility" --branch main --limit 1`
   conclusion = `success`.
9. **Every ledger row in this plan marked `done`** — count `pending`
   lines under the Ledger table; must be 0.
10. **Latest `ci.yml` run on `main` is `success`** — no regression
    to the four closed CI plans' gating. (Mirrors the condition the
    CC/CM/CA/CW verifiers all carry.)

Conditions 1-7 are deterministic against the working tree.
Conditions 8 and 10 query GitHub. Condition 9 parses this file.

## Execution Log

| Date | NCG | Status | Files touched | Notes | Verification |
|------|-----|--------|---------------|-------|--------------|
| _pending NCG0 commit_ | NCG0 | _pending_ | | | |

## Risks and explicit non-goals

- **Risk: NCG2 classifies more than 1 fixture as (d)** — multiple
  intentional foundation-slice divergences erode the "foundation =
  baseline contract" framing. Mitigation: NCG5 has a soft `≤ 1`
  expectation and a triage-pause clause if exceeded.
- **Risk: NCG4 forks are pinned to old Deno upstreams** — the
  `nimbus/deno` v2.7.14-locker.N line has been steadily moving; a
  rebase against upstream may surface new tests beyond the foundation
  slice. Mitigation: NCG4 commits one fork bump at a time, with
  Nimbus-side verification gated between each.
- **Risk: cron-vs-local divergence** — fixtures might pass locally
  but fail in the GH Actions environment (rare but documented in
  the existing failure inventory for some carried fixtures).
  Mitigation: NCG2 captures the slice JSON from a `workflow_dispatch`
  cron run on a feature branch BEFORE landing NCG3, so the local
  triage matches the cron's observation.
- **Non-goal: speed up the cron's wall time**. The cron currently
  runs ~3 hours (`08:57 → 11:58`). That is independent of slice
  correctness; cron-wall is owned by future infra plans.
- **Non-goal: change the cron's exit-code-on-fail behavior**. The
  hard-fail gate is what makes this plan motivating in the first
  place; converting to evidence-only would silence the gap rather
  than close it.

## References

- `docs/architecture/runtime/node-lts-compat/node-lts-compat-summary.md` — current contract baseline.
- `docs/architecture/runtime/node-lts-compat/failures/loader-context.md` — pre-NCG failure inventory.
- `docs/architecture/runtime/node-lts-compat/failures/process-and-timing.md` — pre-NCG failure inventory.
- `docs/plans/archive/node-lts-compatibility-plan.md` — completed NLC0..NLC10 baseline.
- `docs/plans/archive/node-compat-evidence-hardening-plan.md` — completed evidence-hardening baseline.
- `docs/plans/archive/node-compat-test-infrastructure-plan.md` — completed infra baseline.
- `docs/plans/archive/node-compat-supported-lanes-plan.md` — completed supported-lanes baseline.
- `.github/workflows/node-compat-nightly.yml` — the cron this plan greens.
- `scripts/runtime/node/report.sh` — slice runner; NCG1 extends it with `NIMBUS_NODE_LANE`.
- Failing run referenced throughout: `gh run view 26328664800` (`23eb430e`, 2026-05-23).
