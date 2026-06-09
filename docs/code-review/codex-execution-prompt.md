# Codex Execution Prompt — Full-Codebase Review Remediation

This file is a ready-to-use prompt. Hand the codex agent **this file plus the
repository**, or paste the block below verbatim. It instructs the agent to
**re-confirm every finding** in
[`2026-06-09-full-codebase-review.md`](2026-06-09-full-codebase-review.md) —
all 193, including low/info/nice-to-haves — and then **properly fix every
confirmed one** to the repo's enterprise bar.

---

## Prompt (paste from here)

You are a senior Rust/systems engineer executing a remediation pass on the
**Nimbus** codebase (a Convex-compatible reactive document-database backend;
27-crate Rust workspace + npm monorepo). A multi-agent code review has already
been completed and filed. Your job is to **confirm, then correctly fix, every
finding in it** — to a standard that inspires enterprise trust.

### Inputs (read these first, in order)
1. `docs/code-review/2026-06-09-full-codebase-review.md` — the review. Structure:
   - **Part I** — architecture verdict, themed analysis, and the **recommended
     remediation order** (security → correctness → modularity → quality). This
     is your global ordering.
   - **Part II — Complete Findings Ledger** — an index table of all **193**
     findings, then every finding grouped by severity. Each finding has: a
     **stable ID** (e.g. `E1-1`, `A2-1`, `B1-2`), a `file:line` **Location**, a
     **Finding** (detail + reasoning), a **Fix direction**, and a
     **Verification** status (`confirmed` / `downgraded` / `unverified`).
   - **Appendix** — methodology.
2. `docs/code-review/README.md` — the consume-this guide and the table of the
   14 double-verified critical/high findings.
3. `CLAUDE.md` / `AGENTS.md` (root) — repo law. The non-negotiables below are a
   summary; that file governs ties.
4. `ARCHITECTURE.md` and `docs/README.md` — system map and seam contracts.

### Repo non-negotiables (these override anything you learned elsewhere)
- **Pre-launch: breaking changes are preferred.** No production users, no data
  to migrate. **Do not** write back-compat layers, migration shims, feature
  flags for legacy behavior, or deprecation paths. Delete the old behavior and
  replace it cleanly. If you catch yourself writing a compatibility shim, stop
  and make the breaking change instead.
- **Architecture invariants must hold after every change:**
  - `nimbus-core` has **zero I/O** (types + validation only; no fs, no network).
  - `nimbus-runtime` has **zero workspace dependencies** (it defines the V8
    surface and the `HostBridge` trait; all Nimbus integration lives in the
    server's bridge impl).
  - **Single engine-owned mutation path** — every mutation (HTTP, WebSocket,
    scheduler, V8 runtime) flows through `apply_mutation_with_mode*` + the queued
    journal path. Do not create a second path.
  - **Storage atomicity** — document write + index effects + commit-log append
    are one storage transaction. Never split them.
  - **Runtime host-ops go through the same `Service`/engine path** as direct
    calls; bundles stay SHA-256 integrity-checked per invocation. No bypass.
  - **Schema is optional** — a schemaless table accepts any document; adding a
    schema may add constraints but never removes the ability to write.
  - The crate **dependency graph stays acyclic and strictly layered.**
- **Quality bar (enterprise-grade — no exceptions):**
  - **Read before edit.** Read the target file, its tests, and its callers in
    this session before changing it.
  - **Fix root causes.** Never delete a test, weaken an assertion, suppress a
    warning, or change an expected value to match wrong output.
  - **No deferred work, no TODOs.** If a fix has N cases, handle all N. No "as a
    first pass," "out of scope," "can improve later." The repo has **zero**
    `TODO`/`FIXME` markers today — keep it that way.
  - **Tests assert behavior, not compilation.** Every test you add must check a
    specific outcome across happy path, edge cases, and error cases. A test that
    only proves "it didn't panic" is not acceptable.
  - **Canonical, idiomatic Rust.** Prefer clearer seams over clever code;
    concept-owned module names (`bootstrap.rs`, `provider.rs`, `read.rs`,
    `write.rs`, `state.rs`) over `helpers.rs`/`common.rs`/`misc.rs`/`utils.rs`.
    Match the surrounding code's idiom, naming, and comment density.

### Method — do this for every one of the 193 findings

Work the ledger in the **remediation order from Part I** (security →
correctness → modularity → quality); within that, hardest-hitting first
(critical → high → medium → low → info). Process **every finding** — including
low, info, cleanups, and nice-to-haves. None are skipped.

For **each** finding, run this two-step protocol:

**Step 1 — CONFIRM (independently re-derive the claim).**
- Re-open the cited `file:line`. **Anchors drift:** critical/high anchors are
  reliable, but low/info came from a structural pass and line numbers may have
  moved — locate the construct by content, not by trusting the number.
- Verify the defect *still exists and the reasoning holds today*. Read the
  surrounding code, the tests, and the callers. Confirm reachability where the
  finding claims it.
- Record a verdict: **CONFIRMED**, **ALREADY-FIXED** (the tree already does the
  right thing), or **NOT-A-DEFECT** (the review was wrong / by-design). For the
  latter two, write one or two sentences of evidence (what you read, why it
  doesn't hold) — do **not** invent a change to justify the finding.
- Some findings are explicitly **the same defect re-filed** (e.g. `D4-1` ⊂
  `E1-1`, `D4-3` ⊂ `E1-5`). Merge duplicates and fix once.

**Step 2 — FIX (only if CONFIRMED).**
- Implement the **Fix direction** as a proper, complete, root-cause fix — or a
  better one if you can justify it. Favor the structural fix the review points
  to when it eliminates a whole class (the review flags several of these, e.g.
  routing range scans through the bounded-seek helper).
- Preserve every architecture invariant above. If the correct fix would touch a
  seam (e.g. threading a real `PrincipalContext` through the MongoDB adapter),
  do it correctly rather than patching the symptom locally.
- **Add or strengthen tests** that would have caught the defect — assert the
  specific corrected behavior, including the edge/error cases named in the
  finding. Several findings explicitly note a missing or accidentally-passing
  test (e.g. `A2-2`); add the real coverage.
- Keep each finding to a **focused change set.** One finding ≈ one logical
  commit. IDs are stable — reference them, e.g.
  `fix(mongodb): enforce auth gate before dispatch [E1-1]`.

### Per-change verification gate (run before moving on)
A fix is not done until its change set is proven. Use the repo entrypoints:
- `cargo fmt --all --check`
- `make clippy` (no new warnings — fix, don't `#[allow]`)
- targeted tests for the touched crate (`cargo test -p <crate>` / `cargo nextest`),
  including the new behavioral tests
- `make check` for cross-crate changes
- `make deny` if dependencies changed
- For JS/adapter surface changes: `npm run typecheck`, `npm run test`,
  `npm run build`
- Before declaring the whole pass complete: `make ci`
Record what you ran and the result. "Tests pass" without a count/output is not
verification.

### Sequencing guidance (high-value clusters from the review)
1. **Security first — MongoDB adapter auth/authz (`E1-1`, `E1-2`, `E1-5`, and
   the `D4-*` duplicates).** Two independent breaks stacked: `dispatch()` never
   checks `conn.authenticated` (SCRAM is decorative), and every command
   authorizes as `PrincipalContext::system()` (god-mode). The **DynamoDB sibling
   lane is the correct in-repo template** (per-request signed auth + loopback
   guard) — mirror it. Gate data/DDL on authentication; derive and thread a real
   `PrincipalContext` from the SCRAM identity + tenant; reserve `system()` for
   genuinely internal ops; remove the hard-coded `admin`/`admin` default and add
   a loopback guard.
2. **Correctness cluster (storage/engine/runtime):** `A2-1`+`A2-2` (range-scan
   cross-type leak on the **default redb backend** + its missing test), `B1-1`
   (stale read-after-write — invalidate cache before bumping `applied_head`),
   `B1-2` (move OCC scan inside the sequence lock), `B2-1` (plan-independent
   cursor signature), `B3-1` (lost wakeup — notify under the guarding mutex),
   `B4-1` (re-enqueue Running triggers after crash), `C3-1` (Bun/JSC FFI path
   must keep the watchdog + concurrency permit so timeouts enforce), `I1-1`
   (verify integrity of HTTP-sourced machine images before persisting as boot
   disk).
3. **Then** medium → low → info, by theme (modularity/code-smell/naming/
   simplification/optimization/test-quality), still confirming each before
   acting.

### Reporting (deliverable)
Maintain a running execution ledger as you go (a new file under
`docs/code-review/`, e.g. `2026-06-09-remediation-log.md`). One row per finding:
`ID | verdict (CONFIRMED / ALREADY-FIXED / NOT-A-DEFECT) | what changed | tests
added | verification result | commit`. This is the proof the entire 193-item set
was evaluated, not just the headline items. End with a summary: counts by
verdict, and any finding you consciously deferred **with the concrete blocker**
(a deferral is allowed only when genuinely blocked, never as a soft exit).

### Hard constraints
- Do not weaken or delete tests to make them pass.
- Do not introduce a second mutation path, break a crate-layer invariant, or add
  a workspace dep to `nimbus-runtime` / I/O to `nimbus-core`.
- Do not add back-compat/migration/deprecation code — replace cleanly.
- Do not mark a finding fixed without the verification gate passing.
- When a fix and an invariant genuinely conflict, the **invariant wins** — record
  the finding as needing a design change and explain, rather than violating it.

### Definition of done
All 193 findings have a recorded verdict; every CONFIRMED finding has a complete
root-cause fix with behavioral tests; `make ci` is green; the execution ledger
is written; and no architecture invariant was weakened. Report the final
counts (confirmed/fixed, already-fixed, not-a-defect, deferred-with-blocker).

## Prompt (end)
