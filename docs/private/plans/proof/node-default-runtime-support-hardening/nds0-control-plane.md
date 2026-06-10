# NDS0 Control Plane Proof

status: in_progress
date: 2026-06-01
branch: codex/node-default-runtime-support-hardening
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening
pr: _pending initial NDS0 scaffold commit/push_
verifier: scripts/verify-node-default-runtime-support-hardening.sh

## Row And Status

NDS0 is in progress. This proof records the execution surface that survives
context compaction.

## Broad Pre-Run

Control-plane bootstrap checks:

```console
git worktree add -b codex/node-default-runtime-support-hardening /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening origin/main
git apply /private/tmp/nds-plan-hardening.patch
git rev-parse --abbrev-ref HEAD
git rev-parse HEAD
```

Observed:

- branch: `codex/node-default-runtime-support-hardening`
- base commit: `db30ddac8776c0105ae8ebdefcd85541a6d11fc2`
- worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`

## Failure Grouping

Control-plane gaps remaining before NDS0 can close:

- Draft PR URL is pending until the initial NDS0 scaffold commit is pushed.
- Main-visible pointer is pending. The plan requires either a pointer artifact
  visible from `origin/main` or a developer-approved fallback that relies on the
  draft PR plus local goal state.
- Final verifier will fail while NDS1-NDS10 are pending by design.

## Focused Work

Active goal objective:

> Complete `docs/plans/node-default-runtime-support-hardening-plan.md`
> autonomously end to end in a dedicated worktree and draft PR. Success means
> Node24 becomes a verifier-backed well-supported default, Node22 remains a
> comparable supported LTS peer, Node26 gets real Current-line evidence,
> Application canaries reach at least 50 claims across 12
> schema-controlled `compat_category` values, Convex-compatible `"use node"` app
> suites reach at least 5, non-isolate behavior stays fail-closed, generated
> docs match evidence, PR/nightly gates are wired, and the final verifier has
> zero failed required checks. If NDS1 or NDS3 proves the Node24 `2000` gate is
> unreachable truthfully, stop in the documented blocked state with exact
> fixtures, follow-up plan, ledger/pointer updates, and unsatisfied gates
> recorded.

Resume protocol:

1. Read `AGENTS.md`.
2. Read `docs/plans/node-default-runtime-support-hardening-plan.md`.
3. Read this file and `nds0-baseline.md`.
4. Run `git status --short --branch` in the dedicated worktree.
5. Resume the first `in_progress` row, or the first `pending` row if no row is
   in progress.
6. Before handoff or compaction, update the row proof, plan ledger, Active
   Execution Pointer, and verifier output.

Deno fork publish/repin protocol:

1. Keep Nimbus-specific bootstrap/profile/capability fixes local.
2. Promote fixes to `~/src/github.com/nimbus/deno` only when local code would
   duplicate Deno/Node builtin semantics, shadow internal behavior long-term, or
   add avoidable hot-path overhead.
3. Temporarily unpin Nimbus to the local Deno worktree only for proving the
   fork-owned fix.
4. Commit/tag/push the Deno fork.
5. Repin Nimbus `Cargo.toml` and `Cargo.lock` to the immutable fork tag.
6. Rerun Nimbus verification against the repinned tag before updating row
   status.

## Broad Final Rerun

NDS0 scaffold verification:

```console
bash scripts/verify-node-default-runtime-support-hardening.sh
git diff --check
npm run docs:validate-refs:strict
```

Observed:

- `bash scripts/verify-node-default-runtime-support-hardening.sh`:
  `8 passed, 26 failed`.
- `git diff --check`: pass.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (241 working-tree Markdown files)`.

The failing verifier conditions are expected future-row gates. The control-plane
and baseline checks pass.

## Evidence Links

- Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
- Branch: `codex/node-default-runtime-support-hardening`
- Draft PR: `_pending initial NDS0 scaffold commit/push_`
- Plan: `docs/plans/node-default-runtime-support-hardening-plan.md`
- Baseline proof: `docs/plans/proof/node-default-runtime-support-hardening/nds0-baseline.md`
- Verifier: `scripts/verify-node-default-runtime-support-hardening.sh`

## Residual Risks

- NDS0 remains `in_progress` until the draft PR URL or approved substitute is
  recorded.
- A direct main-visible pointer update requires explicit developer approval.
  Without that approval, the draft PR plus active goal must be recorded as the
  fallback.
