# /goal: Complete Bun/JSC Embedder API And Pool Plan

Objective: Complete `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md` from
`BEP1` through `BEP9`, keeping Bun/JSC an optional in-process backend candidate
beside Deno/V8 and not selectable for tenant code until containment is proven.

Autonomy rules:

- Treat `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md` plus local git
  history as the control plane.
- Resume the first non-`done` gate before starting a later gate.
- Update the plan progress log, gate statuses, and proof documents before any
  likely context loss.
- Keep Nimbus and Bun worktree changes separate and commit each repository's
  checkpoint when the relevant verification passes.
- Do not fork Bun until the plan's fork threshold says the required embedder
  API cannot reasonably be kept upstream-first.

Verifiable success criteria:

- `BEP1` has a checked-in Bun embedder API proposal mapping every unsafe
  surface to construction, resolver, permission, lifecycle, audit, or
  unsupported state.
- `BEP2` records the upstream-first versus fork threshold, including release
  tag format, no-fork conditions, and the exact trigger for creating
  `nimbus/bun`.
- `BEP3` keeps Nimbus typed runtime/backend/pool diagnostics ready but fail
  closed for Bun/JSC by default.
- `BEP4` defines or scaffolds a dedicated Bun/JSC pool owner with lifecycle,
  state/ack-driven cancellation, teardown, metrics, and no Deno/V8 internals
  in the public envelope; product cancellation does not rely on elapsed-time
  sleeps.
- `BEP5` proves resolver/package policy denial or hookability in the Bun proof
  target.
- `BEP6` proves native permission denial or hookability for filesystem,
  network, env/process, subprocess, FFI, plugin, worker, timer, fetch/WebSocket,
  and dynamic-code surfaces.
- `BEP7` proves memory, cancellation, teardown, and reuse policy on macOS and
  Linux, including cancellation before/after guest entry, during sync loops,
  promise/microtask work, HostBridge in-flight work, normal completion, and
  teardown, or keeps untrusted Bun on fresh/discard with an outer quota.
- `BEP8` integrates Bun/JSC as optional only after the proven lockdown profile
  and pool policy exist; V8/Deno remains default.
- `BEP9` closes with repeatable local and Linux verification, updated docs,
  residual risks, fork status, and product go/no-go.
- Required checks pass or the plan records a precise, source-level blocker:
  `cargo fmt --all --check`, the focused Nimbus runtime/server tests named in
  the plan, `bash scripts/verify-bun-jsc-in-process-lockdown.sh`,
  Bun `cargo fmt --all --check`, Bun native `check-bun-embed-probe`, and
  `git diff --check` in both worktrees.
