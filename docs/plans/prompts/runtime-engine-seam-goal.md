# Codex Goal Prompt - Runtime Engine Seam Autonomous Completion

Historical prompt for the completed runtime engine seam plan. New work should
start from `docs/plans/execution-isolation-and-runtime-backends-plan.md`.

---

## /goal Prompt

```text
/goal Complete the Nimbus runtime engine seam plan in /Users/jack/src/github.com/nimbus/nimbus.

Source of truth:
- Current git worktree and local git history
- docs/plans/archive/runtime-engine-seam-plan.md
- docs/architecture/runtime/engine-seam.md
- ARCHITECTURE.md
- docs/plans/README.md

Do not rely on chat history. Resume from the plan's Phase Status Ledger,
Execution Log, and the current git worktree. If compaction happens, continue
from the plan and git state rather than restarting.

Startup:
1. Read AGENTS.md, README.md, ARCHITECTURE.md, docs/README.md,
   docs/plans/README.md, docs/architecture/runtime/engine-seam.md, and
   docs/plans/archive/runtime-engine-seam-plan.md.
2. Run git status --short.
3. Inspect any dirty files before editing. Treat existing changes as user or
   prior-agent work. Do not revert unrelated changes.
4. If any RS phase is in_progress, resume it. Otherwise start the first todo
   phase whose dependencies are satisfied.

Autonomous execution loop:
1. Work exactly one RS phase at a time.
2. Mark the active phase in_progress before code-changing work.
3. Keep edits scoped to the active phase.
4. Implement the phase end to end when feasible.
5. Run the phase's minimum verification from the Verification Matrix.
6. Record exact verification commands and outcomes in the Execution Log.
7. Mark the phase done only when its acceptance criteria and verification pass.
8. Immediately continue to the next unblocked todo phase unless blocked,
   unsafe to continue, or the whole plan is complete.

Success criteria:
- RS1 through RS6 are done.
- Deno/V8 remains the default runtime path and existing behavior is verified
  after each code-changing phase.
- WorkerLoop remains the scheduler-facing seam.
- RuntimeBackend and worker queue invocation envelopes are engine-neutral and
  do not carry Deno/V8 VM state as the generic runtime contract.
- The JavaScript Nimbus context contract is separable from Deno-op transport.
- HostBridge, HostCallOperation, HostCallPayload, RuntimeExtensionCall,
  cancellation, permit pause/resume, metrics, and tracing semantics survive the
  Deno/V8 transport split.
- RuntimeBackendKind, compatibility target, execution model, pooling model, and
  bundle content kind are explicit validated policy axes.
- Generated artifacts and server registry routing name engine/content choices
  explicitly while preserving current default and Node20/Node22/Node24 lanes.
- The new-engine proof harness shape, including Bun/JSC build/link hazards, is
  documented with concrete commands and pass/fail promotion criteria.
- The Execution Log contains a durable checkpoint for every completed, blocked,
  or split phase.

Verification expectations:
- Use the per-phase Verification Matrix in
  docs/plans/archive/runtime-engine-seam-plan.md.
- Prefer focused cargo/npm tests first.
- Run cargo fmt --all --check after Rust changes.
- Run npm run test --workspace @nimbus/codegen after codegen changes.
- Run make clippy before final handoff if shared Rust runtime/server seams were
  changed.
- If a documented verification command is unavailable or fails for an unrelated
  pre-existing reason, record the exact failure and the residual risk in the
  Execution Log.

Stop only when:
- the entire plan is complete,
- a real blocker requires user/design input,
- verification fails and cannot be resolved safely in the active phase,
- or continuing would require splitting the plan first.

Before stopping:
- Update docs/plans/archive/runtime-engine-seam-plan.md with the current phase
  status, verification evidence, and exact next action.
- Leave at most one phase in_progress.
- Summarize completed phases, verification results, and next action.

Do not add Bun, JSC, wasmtime, or new engine dependencies unless the active
phase explicitly calls for a proof harness and the plan has been updated with
the safe scope first. Do not stage or commit unless the user asks.
```
