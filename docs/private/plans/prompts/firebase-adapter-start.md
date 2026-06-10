# Codex Agent Start Prompt - Firebase Adapter Continuous Resume

Historical prompt for the archived Firebase/Firestore adapter execution wave.
Use it only to review or replay the completed control-plan record from the
current repo state. Copy the full text below into a fresh Codex agent.

---

## Prompt

You are Codex working in the Nimbus repository:

`/Users/jack/src/github.com/nimbus/nimbus`

Nimbus is a Rust workspace plus npm monorepo that implements a
Convex-compatible backend server. The current work is the Firebase/Firestore
compatibility adapter and the Nimbus core primitive hardening needed to keep
database semantics out of compatibility adapters.

Your source of truth is the archived control-plane plan:

`docs/plans/archive/firebase-adapter-plan.md`

Do not rely on chat history. Resume from git state plus the plan's
`pending` / `in_progress` / `done` status ledger.

## Current State

At the time this prompt was written:

- `F0.1 Document-key design and implementation` is `done`.
- `F0.2 Resource path and collection group metadata model` is `done`.
- The next unblocked roadmap item should be `F0.3 Atomic write batch
  primitive`, unless the current worktree or plan status shows another item is
  already `in_progress`.
- The immediate goal is to keep progressing through the Firebase plan
  autonomously rather than stopping after a single completed item.

Do not redo completed items unless the current worktree proves they are
incomplete or incorrect.

## Required Startup

1. Read these files first:
   - `AGENTS.md`
   - `README.md`
   - `ARCHITECTURE.md`
   - `docs/README.md`
   - `docs/plans/README.md`
   - `docs/plans/archive/firebase-adapter-plan.md`
2. Run `git status --short`.
3. If the worktree is dirty, inspect the changed files before editing. Treat
   existing changes as user or prior-agent work; do not revert them unless the
   user explicitly asks.
4. In `docs/plans/archive/firebase-adapter-plan.md`, check whether any roadmap item is
   `in_progress`.
   - If yes, resume that item.
   - If no, pick the first unblocked `pending` item in roadmap order.

## Execution Mode

Work in a continuous item-by-item loop, not a one-item-only pass.

Within a single Codex run:

1. Select the active roadmap item from the plan.
2. Mark that one item `in_progress`, update the top-level control item, and add
   an execution-log start row if needed.
3. Implement the item end to end when feasible.
4. Run focused verification for that item.
5. If the item is complete, mark it `done`, update the phase ledger if needed,
   and record verification in the execution log.
6. Then immediately pick the next unblocked `pending` item and continue in the
   same run.

Do not stop just because one roadmap item is complete.

Treat completion of a roadmap item as an intermediate checkpoint, not a final
stop condition.

## When To Stop

Only stop the run if one of these is true:

- You are genuinely blocked by a design issue, failing verification you cannot
  resolve, missing information, or a required user decision.
- Continuing would require an unsafe or very large cross-cutting change that
  should be split in the plan first.
- The current phase is complete.
- You have made substantial progress across multiple adjacent roadmap items and
  need to checkpoint because context limits would make the next item risky.

If you stop for any reason other than phase completion, leave the next item
clearly marked in the plan and record the exact next action in the execution
log.

## Immediate Priority

Unless the plan or worktree shows another item is already `in_progress`, start
with:

`F0.3 Atomic write batch primitive`

Goal: add a shared write batch surface over the engine-owned commit path that
models Firestore set/patch/delete/verify/transform semantics without creating a
Firebase-specific write path.

Important semantic gaps called out by the plan:

- Overwrite must support create-if-missing atomically inside the execution
  unit.
- Delete without a precondition must be able to succeed on a missing document.
- Transform forms must be recognized from the start even if executable
  transform semantics land later.
- Ordered per-write results and atomic rollback semantics must remain correct.

After `F0.3`, continue automatically to the next unblocked item, expected to be
`F0.4`, then `F0.5`, and so on, unless you become blocked.

## Sources To Inspect First

Start with the plan sections for the active item, its dependencies, the
execution log, and the Source Evidence Map. Then inspect the relevant code
before designing the change.

For `F0.3`, begin with:

- `crates/nimbus-core/src/mutation.rs`
- `crates/nimbus-engine/src/service/execution_units/`
- `crates/nimbus-engine/src/service/mutations/`
- `crates/nimbus-storage/src/store/write/`
- `crates/nimbus-storage/src/index/`
- Any new path/resource metadata added for `F0.2`
- Firebase protocol sources listed in the plan's Source Evidence Map,
  especially `write.proto`, `firestore.proto`, and the Firebase JS SDK
  serializer/persistent stream sources

## Architectural Constraints

- `nimbus-core` remains zero I/O: shared types and validation only.
- `nimbus-runtime` keeps zero workspace dependencies.
- Every mutation path continues through the engine-owned mutation path and
  queued journal path. Do not create a Firebase-specific write path.
- Document write, index effects, and commit-log append must remain one storage
  transaction.
- This project has not launched; prefer clean breaking changes over shims.
- Keep storage-visible metadata outside user document fields.
- If existing Convex adapter logic turns out to be general database behavior,
  promote it into core/engine/server shared code rather than copying it into
  Firebase.
- At any given moment, keep edits scoped to the active roadmap item, but once
  that item is complete, continue to the next unblocked item in the same run.

## Plan Discipline

- Follow the completion gate and verification evidence listed for each roadmap
  item in `docs/plans/archive/firebase-adapter-plan.md`.
- If an item is too large for one safe pass, split it in the plan before
  implementation and continue with the first resulting sub-item.
- Keep the plan, phase ledger, control item, and execution log up to date as
  durable progress state.

## Verification Expectations

For each completed item:

- Run the narrowest meaningful `cargo test` or `cargo check` lane for the
  touched crates first.
- Run `cargo fmt --all --check`.
- Run `make clippy` when the item broadly affects shared primitives across
  `nimbus-core`, `nimbus-engine`, `nimbus-storage`, or `nimbus-server`, or if
  you are preparing a PR.
- Record every command and result in the execution log before moving on.

## Working Style

- Implement the active item end to end when feasible.
- Add abstractions only when they clarify the Nimbus core primitive boundary.
- Add tests proportional to the blast radius.
- Use `rg` / `rg --files` for search.
- Use `apply_patch` for manual file edits.
- Do not use destructive git commands.
- Do not stage or commit unless the user asks.

## Final Response

When you finally stop the run:

- Summarize the roadmap items completed in this run.
- List verification commands run and their results.
- State the exact next item and next action if the phase is not complete.
- Keep the answer concise.
