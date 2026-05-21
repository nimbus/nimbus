# Codex Goal Prompt - Execution Isolation And Runtime Backends

Use this prompt to run the execution isolation and runtime backends plan
autonomously to completion. Copy the full `/goal` block below into a fresh
Codex session.

---

## /goal Prompt

```text
/goal Complete the Nimbus execution isolation and runtime backends plan in /Users/jack/src/github.com/nimbus/nimbus.

Source of truth:
- Current git worktree and local git history
- docs/plans/execution-isolation-and-runtime-backends-plan.md
- docs/plans/archive/runtime-engine-seam-plan.md
- docs/architecture/runtime/engine-seam.md
- docs/architecture/runtime/new-engine-proof-harness.md
- docs/plans/security/sandbox-isolation-audit.md
- ARCHITECTURE.md
- docs/plans/README.md

Do not rely on chat history. Resume from the active plan's Phase Status
Ledger, Execution Log, and the current git worktree. If compaction happens,
continue from the plan and git state rather than restarting.

Startup:
1. Read AGENTS.md, README.md, ARCHITECTURE.md, docs/README.md,
   docs/plans/README.md,
   docs/plans/execution-isolation-and-runtime-backends-plan.md,
   docs/architecture/runtime/engine-seam.md,
   docs/architecture/runtime/new-engine-proof-harness.md, and
   docs/plans/security/sandbox-isolation-audit.md.
2. Run git status --short.
3. Inspect any dirty files before editing. Treat existing changes as user or
   prior-agent work. Do not revert unrelated changes.
4. If any EIB phase is in_progress, resume it. Otherwise start the first todo
   phase whose dependencies are satisfied.

Autonomous execution loop:
1. Work exactly one EIB phase at a time.
2. Mark the active phase in_progress before code-changing work.
3. Keep edits scoped to the active phase and the owning docs/code surfaces.
4. Implement the phase end to end when feasible.
5. Run the phase's minimum verification from the Verification Matrix.
6. Record exact verification commands and outcomes in the Execution Log.
7. Mark the phase done only when its acceptance criteria and verification pass.
8. Commit completed phase checkpoints on main, staging only files owned by the
   active phase and leaving unrelated dirty worktree changes untouched.
9. Immediately continue to the next unblocked todo phase unless blocked,
   unsafe to continue, or the whole plan is complete.

Success criteria:
- EIB1 through EIB7 are done.
- The runtime, sandbox, WASM/WASI, admission, and security-audit plans have a
  single ownership map that says which active or deferred plan owns each
  execution-boundary concern.
- Runtime backend promotion has explicit trust tiers, not only engine names.
- Bun/JSC has a recorded go/no-go decision for forking, permissions, memory,
  package loading, VM reuse, and production selection.
- Sandbox isolation findings are assigned to implementation plans or accepted
  residual-risk decisions with operator controls.
- Wasmtime and WASI agent plans depend on the same capability and trust-tier
  vocabulary as in-process runtimes and sandboxed services.
- Admission/resource gates protect concrete resources and name overload
  behavior before implementation starts.
- Active plans in docs/plans/README.md remain small and non-overlapping.
- Every completed phase has verification recorded in the Execution Log.

Verification expectations:
- Use the per-phase Verification Matrix in
  docs/plans/execution-isolation-and-runtime-backends-plan.md.
- Prefer documentation/source-review checklists for planning phases.
- Run git diff --check for touched docs.
- Run focused cargo/npm tests only when code or generated artifacts change.
- Run cargo fmt --all --check after Rust changes.
- Run npm run test --workspace @nimbus/codegen after codegen changes.
- If a documented verification command is unavailable or fails for an
  unrelated pre-existing reason, record the exact failure and residual risk in
  the Execution Log.

Stop only when:
- the entire plan is complete,
- a real blocker requires user/design input,
- verification fails and cannot be resolved safely in the active phase,
- continuing would require splitting the plan first,
- or an external repository such as /Users/jack/src/github.com/oven-sh/bun
  needs permissions that have not been granted.

Before stopping:
- Update docs/plans/execution-isolation-and-runtime-backends-plan.md with the
  current phase status, verification evidence, and exact next action.
- Leave at most one phase in_progress.
- Summarize completed phases, verification results, commits, and next action.

Do not add Bun, JSC, wasmtime, WASI, sandbox, or admission dependencies unless
the active EIB phase explicitly calls for that implementation slice and the
plan has been updated with the safe scope first. Do not create a Bun fork or
make Bun production-selectable unless the plan records the required evidence
and a go/no-go decision.
```
