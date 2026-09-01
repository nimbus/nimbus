# Runtime Strategy Lifecycle Draft Audit

Date: 2026-08-30.
Independent re-review: 2026-09-01.
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
| Draft-baseline product type | pass | `RuntimePoolKind::WarmContextRecycle` is public and serialize-only at `304a2e677`. RRC8 U3 removes the value and its product branches in the paired candidate. |
| Current U3 realm omission | pass | The paired Nimbus source has no `WarmContextRecycle`, realm-lease, realm-lifecycle, or replay-table product symbol. The Deno candidate has no Nimbus replay or in-realm API; generic upstream realm infrastructure remains. |
| External selector search | pass with limit | Both facade crates re-export the type. Server and UI code report it. No CLI, package, server, or compute selector was found. RSL4 still requires a complete caller and serialization audit. |
| Construction truth | pass | `V8RuntimeConstructionMode::for_compatibility_target` selects `StartupSnapshot` for Node targets and `Unsnapshotted` for other V8 targets. |
| Web performance evidence | pass with qualification | PIR2 records 2.23 and 2.50 ratios against the row labelled `startup_snapshot_cache`. The label does not prove Web snapshot use. |
| Node performance evidence | pass | NFR6 records realm lease at 5.38 to 10.01 times startup snapshots and 13.35 to 16.13 times exact warm pools. |
| Replay source coupling | pass | Nimbus snapshot blobs include replay source tables. The Deno runtime stores and replays the same source classes. |
| Replay scaffolding cost | closed after review | RRC8's exact old-graph A/B found no detected Web change, a small favorable Node replay-off result after counterbalancing, and 16 encoded replay-table bytes in the Node22 blob. RSL8 owns permanent-lab reproduction. |
| Lazy-ESM termination | pass with independent audit required | Its error contract does not depend on realm pooling. RSL7 forbids automatic removal with realm-only carries. |
| Fork ledger drift | pass | The current ledger names Deno 2.9.1 while `Cargo.toml` names `v2.9.3-nimbus.2`. RRC8 already owns the replacement. |
| Uplift state | pass after re-review | RRC8 U1 through U6 are terminal for the runtime handoff. The immutable releases are Deno `625e4c259488dfa1c3c9d03fabde17758e1130d9` (`v2.9.6-nimbus.2`) and rusty_v8 `961a76d0cee88efdecfa9224c519fd153c404b51` (`v150.4.0-nimbus.1`). Nimbus records the substantive runtime graph at `76165b0b91da274ba0d70a2f4fbd6c7b81c1ee88`, the test checkpoint at `208c2e6f5b5507214ce5c8a75c1617ea9d0259c4`, and the Linux evidence checkpoint at `cb84dfec8ad61cabaf5b4d763e5c3ff8b9abbcf8`. |

## Control-Plane Audit

| Check | Result | Evidence |
|---|---|---|
| One outcome | pass | Product code keeps supported winners. The lab and archive preserve reproducible alternatives. |
| Active-plan precedence | pass | RRC8 keeps exclusive ownership of the Deno 2.9.6 and V8 150.4 uplift. |
| Safe plan state | pass | Status is `proposed`. Terminal U6 evidence is recorded. Owner approval of activation and the project skill remains required. |
| Source authority | pass | Current source and tests override historical labels. Historical measurements remain unchanged. |
| Conditional cleanup | pass | RSL4 must prove callers and carry ownership before RSL5 through RSL7 remove code. |
| Safety before speed | pass | Isolation, semantics, cancellation, memory, recovery, and crash gates precede scoring. |
| Decision method | pass | The plan uses workload-class Pareto winners instead of one global microbenchmark winner. |
| Product and lab split | pass | Product types contain supported choices. Lab metadata and exact patches preserve rejected choices. |
| Publication ownership | pass after correction | RRC8 U5 and U6 own the only fork publication and repin before activation. RSL7 audits the immutable result, and RSL8 reproduces its A/B without release writes. |
| Future backend use | pass | RSL3 tests backend-neutral lifecycle mapping for Bun/JSC and Wasmtime without adding product code. |
| Documentation ownership | pass | RSL1 owns the canonical contract. RSL3 owns the project skill and routing. |
| Comment policy | pass | Source comments explain stable invariants or removal triggers. Agent-only read-this comments are refused. |
| Cleanup task | pass | RSL99 removes stale plan routing after the final merge while it preserves the experiment archive. |

## Task Audit

The ledger and task bodies have matching IDs from RSL0 through RSL9, plus
RSL99. Each task has these fields:

- problem.
- owning boundary and paths.
- ordered steps.
- falsifiable acceptance.
- fail-before evidence.
- exact verification commands or a command source that RRC8 must pin.

The activation inputs are the only cross-repository sequence, and RRC8 owns
them. RSL7 checks immutable fork references without editing them. RSL8
reproduces the accepted A/B in the permanent lab. Each RSL task targets one
Nimbus pull request unless execution finds a scope tripwire.

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
| DPA9 | RSL7 published and repinned before RSL8 measured the cleanup. | The first correction delayed publication, but the post-U6 activation gate made even that workflow stale. DPA18 owns the final correction. | superseded |
| DPA10 | RSL6 referred to three runtime verifiers, but RSL4 named two. | U3 removed the NodeFull realm verifier and replaced the old profile monolith with a compact current-contract aggregator. RSL4 and RSL6 now use that entry point plus the crossover, execution-classification, and tenant-isolation successor gates. | closed |
| DPA11 | The first promotion gate mixed proposal review with post-RRC8 activation inputs. | The reviewed plan is proposed. Exact handoff commits and U6 evidence are now recorded. Owner approval remains an activation input. | closed |
| DPA12 | The first reachability text omitted the serialize-only and report-only boundaries. | RSF3 now records public re-exports, live product branches, report surfaces, and the lack of a found external selector. | closed |
| DPA13 | U3 had no recorded decision about the realm-only carries. | U3 omits them, pairs the omission with Nimbus consumer cleanup, and completed the controlled A/B before U5. | closed |
| DPA14 | `RuntimePoolKind::StartupSnapshotCache` source documentation calls it the current default although `RuntimeLimits::default` selects `WarmPool`. | RSL4 now owns this source-documentation mismatch with the other strategy labels. | open for RSL4 |
| DPA15 | The goal referred to activation placeholders that did not exist. | The goal now contains explicit worktree and branch placeholders. | closed |
| DPA16 | RSL1 accepted a subjective cold-agent judgment instead of a reproducible result. | RSL1 now requires transition fixtures that reach both terminal verdicts and reject incomplete transitions. | closed |
| DPA17 | RSL2 used a Cargo benchmark test as its schema-test command. | RSL2 now requires a named schema-test filter and compiles the benchmark separately. | closed |
| DPA18 | The plan activated after U6 but still scheduled a second Deno cleanup publication and repin in RSL7 and RSL8. | RSL7 now audits the immutable RRC8 graph and archive. RSL8 reproduces the A/B without fork edits or publication. | closed |
| DPA19 | Several fail-before criteria assumed realm product code still existed after RRC8 removed it. | RSL5 through RSL7 now use the exact archived pre-cleanup graph and mutation fixtures as fail-before evidence. | closed |
| DPA20 | The review workflow did not record the owner's temporary model-credit restriction. | Coordination and RSL9 require Sol-only review and prohibit Opus 5 and Fable until the owner lifts the restriction. | closed |
| DPA21 | The draft audit still described an in-progress U2 and U3 after RRC8 made the runtime handoff terminal. | The source audit and activation gate now record the immutable fork releases and the three Nimbus evidence checkpoints. | closed |

## Remaining Activation Inputs

RRC8 supplied the immutable handoff anchors. RSL0 will bind the exact execution
baseline from the current release branch at activation. The owner must still
supply these approvals:

1. Activate the plan.
2. Accept the proposed project skill and narrow routing entry.

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

The corrected plan is coherent, source-grounded, and safe to list as proposed
after the active release plan. RRC8 keeps ownership through terminal U6 and the
owner still controls activation. RRC8 U3 should omit the proven realm-only
carries now, not schedule a second fork cleanup. Its single-host A/B remains a
reproduction input, not a cross-host product performance claim.
