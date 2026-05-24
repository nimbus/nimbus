# Plan: Bun/JSC Linked Adapter And Execution

This plan owns the next product-moving Bun/JSC wave after
`docs/plans/bun-jsc-embedder-api-and-pool-plan.md`.

The target is a linked in-process Bun/JSC runtime backend beside the existing
Deno/V8/Node-compatible backend. This is still not "Bun in a microVM"; Bun in
an OCI image remains a sandbox workload mode. This plan is about making the
existing `RuntimeBackendKind::BunJsc` lane execute through a verified Bun
embedder adapter, a dedicated Bun/JSC pool, Nimbus HostBridge policy, and
fresh/discard lifecycle controls.

## Status

- **Status:** active; `BJA1` is next
- **Primary owner:** this plan
- **Nimbus worktree:** `/Users/jack/src/github.com/nimbus/nimbus`
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Nimbus starting baseline:** `40d1a8ea`
  (`Harden Bun runtime diagnostics contract`)
- **Current Bun proof head:** `4b5de5ee5d`
  (`Add Bun embedder pre-entry cancellation gate proof`)
- **Predecessor:** `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`
- **Default posture:** Deno/V8 remains the default runtime; Bun/JSC remains
  optional and fail-closed unless a verified adapter is linked

## Product Shape

```text
Convex/runtime manifest
  -> RuntimeBackendKind::BunJsc
  -> RuntimePolicy::application_bun_jsc()
  -> RuntimeBackendFactory
  -> BunJscRuntimeBackend
  -> BunJscPool
  -> BunJscExecutionAdapter
  -> Bun/JSC embedder API
  -> Nimbus HostBridge
```

The key seam is the `BunJscExecutionAdapter`. It must be backend-owned and
smaller than the full runtime worker surface. The worker loop should continue
to pass one `RuntimeBackendInvocation`; the Bun adapter should own only:

- VM construction using the proven lockdown profile
- self-contained program-wrapper evaluation
- JSON args/result transport
- HostBridge callback transport and grant checks
- resolver/package denial
- native permission denial/hook propagation
- cancellation, teardown, and fresh/discard lifecycle acknowledgement
- memory/outer-quota enforcement evidence
- adapter-linked diagnostics

## Non-Goals

- Do not make Bun/JSC the default JavaScript runtime.
- Do not share Deno/V8 VM internals with the Bun/JSC pool.
- Do not enable retained untrusted Bun/JSC VM reuse.
- Do not route Bun packages through `node_external_packages`.
- Do not make product correctness depend on an uncommitted local Bun checkout.
- Do not treat a wrapper-level deletion of unsafe globals as sufficient
  containment.
- Do not hide missing Bun embedder APIs behind a feature flag that claims
  product support without passing the execution gates below.

## Fork And Source Rule

The plan starts upstream-first, but the completion gate is stricter than the
research gate:

```text
complete product source
  = upstream Bun release/tag with required APIs
  OR Nimbus-owned Bun fork/tag with the required APIs
```

Local proof commits are acceptable while proving gates, but `BJA8` cannot be
marked complete until Nimbus references a reproducible source. If current Bun
does not expose the required embedder API, create the smallest maintainable
Nimbus-owned Bun fork/tag rather than relying on `~/src/github.com/oven-sh/bun`.

## Execution Gates

| Gate | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BJA0 | `done` | Rebaseline the linked-adapter architecture against current Nimbus and Bun. | `docs/plans/proof/runtime-engine/bun-jsc/gate-35-linked-adapter-rebaseline.md` records current Nimbus commit, Bun commit/tag, Bun local dirty state, required Bun-side APIs, whether upstream current Bun is enough, and the exact fork trigger. `git diff --check`, `cargo fmt --all --check`, and `make verify-bun-jsc-runtime-contract` passed. |
| BJA1 | `pending` | Introduce a real Bun/JSC execution-adapter boundary while preserving fail-closed defaults. | `crates/nimbus-runtime/src/backends/bun_jsc/` contains a concept-owned adapter trait/factory that separates `linked` from `not_linked`; default builds keep `execution_adapter_state = "not_linked"` and return the existing contract error; tests prove executors remain lazy and no Bun/JSC code is linked or started by default. |
| BJA2 | `pending` | Prove or implement the Bun-side embedder execution API. | Bun exposes a native proof target or crate/API that constructs the locked-down VM, executes a self-contained program wrapper, returns JSON, emits resolver/permission/lifecycle evidence, and fails on unsafe surfaces. The Bun proof target passes locally and on Debian 13 `minicloud` with home-backed temp/cache paths. |
| BJA3 | `pending` | Make Nimbus build with an optional linked Bun/JSC adapter without regressing default builds. | Default `cargo check --workspace` and `make verify-bun-jsc-runtime-contract` still pass without Bun. A linked build path, feature, or source override compiles the adapter from a reproducible Bun source. CI/docs name both the default no-link lane and linked proof lane. |
| BJA4 | `pending` | Execute pure Bun/JSC functions through the Bun pool. | An end-to-end runtime/server test selects the Bun/JSC lane, invokes a self-contained pure function, passes JSON args, receives JSON result, records lifecycle transitions, and proves V8/Node lanes still use V8. The old adapter-not-linked error remains only for no-link builds. |
| BJA5 | `pending` | Wire Nimbus HostBridge, tenant identity, and permission grants through Bun/JSC. | Bun/JSC functions can perform explicitly granted HostBridge operations through the same service/engine path as V8, and denied operations fail with audited, tenant-scoped errors. Tests cover allowed operation, denied operation, forged tenant/context attempt, and no raw host token exposure inside Bun/JSC guest code. |
| BJA6 | `pending` | Enforce cancellation, teardown, and memory policy for untrusted Bun/JSC. | Tests cover before-entry cancellation, after-entry sync loop cancellation, promise/microtask progress, in-flight HostBridge cancellation, normal completion, teardown, fresh/discard after each untrusted invocation, and outer memory quota behavior. Evidence passes locally and on `minicloud`. |
| BJA7 | `pending` | Promote Bun/JSC selection into product metadata only after linked execution is real. | Codegen/server artifacts can express function-level Bun selection only when the linked adapter gate is available; no-link builds continue to reject selection clearly. `/debug/runtime/metrics` and the operator UI show `linked` only for linked builds, executor started only after actual invocation, and Bun/JSC memory as `outer_quota_required`. |
| BJA8 | `pending` | Close the plan with reproducible verification, CI, docs, and source ownership. | A reusable gate such as `make verify-bun-jsc-linked-adapter` runs the linked adapter tests, default no-link contract tests, Bun source proof, and UI/API diagnostics checks. The gate passes locally and on Debian 13 `minicloud`. Docs record the final upstream-or-fork source, tag/revision, residual risks, and exact operator/product behavior. The plan is updated to `complete` only after a clean commit records the implementation and evidence. |

## Verification Baseline

Every implementation batch must keep the existing default lane green:

```sh
cargo fmt --all --check
make verify-bun-jsc-runtime-contract
bash scripts/verify-bun-jsc-in-process-lockdown.sh
npm run typecheck
git diff --check
```

Linked-adapter batches must add a stronger gate before `BJA8` can close:

```sh
make verify-bun-jsc-linked-adapter
```

That target should prove, at minimum:

- default no-link build remains fail-closed
- linked build reports Bun/JSC as `linked`
- pure Bun/JSC invocation succeeds end to end
- HostBridge grants are enforced per tenant/workload
- forged tenant identity is rejected
- cancellation and teardown are deterministic
- outer-quota memory policy is visible in diagnostics
- V8/Node lanes do not inherit Bun/JSC backend axes

When Bun source changes are part of a batch, run in
`/Users/jack/src/github.com/oven-sh/bun` or the selected Nimbus Bun fork:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

For `minicloud`, avoid `/tmp` because it has filled during prior proof runs.
Use home-backed paths such as:

```sh
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp
NIMBUS_BUN_BUILD_DIR=$HOME/.cache/nimbus-bun-proof/embed-native
NIMBUS_BUN_CACHE_DIR=$HOME/.cache/nimbus-bun-proof/cache
NIMBUS_BUN_CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/cargo-target
```

## Autonomous Goal Prompt

Complete `docs/plans/bun-jsc-linked-adapter-plan.md` from `BJA0` through
`BJA8` autonomously. Treat the plan file and git history as the control plane:
update gate status and evidence as work progresses, keep unrelated dirty files
out of commits, and do not mark the plan complete until every verifiable
success criterion passes.

Required completion evidence:

- `make verify-bun-jsc-runtime-contract` passes for the default no-link build.
- The linked adapter gate, `make verify-bun-jsc-linked-adapter`, exists and
  passes locally.
- The linked adapter gate or equivalent proof passes on Debian 13 `minicloud`.
- Bun source ownership is reproducible: upstream release/tag or Nimbus-owned
  fork/tag, never an uncommitted local checkout.
- End-to-end Bun/JSC invocation tests cover pure execution, HostBridge allow,
  HostBridge deny, forged tenant/context rejection, cancellation, teardown, and
  diagnostics.
- `cargo fmt --all --check`, `npm run typecheck`, and `git diff --check` pass.
- The implementation and plan evidence are committed in focused commits.

## Progress Log

| Date | Gate | Status | Notes | Verification | Next |
| --- | --- | --- | --- | --- | --- |
| 2026-05-23 | BJA0 | `done` | Added Gate 35, the linked-adapter rebaseline. Current Nimbus is `9b575308`; local Bun proof head is `4b5de5ee5d`, clean and 16 commits ahead of upstream `origin/main` at `f161e0311d`. Current upstream Bun is not sufficient for product source ownership because the embed probe, resolver denial, native permission denial, host-call, lifecycle, and cancellation proof surfaces exist only in the local proof delta. The fork trigger for this wave is product dependency ownership, not merely the existence of local proof commits. | `cargo fmt --all --check` passed; `make verify-bun-jsc-runtime-contract` passed outside the Codex filesystem sandbox: 11 runtime policy tests, 4 Bun/JSC pool scaffold tests, 13 Convex registry tests, 2 runtime diagnostics tests, 1 tenant-admission test, and 2 UI test files / 5 tests; `git diff --check` passed. | Start BJA1 by introducing the explicit `BunJscExecutionAdapter` boundary while preserving default no-link behavior. |
