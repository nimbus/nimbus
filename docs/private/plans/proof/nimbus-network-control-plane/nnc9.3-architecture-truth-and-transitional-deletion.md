# NNC9.3 Architecture Truth And Transitional Deletion

Status: `complete`

Starting checkpoint:
`bcb0a1b06758d78155787aaf98a2cc502e02c039`.

## Unit of value

NNC9.3 makes the repository describe the network control plane that is now
landed. It regenerates exact source/compiler/dependency evidence, removes stale
planning chronology from active routing surfaces, and publishes a durable
architecture reference. It changes no product behavior or authority.

## Acceptance contract

| ID | Verifiable criterion |
| --- | --- |
| K1 | Generated normal, development, all-feature, macOS, Linux, and Windows dependency profiles are current, acyclic, and prove `nimbus-network -> nimbus-core` is the only outgoing workspace edge. |
| K2 | The bind/allocation inventory exactly matches current source. Test-only owner-exit listeners are excluded only through mechanically proven `#[cfg(test)]` module ownership. The libkrun private TSI formatter is classified as serialization, not allocation. |
| K3 | The production composition census exactly matches current source and retains one classification, OS-node realm, and evidence record per occurrence. |
| K4 | The sandbox effect census records all eight current production process-launch sites in their exact allowed owners. No effect moves into `nimbus-network`. |
| K5 | Every 1,500–1,999-line owner checked by NNCV021 has an exact current line count and concept-owned disposition. No touched handwritten source reaches 2,000 lines. |
| K6 | The compiler baseline authenticates current inputs, inventory, toolchain/configuration, target matrix, MIR call counts, parsed boundaries, and generated Rust. One refresh/deep collection and the cheap live check reproduce it. |
| K7 | Top-level and private architecture docs state the landed product model, owners, dependency rule, state separation, lifecycle order, sovereignty rules, and cluster separation without a stale target/future claim. |
| K8 | The active plan index is routing-only and at most 260 lines. The canonical plan has a short recovery header, no run chronology or duplicated evidence record, and is at most 1,000 lines. |
| K9 | Shared plans link the canonical network seam without a second allocator, effect owner, workload coordinator, service-name owner, projection authority, or cluster transport owner. Missing design/plan files are not presented as current authority. |
| K10 | The aggregate verifier passes every condition; focused helper tests, syntax/format/diff checks, docs gates, and proof lint pass with exact results. |
| K11 | One GPT-5.6 Sol/xhigh/fast item review runs only after K1-K10 are green. A narrow review runs only for an accepted executable defect. |
| K12 | The proof, generated evidence, architecture truth, routing compression, ledger transition, and exact item commit are one checkpoint. No push or PR occurs. |

## Fail-before evidence

The first aggregate run passed `33/39`. It had the five expected stale-source
failures—NNCV006, NNCV015, NNCV021, NNCV022, and NNCV038—plus NNCV008 because
the NNC9.3 ledger transition lacked its owned-path/last-green fields and the
NNC9.2 checkpoint hash. This was a recovery-text defect, not a product defect.

The source deltas are bounded:

- one test-only `workload_ingress/owner_exit_tests.rs` module added two loopback
  listener fixtures;
- libkrun private TSI port-map formatting moved from `bundle.rs` to
  `ingress.rs`;
- four composition coordinates and several bind/risk coordinates moved;
- the already-authorized `nimbus-guest-user-switch` binary is the eighth
  sandbox process-launch site; and
- NNC9.2 changed compiler inputs, so its authenticated baseline is stale.

## Architecture disposition

- Keep the portable allocator contract in `nimbus-network`.
- Keep `SingleNodeSegmentAllocator`, `ClusterSegmentAllocator`, Netavark,
  namespaces, IPAM realization, nftables, gvproxy, and guest-network effects in
  `nimbus-sandbox`.
- Keep future membership, node identity, mesh, routing, forwarding, and raft
  super-net lease sourcing in the deferred horizontal-scaling lane.
- Keep compute as the sole workload saga coordinator, services as logical-name
  and readiness owner, tenant/egress/proxy as policy and enforcement owners,
  and system records as projections.

## Verification ledger

| Gate | Result |
| --- | --- |
| Dependency profiles | Current `bcb0a1b0…` baseline: 266 declared workspace edges; six normal/dev/all-feature/target profiles; zero cycles. The network crate has one outgoing workspace edge. |
| Bind census | `67` authority occurrences, `37` classified risks, `7` production TCP binds, `0` UDP binds, `5` local-IPC occurrences, and `0` unclassified sites. Baseline passes. Removing the owner-exit cfg-test proof exposes both listener binds and fails. |
| Composition census | `114` exact occurrences: `25` owning managers, `40` manager-derived handles, `24` admitted reconstructions, and `25` test fixtures. Baseline passes. |
| Modularity | NNCV021 passes with the current 1,589-line port-lifecycle disposition and four named concept children. |
| Effect locality | NNCV022 passes with eight exact production process-launch sites. An injected ninth launch fails with expected `8`, found `9`. |
| Compiler baseline | Local Rust `1.96.1` refresh/deep collection passes: `1,661` authenticated inputs, six packages, seven production targets, `16` resolved calls, `103,339,693` MIR bytes, 17 macros, 37 classified risks, and five generated outputs. The cheap check passes. |
| Aggregate verifier | Pass: `39/39`. |
| Docs and static quality | Final correction rerun: docs pass `108`; site passes `17/17`; Node/Bash/JSON syntax, Rustfmt, diff checks, and plan/index limits pass. The plan is `991` lines and index is `237`. |
| Item review | One GPT-5.6 Sol/xhigh/fast review ran after candidate-green acceptance (thread `01a0096e-529e-7142-8e42-18271a17497d`). It accepted one P2: the compressed lifecycle omitted the attach-before-activation boundary. The architecture summaries now say `prepare inert -> attach -> activation-ready -> activate -> workload-ready -> publish`, and explicitly prohibit tenant instruction execution before authenticated same-generation attachment and required policy-enforcement readiness. The correction changes documentation only, so the review cadence does not require a narrow correction review. |

The broad historical `--self-test` replay was stopped after its first 15
unchanged fail-closed cases because each case reran the full aggregate and the
suite would duplicate NNC9.1 evidence for hours. It is not an NNC9.3 acceptance
gate. The two guards changed by this item have exact affected negative
mutations above. No result from the interrupted run is claimed as a pass.

## Recovery

Owned changes are limited to generated dependency/bind/composition/compiler
evidence, exact verifier ownership counts, network architecture/routing docs,
the canonical plan/index, shared-plan truth-ups, and this proof. Product Rust
is unchanged. The one item review and affected final gates are complete. The
commit containing this proof is the exact NNC9.3 checkpoint; NNC9.4 is next.
