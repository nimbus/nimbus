# Runtime Strategy Lifecycle Draft Audit

Date: 2026-08-29.
Plan: `docs/private/plans/runtime-strategy-lifecycle-plan.md`.
Draft baseline: `304a2e677293fec7d150e12ffc0ba98960917753`.
Verdict: independently reviewed and ready as `proposed`, but not ready for
activation.

## Review Scope

This audit independently checks the draft against five concerns:

1. Current source and history support its baseline facts.
2. The plan does not conflict with active release work.
3. The lifecycle preserves experiments without dormant product code.
4. Each task has an owner, failure proof, acceptance state, and command.
5. The document follows the plans and technical-writing skills.

This audit does not review an implementation. This review changes no product or
fork code and does not start an RSL task.

## Source Audit

| Check | Result | Evidence |
|---|---|---|
| User comparison anchor | pass | `f5f81336688dbb6e1994b76ab256f72d4f1362c7` is dated 2026-08-21. It is later than the profile-aware runtime work. |
| Actual PIR introduction | pass | `git log -S WarmContextRecycle` identifies `0d934c5b9b4782e43467191bb64e68163ba63ab2` from 2026-06-21. |
| Web snapshot restriction | pass | `53ad5e25c9f845f6a555a3157011c76cc06cf8a1` added the current cross-profile cage repair on 2026-06-27. |
| Post-anchor strategy delta | pass | The focused `git diff --quiet` from `f5f813366` through `304a2e677` exits 0 for pool policy, invocation, construction, and the main pool benchmark. |
| Current product type | pass | `RuntimePoolKind::WarmContextRecycle` remains a public, serialize-only value. Execution planning, the worker loop, invocation, pooling, and retained-state code still branch on it. |
| External selector search | pass with limit | Both facade crates re-export the type. Server and UI code report it. No CLI, package, server, or compute selector was found. RSL4 still requires a complete caller and serialization audit. |
| Construction truth | pass | `V8RuntimeConstructionMode::for_compatibility_target` selects `StartupSnapshot` for Node targets and `Unsnapshotted` for other V8 targets. |
| Web performance evidence | pass with qualification | PIR2 records 2.23 and 2.50 ratios against the row labelled `startup_snapshot_cache`. The label does not prove Web snapshot use. |
| Node performance evidence | pass | NFR6 records realm lease at 5.38 to 10.01 times startup snapshots and 13.35 to 16.13 times exact warm pools. |
| Replay source coupling | pass | Nimbus snapshot blobs include replay source tables. The Deno runtime stores and replays the same source classes. |
| Replay scaffolding cost | unknown | Current proof has no controlled ordinary-construction A/B with the scaffolding removed. RSL8 owns this measurement. |
| Lazy-ESM termination | pass with independent audit required | Its error contract does not depend on realm pooling. RSL7 forbids automatic removal with realm-only carries. |
| Fork ledger drift | pass | The current ledger names Deno 2.9.1 while `Cargo.toml` names `v2.9.3-nimbus.2`. RRC8 already owns the replacement. |
| Uplift state | pass | U1 remains in progress, U2 through U6 are todo, and the Deno 2.9.6 worktree remains clean at exact upstream `e518fbd66`. U3 has not replayed a carry. |

## Control-Plane Audit

| Check | Result | Evidence |
|---|---|---|
| One outcome | pass | Product code keeps supported winners. The lab and archive preserve reproducible alternatives. |
| Active-plan precedence | pass | RRC8 keeps exclusive ownership of the Deno 2.9.6 and V8 150.4 uplift. |
| Safe plan state | pass | Status is `proposed`. The activation gate requires terminal U6 evidence and owner approval. |
| Source authority | pass | Current source and tests override historical labels. Historical measurements remain unchanged. |
| Conditional cleanup | pass | RSL4 must prove callers and carry ownership before RSL5 through RSL7 remove code. |
| Safety before speed | pass | Isolation, semantics, cancellation, memory, recovery, and crash gates precede scoring. |
| Decision method | pass | The plan uses workload-class Pareto winners instead of one global microbenchmark winner. |
| Product and lab split | pass | Product types contain supported choices. Lab metadata and exact patches preserve rejected choices. |
| Publication order | pass after correction | RSL7 prepares an unpublished fork candidate. RSL8 runs the controlled A/B before any tag, release, or repin. |
| Future backend use | pass | RSL3 tests backend-neutral lifecycle mapping for Bun/JSC and Wasmtime without adding product code. |
| Documentation ownership | pass | RSL1 owns the canonical contract. RSL3 owns the project skill and routing. |
| Comment policy | pass | Source comments explain stable invariants or removal triggers. Agent-only read-this comments are refused. |
| Cleanup task | pass | RSL99 removes stale plan routing after the final merge while it preserves the experiment archive. |

## Task Audit

The ledger and task bodies have matching IDs from RSL0 through RSL9, plus
RSL99. Each task has these fields:

- problem.
- owning seam and paths.
- ordered steps.
- falsifiable acceptance.
- fail-before evidence.
- exact verification commands or a command source that RRC8 must pin.

RSL7 and RSL8 form the only cross-repository sequence. RSL7 prepares the
unpublished candidate. RSL8 measures, decides, publishes, and repins only after
an accepted verdict. All other tasks target one pull request unless execution
finds a scope tripwire.

## Findings And Corrections

| ID | Finding | Correction | Status |
|---|---|---|---|
| DPA1 | The first framing could treat `f5f813366` as the old architecture. | RSF1 and RSL0 now separate the pre-PIR, PIR, cage-repair, user, and activation anchors. | closed |
| DPA2 | A runtime cleanup plan could conflict with active RRC8 fork edits. | The coordination and activation gates give RRC8 precedence. | closed |
| DPA3 | Product cleanup could destroy future comparison ability. | RSL2 and RSL5 require raw evidence, exact commits, patches, and detached experiment reproduction. | closed |
| DPA4 | Benchmark pool labels can misstate actual Web construction. | Invariant 3, RSL2, and RSL8 require requested policy and actual construction mode. | closed |
| DPA5 | A broad Deno carry deletion could remove the lazy-ESM termination fix. | RSF7 and RSL7 require an independent contract audit. | closed |
| DPA6 | Source comments could become stale agent instructions. | RSL3 limits comments to local invariants and removal triggers. The skill and index own agent routing. | closed |
| DPA7 | The first draft exceeded the plan size guide and had 34 strict prose diagnostics. | The reviewed plan stays within the about-500-line guide and has zero strict prose diagnostics. | closed |
| DPA8 | Task consolidation left one stale task reference. | The ledger, task headings, finding route, and goal now use RSL0 through RSL9 and RSL99. | closed |
| DPA9 | RSL7 published and repinned before RSL8 measured the cleanup. | RSL7 now prepares an unpublished candidate. RSL8 runs the controlled A/B before publication and repin. | closed |
| DPA10 | RSL6 referred to three existing runtime verifiers, but RSL4 named two. | RSL4 now also names `verify-node-full-substrate-realm.sh`. | closed |
| DPA11 | The first promotion gate mixed proposal review with post-RRC8 activation inputs. | The reviewed plan is proposed. Exact commits, U6 evidence, and owner approval remain activation inputs. | closed |
| DPA12 | The first reachability text omitted the serialize-only and report-only boundaries. | RSF3 now records public re-exports, live product branches, report surfaces, and the lack of a found external selector. | closed |
| DPA13 | U3 had no recorded decision about the realm-only carries. | U3 will omit them, pair the omission with Nimbus consumer cleanup, and complete the controlled A/B before U5. | closed |
| DPA14 | `RuntimePoolKind::StartupSnapshotCache` source documentation calls it the current default although `RuntimeLimits::default` selects `WarmPool`. | RSL4 now owns this source-documentation mismatch with the other strategy labels. | open for RSL4 |
| DPA15 | The goal referred to activation placeholders that did not exist. | The goal now contains explicit worktree and branch placeholders. | closed |
| DPA16 | RSL1 accepted a subjective cold-agent judgment instead of a reproducible result. | RSL1 now requires transition fixtures that reach both terminal verdicts and reject incomplete transitions. | closed |
| DPA17 | RSL2 used a Cargo benchmark test as its schema-test command. | RSL2 now requires a named schema-test filter and compiles the benchmark separately. | closed |

## Remaining Activation Inputs

The owner and RRC8 must supply these items before activation:

1. Which exact post-RRC8 Nimbus, Deno, and V8 commits become the execution
   baseline?
2. Does the owner approve activation after RRC8 U6 becomes terminal?
3. Does the owner accept the proposed project skill and narrow routing entry?

## Verification

| Command | Result |
|---|---|
| `git diff --quiet f5f813366..304a2e677 -- <four strategy paths>` | pass, exit 0 |
| strict technical-writing lint on the plan and audit | pass, 2 files and 0 diagnostics |
| no-index whitespace checks on both ignored files | pass, no diagnostics |
| `bash scripts/check-docs.sh` | pass, 109 pages link-clean |
| ledger and task ID comparison | pass after DPA8 correction |

The RSL verifier does not exist yet. This is intentional. RSL0 must add it red
against the post-RRC8 baseline before any implementation task.

## Final Assessment

The reviewed plan is coherent, source-grounded, and safe to list as proposed
after the active release plan. RRC8 keeps ownership through terminal U6 and the
owner still controls activation. The review changes no current product
performance claim because the replay-scaffolding cost remains unknown.
