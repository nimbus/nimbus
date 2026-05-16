# Desktop Plans — Autonomous Mission

This file is the in-tree control plane for the autonomous, multi-session
work to drive both desktop plans to `done` and archived. The two plans
themselves are the per-item control plane; this file binds them together
and records the durable authorizations and resume procedure.

## Mission

Drive both desktop plans to `done` and archived, with every roadmap item
passing its plan's Verification Contract:

- [`docs/plans/desktop-ui-plan.md`](desktop-ui-plan.md) — Phase 1,
  embedded operator console SPA. Implementation 100% done (DU0–DU10 +
  DU11 hardening). Remaining: pass the revised archive gate (see
  "Phase 1 archive gate" below), then move to `docs/plans/archive/`.
- [`docs/plans/desktop-shell-plan.md`](desktop-shell-plan.md) — Phase 2,
  native Electron shell. DS0 done; DS1–DS10 remain. After DS10 closeout,
  move to `docs/plans/archive/`.

## Stop condition

Mission completes when ALL of these are true:

1. `docs/plans/desktop-ui-plan.md` lives at
   `docs/plans/archive/desktop-ui-plan.md`.
2. `docs/plans/desktop-shell-plan.md` lives at
   `docs/plans/archive/desktop-shell-plan.md`.
3. `docs/plans/README.md` does NOT list either plan under "Active
   execution plans" (both appear in the archived list instead).
4. This mission file (`docs/plans/desktop-mission.md`) is moved to
   `docs/plans/archive/desktop-mission.md` as the mission-completion
   record.
5. Final closure commit on `main` is pushed to `origin/main` on
   `nimbus/nimbus` (and `nimbus/desktop` for any Phase 2 artifacts).

## Control plane

The two plans are the per-item control plane. Read them in full at the
start of each session. The `Status:` field on each roadmap item plus
the per-plan execution log are the authoritative record of progress;
this mission file does not duplicate that state.

## Source of truth

Local git state on `main` is the source of truth. Plan edits, code
changes, and execution-log rows must be committed before the work
counts as done. Resume always reads from the current `main` HEAD.

## Authorizations (durable, scoped to this mission)

These are durable, scope-specific authorizations recorded by operator
(jack@spirou.io) on 2026-05-15. Inside the mission scope, do not pause
to ask:

- **Commit + push** to `main` on `nimbus/nimbus` and `nimbus/desktop`
  directly. No PRs (pre-launch project; commits to `main` are the
  tracking mechanism).
- **Create repos** via `gh repo create` as needed.
- **Run gh actions** via `gh workflow run` (and re-run via
  `gh run rerun`) for DS9 release-CI verification.
- **Operate across multiple sessions and compaction events** — that is
  the expected mode, not an exception.

Outside this mission scope, the normal "ask before risky shared-state
actions" rule from `CLAUDE.md` continues to apply unchanged.

## Resume procedure (compaction-safe)

After any compaction event, a fresh agent enters via the entry-point
prompt (below) and runs:

1. Read this file in full.
2. Read both desktop plans in full.
3. Identify the next pending item by scanning per-item `Status:` fields
   top-to-bottom; the lowest-numbered `pending` item wins.
4. If no pending items remain and the stop condition above is met →
   archive both plans + this file, commit, push, stop.
5. Otherwise → execute the next pending item under its plan's
   Verification Contract (see below). Add an execution-log row with
   concrete evidence. Flip its `Status:` to `done`. Commit. Push.
   Return to step 3.
6. If blocked on external feedback (Apple notarization round-trip,
   gh workflow run) → state what is blocked, skip that item, and
   continue with the next non-blocked item. Do not idle.

## Verification rigor

Each roadmap item must satisfy its plan's Verification Contract before
its `Status:` flips to `done`. The contracts are defined in:

- `desktop-ui-plan.md` lines 140–160 (Verification Contract section).
- `desktop-shell-plan.md` lines 121–138 (Verification Contract section).

The bar that applies to *every* item (UI or shell):

- `npm run lint` (Biome) clean.
- `npm run typecheck` (`tsc --noEmit`) clean.
- `npm run test` green (with explicit test count).
- For UI/renderer items: browser-driven verification via
  `playwright-cli` + `chrome-devtools-mcp` (Electron items connect to
  the shell's CDP endpoint via `--remote-debugging-port`). Capture
  screenshots under `.playwright-cli/` and reference the paths in the
  execution-log row.
- For shell-packaging items (DS6+): `electron-builder --dir` produces
  an unpacked build for the current platform without errors; fuse
  report shows the required fuses flipped; `webPreferences` review
  confirms sandbox + contextIsolation + nodeIntegration: false.
- For verification of the embedded UI surface specifically: axe-core
  4.10 (WCAG2 A/AA + 2.1 A/AA tags) loaded same-origin against the
  live route in both themes; zero critical or serious violations
  required.
- Per-item manual verification described in the item's own
  "Verification" subsection (in addition to the above).

The execution-log row must record:

- Concrete commands run + exit codes / output summaries.
- Test counts (`N files / M tests passed`).
- Bundle sizes / artifact paths.
- Screenshot paths and what each captures.
- Axe-core violation/passes counts for both themes.
- Commit hash on closure.
- Any deferrals (with the explicit Phase 2 disposition for each).

Reference rigor: the DU6.5, DU7, DU8, DU9, DU11, and DS0A rows already
in the execution logs are the standard. New rows should match that
depth of evidence — no shorter, no thinner.

## Phase 1 archive gate (revised 2026-05-15)

**Original gate** (in the 2026-05-15 closeout-audit row of
`desktop-ui-plan.md`): "closed DU log + one operator-week dogfood +
deferral-matrix review + green `make ci`".

**Revised gate** (this mission, 2026-05-15): "closed DU log + DU11
hardening landed + deferral-matrix review + green `make ci`".

Rationale: the "operator-week dogfood" clause was authored before DU11
added an in-tree rotate-token E2E, shutdown E2E, and a sustained
100+ events/sec live-tail perf lane. Those hardening lanes provide the
stability proof the dogfood week was meant to capture, in a form that
is verifiable in `make ci` rather than calendar-bound. The revised gate
preserves the original intent (proof of stability) while eliminating a
calendar requirement that no longer adds verification value.

**Current status against the revised gate:**

- Closed DU log: ✅ (DU0–DU10 all `done`).
- DU11 hardening landed: ✅ (commit `a114ffe2`).
- Deferral-matrix review: ✅ (12 deferrals captured in
  `desktop-ui-plan.md` lines 956–975 with per-row Phase 2 disposition).
- Green `make ci`: re-run required against current HEAD. Schedule at
  DS10 closeout or sooner if the Phase 1 archive is unblocked first.

## External feedback loops (anticipated)

These items require an external round-trip and may temporarily block
that item without blocking the mission:

- **DS8 (signing + notarization):** Apple notarization round-trip
  typically 5–30 min per submit. Use
  `xcrun notarytool submit <pkg> --wait` (or staple after submit).
  While a notarization is in flight, work other unblocked items.
- **DS9 (release CI):** requires a real tagged release on
  `nimbus/desktop` to fire the workflow. Use a
  `v0.0.0-dryrun-<n>` tag for proof runs and delete the tag + release
  after the workflow validates. Real `v0.x` releases land at DS10.
- **Phase 1 archive `make ci`:** ~10–15 min run on `nimbus/nimbus`.
  Schedule once when Phase 1 is otherwise ready to archive.

## Failure handling

These rules apply throughout the mission:

- **Test / lint / typecheck failure** → fix root cause. Never delete a
  test, weaken an assertion, suppress a warning, or change an expected
  value to match wrong output (CLAUDE.md "Fix root causes").
- **Browser-driven proof fails** → diagnose the underlying issue and
  fix it. Re-run the proof. Do not skip.
- **Verification-contract gate cannot be satisfied without weakening
  it** → STOP the mission. Surface the blocker to the operator. Do not
  proceed by lowering the bar.
- **Pre-commit hook fails** → fix and make a new commit (CLAUDE.md
  rule: never amend or use `--no-verify`).
- **Push fails** (auth, conflict, network) → investigate and retry.
  Never force-push.
- **External-feedback timeout** (notarization > 30 min, workflow run
  hangs) → record the symptom, defer that item, work others. Recheck
  on next iteration.
- **Plan invariant about to be violated** (e.g., bypassing `Service`,
  weakening the auth-runtime trust boundary, embedding a `nimbus`
  binary inside the Electron app) → STOP. Surface to the operator.

## Entry-point prompt

The operator enters the mission with the built-in `/goal` slash
command. Paste this exact message:

```text
/goal Drive the desktop mission to its in-tree stop condition. Read
docs/plans/desktop-mission.md, docs/plans/desktop-ui-plan.md, and
docs/plans/desktop-shell-plan.md in full. Identify the next pending
roadmap item (lowest-numbered DS with Status: pending, or the Phase 1
archive gate if Phase 1 is otherwise ready). Execute it under its
plan's Verification Contract end-to-end. Add an execution-log row
with concrete evidence matching the depth of the existing DU6.5 /
DU7 / DU8 / DU9 / DU11 / DS0A rows. Flip Status to done. Commit a
focused baseline and push to main on the relevant repo
(nimbus/nimbus, nimbus/desktop, or both). Repeat. The goal is
complete only when the stop condition in desktop-mission.md is
fully met: both plans plus this mission file moved to
docs/plans/archive/, docs/plans/README.md updated, final closure
commit pushed to origin/main. Authorizations recorded in the mission
file apply throughout. If blocked on external feedback (Apple
notarization, gh workflow run), state what is blocked and continue
with anything not gated by that block.
```

The `/goal` built-in keeps the agent working until the condition is
met. This goal's condition is the mission's stop condition — no
partial credit. After compaction, the same paste re-enters cleanly
because the persistent state (this mission file + the two plans +
git HEAD on `main`) is complete on its own.

## Mission audit trail

| Date | Event |
| --- | --- |
| 2026-05-15 | Mission authored. Phase 1 archive gate revised from "operator-week dogfood" to "DU11 hardening landed". Durable authorizations recorded (commit/push main, create repos, run gh workflows). Entry-point prompt registered. |
