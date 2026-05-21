# Codex Goal Prompt - Sandbox MicroVM Hardening

Use this prompt to complete the remaining sandbox microVM hardening plan
autonomously. Copy the full `/goal` block below into a fresh Codex session.

---

## /goal Prompt

```text
/goal Complete the remaining Nimbus sandbox microVM hardening plan in /Users/jack/src/github.com/nimbus/nimbus.

Source of truth:
- Current git worktree and local git history
- docs/plans/sandbox-microvm-hardening-plan.md
- docs/plans/security/sandbox-isolation-audit.md
- docs/architecture/sandbox/microvm-service-baseline.md
- docs/plans/archive/execution-isolation-and-runtime-backends-plan.md
- docs/plans/README.md
- ARCHITECTURE.md
- README.md
- /Users/jack/src/github.com/nimbus/nimbus-crun for the patched crun lane

Do not rely on chat history. Resume from the sandbox hardening plan's Phase
Status Ledger, Execution Log, and the current git worktree. If compaction
happens, continue from the plan and git state rather than restarting.

Startup:
1. Read AGENTS.md, README.md, ARCHITECTURE.md, docs/README.md,
   docs/plans/README.md, docs/plans/sandbox-microvm-hardening-plan.md,
   docs/plans/security/sandbox-isolation-audit.md, and
   docs/architecture/sandbox/microvm-service-baseline.md.
2. Run git status --short in /Users/jack/src/github.com/nimbus/nimbus.
3. Inspect any dirty files before editing. Treat existing changes as user or
   prior-agent work. Do not revert unrelated changes.
4. Run git status --short in /Users/jack/src/github.com/nimbus/nimbus-crun
   before changing patched crun files.
5. If any SMH phase is in_progress, resume it. Otherwise start the first todo
   phase whose dependencies are satisfied.

Autonomous execution loop:
1. Work one SMH phase at a time.
2. Mark the active phase in_progress before code-changing work.
3. Keep edits scoped to the active phase and owning repos.
4. Implement the phase end to end when feasible.
5. Run the phase's required verification.
6. Record exact commands and outcomes in the Execution Log.
7. Mark a phase done only when acceptance criteria and verification pass.
8. Commit completed phase checkpoints on main, staging only files owned by the
   active phase and leaving unrelated dirty worktree changes untouched.
9. Continue to the next unblocked phase unless blocked, unsafe to continue, or
   the whole plan is complete.

Remaining phase targets:
- SMH2: Fix F4 TSI bind-address carry-through across Nimbus, nimbus-crun, and
  libkrun. Nimbus must format address-bearing port-map entries, patched crun
  must parse and validate them, and Linux smoke must prove localhost-only
  exposure. If libkrun source or a bind-address API is unavailable, record the
  exact blocker and the smallest required upstream/fork hook.
- SMH3: Add F7 malformed-input coverage for patched crun krun.port_map parsing:
  empty, malformed, out-of-range, duplicate, and long annotation input.
- SMH4: Record Linux host proof for F1-F4/F7, including focused nimbus-sandbox
  tests, nimbus-crun patch verification, rendered config.json evidence, and
  production exposure gate updates.

Success criteria:
- SMH0 through SMH4 are done, or the plan is blocked with a concrete external
  dependency that cannot be resolved from the available worktrees.
- F1-F3 are closed or explicitly pending only on Linux smoke evidence.
- F4 is fixed or blocked on a named libkrun/upstream hook with exact required
  API shape.
- F7 parser robustness has tests in the patched crun lane.
- docs/plans/security/sandbox-isolation-audit.md reflects final F1-F4/F7
  status and remaining production exposure blockers.
- docs/plans/sandbox-microvm-hardening-plan.md records every completed phase,
  verification command, result, commit, and next action.
- Active plans in docs/plans/README.md stay small and non-overlapping.

Verification expectations:
- For Nimbus Rust changes: cargo fmt --all --check, focused cargo tests for
  touched sandbox code, and git diff --check.
- For nimbus-crun changes: use /Users/jack/src/github.com/nimbus/nimbus-crun
  scripts/verify-patch.sh against the pinned crun source when available; add
  focused parser tests or a patch-level verification command for malformed
  input cases.
- For Linux smoke: run the existing krun smoke path only on a capable Linux
  host. If the current host cannot run it, record the exact host limitation and
  leave SMH4 in_progress or blocked with the command to run next.
- If a documented verification command is unavailable or fails for an unrelated
  pre-existing reason, record the exact failure and residual risk in the
  Execution Log.

Stop only when:
- the entire plan is complete,
- a real blocker requires user/design/upstream input,
- verification fails and cannot be resolved safely in the active phase,
- continuing would require a new active plan,
- or an external repository or network dependency requires permission that has
  not been granted.

Before stopping:
- Update docs/plans/sandbox-microvm-hardening-plan.md with current phase
  status, verification evidence, commits, and exact next action.
- Update docs/plans/security/sandbox-isolation-audit.md if any F1-F4/F7 status
  changed.
- Leave at most one SMH phase in_progress.
- Summarize completed phases, verification results, commits, dirty worktree
  caveats, and next action.

Do not add runtime-engine, Bun/JSC, wasmtime, WASI agent, or admission changes
under this goal. Do not mark production microVM service exposure safe until F4
has localhost-only proof and the security audit's production gates are updated.
```
