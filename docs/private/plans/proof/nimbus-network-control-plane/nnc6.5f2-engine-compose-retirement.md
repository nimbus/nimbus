# NNC6.5f2 Engine-backed Compose retirement

Status: `in_progress`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Frozen source: `nnc6.5f-compose-machine-caller-substitution-audit.md`
§ “NNC6.5f2 Compose cutover”. This proof narrows execution and records
evidence. It does not reopen the accepted architecture.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Reconciled main | `8877eaff43a36d9606a1feaa0ab31d0377539d9d` |
| Rebased NNC6.5f1 checkpoint | `596fb9fa71bdbfd1cda55f44338f91ccb429b025` |
| Divergence after rebase | `0 behind / 147 ahead` of `origin/main` |
| Current product state | No NNC6.5f2 product edit. All rebased baseline gates pass, so the recovery commit can proceed. |
| Next action | Commit the exact provenance and recovery checkpoint, then implement the frozen Compose retirement and coarse guest-stop deletion scope. |
| Blocker | None. |

The rebase retained current-main Cloud Functions trusted tenant binding and
Convex snapshot-bound silo authentication while replaying all owner commits.
The two NNC0.1 source-derived records now identify their exact rebased
equivalent commits. The rebase did not relax generated evidence.

## Frozen ownership

NNC6.5f2 owns:

- local and forwarded Compose-down activation through one Engine-backed
  workload store and the existing `ComputeResourceRetirer` facade.
- a durable terminal execution reference in the workloads `Recorded` evidence
  and the narrow compute retirement outcome that exposes it.
- a concept-owned Compose retirement module and its behavioral tests.
- removal of the coarse guest stop route, client, wire types, operation, and
  capability advertisement.
- the source-derived NNCV035 Compose and coarse-envelope conditions.

NNC6.5f2 does not own physical-machine stop or its admission barrier,
failed-provision compensation, tenant retirement, provider-effect ownership,
service naming, network policy, or any `nimbus-network` effect.

The durable terminal reference is evidence, not a new coordinator or store.
Workloads owns its encoding, compute authenticates and returns it, and Compose
renders it. The design forbids a CLI-local saga store.

## Acceptance ledger

| ID | Verifiable success criterion | State |
| --- | --- | --- |
| F2-01 | `run_compose_down` receives the same `EnginePersistenceConfig` as Compose up. | `todo` |
| F2-02 | Down opens one Engine and one `EngineWorkloadSagaStore` before compute retirement. | `todo` |
| F2-03 | The exact activated runtime supplies `ComputeResourceRetirer`; no second activation or store exists. | `todo` |
| F2-04 | Selected and all-service modes submit each stable tenant-qualified service identity exactly once in deterministic order. | `todo` |
| F2-05 | A durable `Recorded` result reports its exact terminal disposition and execution reference without a process-local sandbox handle. | `todo` |
| F2-06 | Unstarted or source-only retirement reports an exact no-execution outcome and never fabricates an address or execution identity. | `todo` |
| F2-07 | Same-process and fresh-process replay return stable durable truth with zero duplicate provider effect. | `todo` |
| F2-08 | Missing source, missing store, missing capability, and unresolved or ambiguous work return typed failure without fabricated success. | `todo` |
| F2-09 | Crossed tenant, source generation, execution, plan, provider, or forwarder evidence fails before provider effects. | `todo` |
| F2-10 | Cancellation and lost provider response retain the same attempt and use exact inspection before retry. | `todo` |
| F2-11 | Partial sibling failure preserves completed and unissued service authority and reports exact per-service results. | `todo` |
| F2-12 | Engine quiescence and provider/network lifetime settlement complete before Compose down returns. | `todo` |
| F2-13 | No production Compose path calls `SandboxBackend::stop` or the old stop target helper. | `todo` |
| F2-14 | The coarse guest stop route and route constant are absent. | `todo` |
| F2-15 | The coarse guest stop client method and response validator are absent. | `todo` |
| F2-16 | The coarse guest stop request, response, response wire, operation, and capability status are absent. | `todo` |
| F2-17 | Exact workload teardown phase transport remains the sole remote teardown-effect ingress. | `todo` |
| F2-18 | The nine frozen Compose behavior tests are substantive and pass with the required phase, CAS, replay, race, and fail-before assertions. | `todo` |
| F2-19 | Affected full suites, strict Clippy, format, dependency/effect scans, NNCV035 mutations, docs, and diff checks pass. | `todo` |
| F2-20 | Exactly one Sol/xhigh/fast item review runs after F2-01–F2-19 are green. An executable correction permits only one narrow review. | `todo` |

The nine test names stay exactly as frozen in the substitution audit. The
fresh-process test must reopen Engine and provider roots. It cannot receive
process-local state from its parent. The phase proof observes withdraw, drain,
stop, detach, release, then `Recorded`. It records at least five effect claims
and five confirmed results for an execution that owns all five resources.

## Rebased baseline evidence

| Proof | Result |
| --- | --- |
| Format | `cargo fmt --all --check` passes. |
| Convex silo-auth reconciliation | `5/5` focused tests pass. |
| Cloud Functions trusted-binding reconciliation | `1/1` focused test passes. |
| Server forwarded composition | `13/13` pass. |
| Full CLI | `996` pass. `3` are ignored. |
| Full server, serialized | `727` pass. `33` are ignored. |
| Full sandbox, serialized | `1,155` unit tests pass. `32` are ignored. `11` integration/binary tests pass. `16` environment-gated tests are ignored. |
| Sandbox contention follow-up | Three timing-sensitive failures from a parallel run each pass `1/1` in isolation before the serialized full suite passes. |
| Full node | `121/121` pass. |
| Strict affected Clippy | CLI, server, sandbox, and node pass with `-D warnings`. Vendored Brotli emits its pre-existing dependency warnings. |
| Live architecture contract | `35/36`. NNCV035 alone is expected red at exact `0/6`. |
| NNCV035 mutation helper | `138/138` pass. |
| Aggregate mutation verifier | `552/552` pass, including exact exclusive failure attribution for NNCV035. |

NNCV035 currently reports six expected-red domains. They are Compose, physical
machine, tenant retirement, compensation, behavior roster, and final
convergence. f2 owns only the Compose domain and deletes the coarse guest
envelope.
