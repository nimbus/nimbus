# NNC9.4 Focused Behavior Evidence

Status: `complete`

Source checkpoint: `56e095b163c65111fc4452eb902c7bbdd6e1029d`
(`docs(network): publish landed control-plane architecture`), tree
`289beff554533536710e632d406e1971f2fe2347`.

## Unit Of Value

NNC9.4 creates one concise recovery index for the completed behavioral proof
set. It avoids historical suite replays because product inputs did not change.
The index records the current portable suite and the final affected suite. It
also records both proof environments, seed policy, declared skips, and one
durable owner proof for each behavioral-matrix row.

This item owns no product, manifest, package, or provider-effect path.

## Acceptance Contract

| ID | Verifiable criterion |
| --- | --- |
| K1 | All 28 rows in the Required Behavioral Proof Matrix appear exactly once below. |
| K2 | Every primary proof path exists in the current branch `HEAD`; no ignored or working-tree-only artifact is cited. |
| K3 | The current `nimbus-network --all-features` suite passes with one recorded fixed property-test seed and exact per-target output. |
| K4 | The final upper-crate behavior counts remain applicable because no product Rust, Cargo, crate, or package path changed after the NNC9.2 candidate. |
| K5 | Local Darwin and privileged Linux runner facts identify OS, architecture, kernel, Rust/Cargo, and source or binary identity. |
| K6 | Randomized work has an exact seed. Deterministic process, crash, model, and live lifecycle proofs explicitly record that they use no random seed. |
| K7 | Every declared ignored or unavailable lane has an exact count and reason. A child entrypoint is evidence only through its passing parent. |
| K8 | No missing capability, skipped lane, interrupted command, or expected-red baseline is reported as a pass. |
| K9 | The item changes evidence and concise ledger routing only. It does not reopen product behavior or authority. |
| K10 | A candidate-frozen Sol/xhigh/fast review, affected static/docs gates, proof/ledger transition, and exact item commit close the item. |

## Environments And Reproducibility

| Environment | Exact facts | Evidence use |
| --- | --- | --- |
| Owner workstation | `Darwin 24.6.0`, `arm64`, Rust `1.96.1 (31fca3adb 2026-06-26)`, Cargo `1.96.1 (356927216 2026-06-26)`, source checkpoint and tree above. | Current portable suite, proof-reference integrity, static and documentation gates. |
| Sovereign runner | `nimbus@192.168.4.29` (`minicloud`), Debian 13, `x86_64`, kernel `6.12.94+deb13-amd64`, Rust/Cargo `1.97.1`; authenticated Run 67 binary SHA-256 `fd1a9030d69fe93c6fc3bd48168c4091c2bc4a2ac43ff2df7351a25fa1699984`. | Privileged attachment, Krun, network-isolation, offline lifecycle, restart, withdrawal, cleanup, and sovereignty proof. |

The current property run fixes `PROPTEST_RNG_SEED=202608160904`. The property
configuration retains 512 cases. Process contention, named crash cuts,
state-machine tables, mutation checks, and the two Run 67 attempts are
deterministic and use no random seed. Run 67 authenticates every input and uses
two disjoint durable roots. It is not a random sampling campaign.

## Current Focused Output

Command:

```text
PROPTEST_RNG_SEED=202608160904 \
  cargo test -p nimbus-network --all-features -- --test-threads=1
```

Exact output summary:

| Target | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Unit tests | 248 | 0 | 0 |
| `attachment_authority_process` | 1 | 0 | 1 |
| `capability_registry` | 3 | 0 | 0 |
| `capability_satisfaction` | 3 | 0 | 0 |
| `local_network_manager` | 2 | 0 | 0 |
| `port_conflict_model` | 6 | 0 | 0 |
| `readiness_dependency` | 13 | 0 | 0 |
| Doc tests | 0 | 0 | 0 |
| **Total** | **276** | **0** | **1** |

The sole ignored test is
`fresh_process_child_reopens_exact_attachment_segment_association`. It is a
subprocess entrypoint. The passing parent
`fresh_process_reopens_exact_attachment_segment_association` executes it and
authenticates its result.

## Final Affected Output

NNC9.2 is the final executable candidate. The exact diff from its checkpoint
`bcb0a1b06758d78155787aaf98a2cc502e02c039` through the NNC9.3 checkpoint has
zero Rust, Cargo, crate, or package paths. The committed NNC9.2 results remain
the final affected behavior evidence:

| Package | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| `nimbus-workloads` | 231 | 0 | 0 |
| `nimbus-compute` | 506 | 0 | 1 |
| `nimbus-sandbox` | 1,212 | 0 | 47 |
| `nimbus-services` | 102 | 0 | 0 |
| serialized `nimbus-server` | 757 | 0 | 35 |
| `nimbus-cli` | 1,011 | 0 | 4 |

Run 67 then passed K1-K14 twice. Each attempt recorded 19 transitions and one
restart. Each attempt also recorded zero unexpected DNS observations, zero
denied-output attempts, and empty forbidden-address evidence. Terminal
provider and projection state was exact, and cleanup was complete. Both
attempts exited `0`. Neither used the `SKIPPED` exit `77`.

## Declared Skip Ledger

The current `cargo test ... -- --ignored --list` inventory matches every
affected-suite ignored count:

| Owner | Count | Exact lane disposition |
| --- | ---: | --- |
| `nimbus-network` | 1 | One subprocess child; the current passing parent executes it. |
| `nimbus-compute` | 1 | `workload_network_plan` compiler child; its parent owns execution and result validation. |
| `nimbus-cli` | 4 | Guest teardown, forwarded two-realm recovery, external port-owner, and physical stop-barrier subprocess children. Their named parents are the evidence owners. |
| `nimbus-sandbox` library | 31 | 29 named crash/recovery subprocess children plus two opt-in scale/preview baselines. Parents execute the child roles. The two baselines have separate committed focused proof and are not counted as current execution. |
| `nimbus-sandbox` Linux integrations | 16 | Two Container egress, six Krun egress/isolation, and eight Krun smoke/provider cases require explicit privileged Linux/KVM admission. NNC9.2 runs the required real Krun lifecycle on the admitted minicloud runner. An unavailable local Darwin lane is not a pass. |
| `nimbus-server` | 35 | 15 external Node-version canaries, six generated-history model campaigns, two runtime-owner subprocess cases, two transport-liveness campaigns, and ten saga subprocess children. The primary proofs link required parent and focused results. This item does not claim the nightly or external campaigns as current passes. |
| `nimbus-workloads`, `nimbus-services` | 0 | No ignored tests in the final affected suites. |

The inventory command lists tests only. It does not execute an ignored child
or capability lane, and it is not behavioral pass evidence.

## Required Behavioral Proof Matrix

| # | Area | Primary durable proof | Exact recorded result |
| ---: | --- | --- | --- |
| 1 | Dependency architecture | `nnc9.1-static-verifier-closure.md` | Scanner `14/14`, compiler helper `18/18`, six dependency profiles, zero cycles, live verifier `39/39`. |
| 2 | Stable identity | `nnc1.2-stable-network-identities.md` | Four 512-case properties cover eight ID domains and two fencing types; current portable suite is `276 + 1` declared child. |
| 3 | Endpoint migration | `nnc1.3-endpoint-vocabulary-migration.md` | `578` affected-library plus `165` focused CLI/server tests; one owner and no compatibility re-export. |
| 4 | Segment allocation | `nnc2.7-multi-tenant-invariants.md` | Network `63`, sandbox `269 + 10` declared ignores, helper `2`, real selected KVM cases `2`, verifier self-test `15/15`. |
| 5 | Allocator substitution | `nnc2.2-portable-allocator-contract.md` | Network `61`, sandbox `252 + 13`, substitution helper `2`; Container and Krun use the portable attachment-ID trait. |
| 6 | Port concurrency | `nnc3.1-atomic-port-lease-lifecycle.md` | Network `69`; real process parent `1 + 1` child; thread and process contenders yield one winner. |
| 7 | Bind semantics | `nnc3.2-port-conflict-model.md` | Unit `73`, conflict matrix `6`, process set `3 + 1` child; TCP/UDP, family, realm, range, provider-assigned, and external collision are explicit. |
| 8 | Admission separation | `nnc3.4-sandbox-pep-machine-port-migration.md` | Named acceptance `3/3`; quota failure, internal PEP attribution, losing admission, and zero-effect rejection remain in the affected `968`-test closeout. |
| 9 | Capability selection | `nnc4.1-capability-dimensions-satisfaction.md` | Capability `21/21`, plan `7/7`, integration `3/3`; no implicit fallback. |
| 10 | Provision order | `nnc6.4-atomic-provision-caller-cutover.md` | E1-E35, NNCV033 `40/40 + 50/50`, aggregate mutations `327/327`; exact phase CAS through ready/publish/observe. |
| 11 | Activation safety | `nnc6.4-atomic-provision-caller-cutover.md` | Same E1-E35 observer proves inert prepare, same-generation attach, activation prerequisites, activate, workload readiness, then publish. |
| 12 | Teardown order | `nnc6.5g-final-teardown-convergence.md` | G1-G28, teardown helper `172/172`, direct/native/physical `1/1` each, aggregate `556/556`, live verifier `36/36`. |
| 13 | Attachment integrity | `nnc5.3-complete-attachment-readiness.md` | Focused `30/30`, sandbox `859 + 21`, portable `235 + 1`; netns-only and every partial prerequisite fail closed. |
| 14 | Effect-before-record | `nnc5.2a-durable-association-effect-ordering.md` | Shared lifecycle `41/41`, provider cleanup `30/30`, Netavark `12 + 3` children, actual backends `332/332`, sandbox `785 + 24`. |
| 15 | Egress boundary | `nnc4.5-egress-readiness-dependency.md` | Readiness dependency `13/13`, full network `198`; current PEP evidence is mandatory and PDP/PEP effects remain outside network. |
| 16 | TLS boundary | `nnc7.6-tls-telemetry-boundary.md` | T1-T4, M1-M5, A1-A10; server TLS `6`, proxy `164`, static mutations `15/15`. |
| 17 | Service ownership | `nnc7.2-service-endpoint-generation.md` | G1-G8; services `101`, network `275 + 1`, stale/crossed generations fail before mutation. |
| 18 | Projection | `nnc7.5-projection-independence.md` | Focused `35/35`, system `84`, compute `473 + 1`, server `753 + 35`; repair is effect-free and authority-independent. |
| 19 | Projection vocabulary | `nnc7.4-connectivity-projections.md` | Connectivity `7/7`, machine `1/1`, drift `3/3`, system `78`; HTTP and connectivity route identities cannot collide. |
| 20 | Listener group | `nnc7.1a-structured-listener-group.md` | F1-F8, focused `42/42`, server `752 + 35`; partial activation unwinds and inherited sockets remain external. |
| 21 | Durability | `nnc8.1-persisted-phase-recovery.md` | Attachment process matrix `40`, network decisions `66`, workload decisions `30`, affected mutations `4/4`, NNCV020 `9/9`. |
| 22 | Proof harness | `nnc8.1-persisted-phase-recovery.md` | One shared bounded semantic protocol serves five consumers; wrong checkpoint, early exit, timeout, cleanup, and real fresh-process roles are proven. |
| 23 | State-root contract | `nnc2.1-crash-safe-local-state.md` | Network `59`, process `2 + 1` child, sandbox `249 + 13`, testing `62 + 2`; known network roots fail before authority opens. |
| 24 | Recovery | `nnc8.6-failure-contract-closure.md` | All `22/22` failure rows map to current deterministic tests or static unreachability proof. |
| 25 | Orphan recovery | `nnc8.3-orphan-resource-convergence.md` | K1-K20, focused `9 + 1` child, NNCV018 `17/17`, mutations `588/588`, multi-tenant `16/16`. |
| 26 | Restart authority | `nnc8.4-stale-generation-restart-eligibility.md` | K1-K14, focused `145`, compute `477 + 1`, NNCV034 `86/86`; ten provider-observation fences reject stale execution. |
| 27 | Cluster handoff | `nnc2.8-horizontal-scaling-seam-truth-up.md` | Cluster tests consume a fenced committed super-net lease without transport; NNC2.4/NNC2.6 prove expiry rejects create while retaining old-handle cleanup; routed-not-overlay is unchanged. |
| 28 | Sovereignty | `nnc9.2-offline-sovereign-lifecycle.md` | Run 67 passes K1-K14 twice; lifecycle mutations `8/8`, invariants `10/10`, durability `24/24`, zero named network counters, empty DNS/forbidden-address evidence. |

The primary-path integrity check reports:

```text
behavior proof references: 28/28 present in HEAD
```

Several rows have supporting proofs in adjacent item files. The table names
one primary owner so authority is not duplicated.

## Candidate Gate Ledger

| Gate | Candidate-frozen result |
| --- | --- |
| Current portable behavior | Fixed-seed `nimbus-network --all-features`: `276` passed, `0` failed, `1` parent-owned child ignored. |
| Matrix and proof integrity | `28/28` rows match the canonical plan in order. Every primary proof is present in `HEAD`. |
| Executable-input identity | Zero Rust, Cargo, crate, or package paths changed after the NNC9.2 checkpoint. |
| Architecture verifier | Post-correction final: `39/39` conditions pass. |
| Documentation | Post-correction final: `108` pages pass, and the site passes `17/17` conditions. |
| Static quality | Post-correction final: technical-writing lint reports zero diagnostics. Rustfmt, diff checks, plan `991`, and index `237` limits pass. |
| Item review | One GPT-5.6 Sol/xhigh/fast review ran after the gates above. It accepted one P2 inconsistent-state defect and one P3 invalid displayed Git range at overall confidence `0.98`. Both corrections are documentation-only, so no narrow executable correction review is required. |

## Scope And Next Action

```text
git diff --name-only \
  bcb0a1b06758d78155787aaf98a2cc502e02c039..HEAD -- \
  '*.rs' 'Cargo.toml' 'Cargo.lock' crates packages
# zero paths
```

NNC9.4 therefore reuses the exact final affected output. Repeated execution
would not add confidence because the executable inputs did not change.

The candidate gates, one item review, and post-correction checks are complete.
The commit containing this proof is the exact NNC9.4 checkpoint. NNC9.5 is
next. This item does not push or open a PR.
