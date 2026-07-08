# Bun Fork Refresh Plan

Status: active
Date: 2026-07-08
Spec: `bun-fork-refresh-spec.md` (contract; this file owns execution order)
Fork checkout: `~/src/github.com/nimbus/bun`

Goal: refresh `github.com/nimbus/bun` from the 2026-05-23 upstream base to
upstream main `332f7444f94` (2026-07-08), republish the branch/tag/default
branch, and land one atomic Nimbus PR that repoints every pin site — with the
full verification matrix green.

Execution model: BFR0/BFR1 and BFR3/BFR4 are delegated to Codex (gpt-5.5)
rescue jobs with this plan + the spec as the brief; BFR2 (GitHub state), all
reviews, BFR5 (PR), and BFR6 (evidence) stay with the orchestrator.

## BFR0 — Rebase the delta onto the new base — `pending`

- In the fork checkout: branch `nimbus/bun-main-20260708` at
  `332f7444f94025776a173a96b0d7c584298ffea1`.
- Cherry-pick the 23-commit delta stack (`5385b59549..ad0e1d2bbc` from
  `nimbus/bun-main-20260525`) in order, preserving authorship and commit
  message intent. Resolve conflicts per the spec's invariants; re-express
  hooks in upstream's new shape instead of reverting upstream refactors.
- Record the observed upstream `package.json` version and any upstream
  release tags newer than `bun-v1.3.14`.
- Completion gate: all 23 commits applied (or explicitly folded with a
  written reason per fold); `bun scripts/build.ts` configure step succeeds;
  `cargo check` of the touched fork crates (`embed_probe`, `link_bridge`,
  `bun_bin`) passes.
- Evidence: (fill at completion — conflict log, observed upstream version.)

## BFR1 — Fork proof suite green on the new base — `pending`

- Build the proof target on darwin-arm64: profile `release-local`, simdutf
  namespace enabled, target `check-bun-embed-shared`.
- Completion gate: build succeeds; the shared adapter exports exactly the 11
  contract symbols; the probe suite (`nimbus_bun_embed_probe_*`) passes;
  build-graph audit shows dlopen-safe TLS and no muldefs.
- Evidence: (fill at completion — proof output, export list, delta HEAD SHA.)

## BFR2 — Publish fork state — `pending`

- Tag `nimbus-bun-jsc-proof-main-20260708` at the proof-verified HEAD.
- Push branch + tag with explicit refspecs; verify with `git ls-remote`.
- Flip the GitHub default branch to `nimbus/bun-main-20260708`; verify via
  `gh api repos/nimbus/bun`.
- Disable inherited upstream automation workflows on the fork.
- Completion gate: remote shows new branch, tag → verified SHA, new default
  branch; old refs untouched.
- Evidence: (fill at completion.)

## BFR3 — Repoint Nimbus pins — `pending`

- On a nimbus feature branch, update every pin site enumerated in the spec's
  pin-site table to the new ref/revision pair.
- Run the stale-ref completeness grep from the spec (zero hits outside
  immutable evidence).
- Completion gate: `make verify-bun-jsc-runtime-contract` passes;
  `scripts/verify-fork-upstream-standardization.sh` passes against the live
  fork.
- Evidence: (fill at completion.)

## BFR4 — Linked-adapter verification — `pending`

- `NIMBUS_BUN_REPO=~/src/github.com/nimbus/bun
  scripts/verify-bun-jsc-linked-adapter.sh` on darwin-arm64 against the new
  branch/tag.
- Completion gate: verifier passes end-to-end (ref/rev match, linker/TLS
  audits, shared-artifact export audit, namespace separation audit).
- Evidence: (fill at completion.)

## BFR5 — Nimbus PR — `pending`

- `cargo fmt --all --check`, `make clippy`, `make ci` on the branch.
- Open the PR against `nimbus/nimbus` main; merge on confirmed-green CI per
  the standing merge-on-green authorization.
- Completion gate: PR merged with green hosted CI including the
  `bun-runtime-contract` lane.
- Evidence: (fill at completion — PR number, merge SHA, CI verdict.)

## BFR6 — Post-merge evidence and closeout — `pending`

- Dispatch `bun-jsc-adapter.yml` for linux-x86_64 + darwin-arm64 against the
  new tag; record run results.
- Write the refresh evidence note under
  `docs/private/plans/proof/runtime-engine/bun-jsc/` (observed upstream
  version, conflict summary, proof outputs, run links).
- Update the spec's identity table if any recorded value diverged; archive
  this plan; remove its README entry per the plans-README convention.
- Completion gate: evidence note exists; plans README carries no stale entry.
- Evidence: (fill at completion.)
