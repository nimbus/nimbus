# RRC8 Deno 2.9.6 Uplift

Date: 2026-08-29

Status: `in_progress`.

## Outcome

Nimbus will consume a reviewed, immutable fork of current upstream Deno
`v2.9.6`. The consumed Deno graph will use the corresponding reviewed Nimbus
V8 150.4 fork. No temporary revision, local path, or older V8 line can remain
in the final dependency closure.

## Baseline

- Upstream Deno `v2.9.6` peels to
  `e518fbd66dda5debcbdefc0beb0b3756b37b64fa`.
- The working Nimbus Deno checkpoint `v2.9.3-nimbus.2` peels to
  `d0a6b9094e0da6acbb53ecd0d88ed6b81a142e63`.
- Deno 2.9.3 through 2.9.6 contains 256 upstream commits and changes 918
  files. None of the 21 Nimbus commits is patch-identical to a 2.9.6 commit.
- Deno 2.9.6 introduces `deno_v8` 0.3.0 and requires rusty_v8 150.4.0.
- Upstream rusty_v8 `v150.4.0` peels to
  `5c15a6995c9bb4bacd3e341b59fff32c909c80bf`. It is eight commits and seven
  changed files after `v150.2.0`.
- The existing Nimbus V8 150.2 release peels to
  `4786595e29679ee5ad9ba4925cdcd1cc83ab6448`. Its 19 fork commits have no
  patch-identical adoption in upstream 150.4.
- The canonical Deno checkout retains the user's dirty `tests/wpt/suite`
  submodule. The canonical rusty_v8 checkout retains its five unrelated
  isolate-group files. All uplift edits use clean dedicated worktrees.
- The valid `v2.9.3-nimbus.2` Nimbus checkpoint uses
  `304a2e677293fec7d150e12ffc0ba98960917753`.

## Execution Ledger

| ID | Work | Status | Evidence |
| --- | --- | --- | --- |
| U1 | Port and verify the Nimbus rusty_v8 carries on upstream 150.4. | `complete` | Exact candidate `dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0`; local source-build gates and final Sol xhigh review pass. |
| U2 | Review, publish, and re-query an immutable rusty_v8 150.4 release. | `todo` | The local candidate is ready. Push, tag, and release require explicit owner authorization. |
| U3 | Audit the 21 Deno carries and replay only product-required concepts on upstream 2.9.6. | `in_progress` | The clean worktree remains at `e518fbd66`. Omit realm-only carries and pair that omission with Nimbus consumer cleanup before U4. |
| U4 | Test Deno itself and Nimbus against the exact unpublished candidate. | `todo` | Requires U2 and U3. |
| U5 | Review and publish an immutable Deno 2.9.6 release. | `todo` | Requires U4 and explicit owner authorization for external writes. |
| U6 | Repin Nimbus, update fork policy, and run the exact release replay. | `todo` | Requires U5. |

## Carry Rules

1. Preserve upstream 2.9.4 through 2.9.6 security and lifecycle fixes.
2. Retain a Nimbus carry only when current Nimbus source or a direct regression
   needs its contract.
3. Rework a carry at the current concept boundary when upstream refactored its
   old location. Do not restore removed upstream structure.
4. Bind Deno candidate tests to the same Nimbus rusty_v8 150.4 revision that
   Nimbus will consume.
5. Publish only annotated, never-moved fork tags after local checks and hosted
   CI pass. Run independent Sol xhigh review before publication. Do not invoke
   Opus 5 or Fable until the owner explicitly permits those reviewers again.
6. Preserve all unrelated dirty files and old branches, tags, releases, and
   proof artifacts.
7. Do not publish or repin the Deno candidate before the runtime-strategy
   disposition and controlled replay-scaffolding A/B are complete.

## U1 Evidence

The exact local rusty_v8 candidate is
`dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0`. It contains the V8 150.4 carry
port plus the final persistent-handle, offline-binding, and release-workflow
repairs.

- `V8_FROM_SOURCE=1 cargo nextest run --all-targets --locked` passes 308 tests
  across 25 binaries.
- Warning-denied all-target Clippy and Rust formatting pass.
- Documentation tests pass 13 tests and ignore 13 examples by declaration.
- C++ formatting and the Nimbus release workflow pass their local syntax
  checks.
- The release tool suite passes 15 tests. The candidate selects
  `v150.4.0-nimbus.1` and defines exactly 44 assets across seven targets.
- Sol xhigh found and closed two P2 defects: stale offline binding reuse and
  mutable release action references. The final exact-commit review reports no
  P0 through P3 finding.
- The owner prohibited Opus 5 and Fable review use. The active Opus 5 review
  then stopped with exit 130. It is not acceptance evidence. No Opus 5 or Fable
  review can run until the owner changes this constraint.

U2 external writes remain intentionally unstarted. The active goal forbids a
push, tag, or release without explicit authorization.

## U3 Architecture Disposition

Decision: omit the proven realm-only carries during the first 2.9.6 replay.
Port by product contract. Do not cherry-pick all 21 commits.

Evidence:

- The Deno 2.9.6 worktree is clean at exact upstream `e518fbd66`. U3 has not
  replayed a carry.
- Commit `8d92e814b9` mixes Locker ownership, teardown, shared-heap work, and
  fresh-realm replay in one 1,705-line change. A mechanical replay cannot keep
  those contracts separate.
- Nimbus defaults to exact-key `WarmPool` with `CooperativeLocker`. Node targets
  use startup snapshots, while WebStandard uses unsnapshotted construction.
- `WarmContextRecycle` is a public, serialize-only Rust value with live product
  branches. Both facade crates re-export it. Server and UI surfaces report it,
  but the audit found no CLI, package, server, or compute selector.
- PIR2 rejects realm recycle at 2.23 to 2.50 times its Web comparison lane.
  NFR6 rejects it at 5.38 to 10.01 times Node startup snapshots. It is 13.35 to
  16.13 times exact warm pools.
- Deno prepares and stores replay sources during ordinary `JsRuntime`
  construction. Nimbus also serializes two replay tables into the Node snapshot
  companion blob. Their exact ordinary-construction cost remains unknown.

U3 must preserve Locker ownership, egress enforcement, the Node near-heap
policy, applicable TCP maintenance, and fork CI. It must audit shared-heap and
teardown hunks independently. It must also keep the generic lazy-ESM termination
error contract unless a current regression proves that upstream replaced it.

The paired Nimbus candidate removes `WarmContextRecycle` product selection
before U4. It also removes fresh-realm execution and replay companion data. It
keeps residual lazy sources, exact owner and reuse authority, heap limits,
shared-heap safety, pointer compression, and reset-or-destroy behavior. U3 must
stop before realm omission if its caller audit finds a supported external
consumer.

Before U5 tags Deno, the candidate must archive the rejected experiment as an
exact patch or source tag. It must also compare ordinary construction and
snapshot size with and without replay scaffolding. Every row must bind the
Nimbus, Deno, and rusty_v8 SHAs, pointer-compression setting, compatibility
target, requested pool kind, actual construction mode, and execution model.

The proposed `runtime-strategy-lifecycle-plan.md` consumes this proof after U6.
Its activation trigger is terminal U6 evidence with exact commits and A/B data,
followed by owner approval. RRC8 keeps exclusive ownership until that trigger.

## Acceptance

- rusty_v8 formatting, compile-fail, Locker, weak-handle teardown, selector,
  release-manifest, and candidate asset checks pass at one exact 150.4 commit.
- Deno locked workspace checks and all Nimbus-patched crate tests pass at one
  exact 2.9.6 commit. Egress regressions prove URL and resolved-address policy,
  proxy behavior, custom-client handling, and bounded equivalent-client reuse.
- The Deno candidate contains no realm-only carry without a supported consumer.
  The archived experiment and the controlled replay-scaffolding A/B exist before
  any Deno tag or Nimbus repin.
- Nimbus compiles and passes runtime, bridge, NodeFull, egress, teardown,
  snapshot, and cage gates against the exact unpublished candidate.
- Both fork releases are public, non-draft, non-prerelease, immutable, and have
  successful branch and tag CI at the peeled commits.
- Nimbus consumes published tags only. Provenance, upstream standardization,
  Deno/V8 coupling, `make ci`, macOS, Linux, application, desktop, archive, and
  OCI candidate proofs pass before the release verdict can change.

## Current Next Action

Keep U2 publication deferred. Start U3 from clean upstream Deno 2.9.6 with the
architecture disposition above. Audit every old carry before the first Deno
edit, then replay only the current product contracts.
