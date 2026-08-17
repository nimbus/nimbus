# NNC6.5f2 Engine-backed Compose retirement

Status: `complete`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Frozen source: `nnc6.5f-compose-machine-caller-substitution-audit.md`
§ “NNC6.5f2 Compose cutover”. This proof narrows execution and records
evidence. It does not reopen the accepted architecture.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Reconciled main | `8877eaff43a36d9606a1feaa0ab31d0377539d9d` |
| Rebased NNC6.5f1 checkpoint | `596fb9fa71bdbfd1cda55f44338f91ccb429b025` |
| Divergence after rebase | `0 behind / 148 ahead` of `origin/main` after the reconciliation checkpoint. |
| Current product state | F2-01–F2-20 are green. The durable terminal-execution carrier and compute outcome are implemented. Compose down opens the canonical Engine/store/runtime and calls `ComputeResourceRetirer`. The direct Compose stop path and coarse guest route/client/wire/capability envelope are deleted. The frozen behavior set passes `9/9`; all affected suites, quality checks, source contracts, mutation tests, docs gates, and the allowed review cadence pass. |
| Current dirty paths | All 50 dirty paths are attributed to NNC6.5f2: workloads/compute carrier, Compose cutover, machine coarse-envelope deletion, behavioral tests, the directly required Engine schema-idempotency correction, exact verifier census/contract reconciliation, and plan/proof/routing state. No forbidden owner is dirty. |
| Next action | Commit the exact item with its plan-ledger transition, then load NNC6.5f3 read-only acceptance. |
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
| F2-01 | `run_compose_down` receives the same `EnginePersistenceConfig` as Compose up. | `done` |
| F2-02 | Down opens one Engine and one `EngineWorkloadSagaStore` before compute retirement. | `done` |
| F2-03 | The exact activated runtime supplies `ComputeResourceRetirer`; no second activation or store exists. | `done` |
| F2-04 | Selected and all-service modes submit each stable tenant-qualified service identity exactly once in deterministic order. | `done` |
| F2-05 | A durable `Recorded` result reports its exact terminal disposition and execution reference without a process-local sandbox handle. | `done` |
| F2-06 | Unstarted or source-only retirement reports an exact no-execution outcome and never fabricates an address or execution identity. | `done` |
| F2-07 | Same-process and fresh-process replay return stable durable truth with zero duplicate provider effect. | `done` |
| F2-08 | Missing source, missing store, missing capability, and unresolved or ambiguous work return typed failure without fabricated success. | `done` |
| F2-09 | Crossed tenant, source generation, execution, plan, provider, or forwarder evidence fails before provider effects. | `done` |
| F2-10 | Cancellation and lost provider response retain the same attempt and use exact inspection before retry. | `done` |
| F2-11 | Partial sibling failure preserves completed and unissued service authority and reports exact per-service results. | `done` |
| F2-12 | Engine quiescence and provider/network lifetime settlement complete before Compose down returns. | `done` |
| F2-13 | No production Compose path calls `SandboxBackend::stop` or the old stop target helper. | `done` |
| F2-14 | The coarse guest stop route and route constant are absent. | `done` |
| F2-15 | The coarse guest stop client method and response validator are absent. | `done` |
| F2-16 | The coarse guest stop request, response, response wire, operation, and capability status are absent. | `done` |
| F2-17 | Exact workload teardown phase transport remains the sole remote teardown-effect ingress. | `done` |
| F2-18 | The nine frozen Compose behavior tests are substantive and pass with the required phase, CAS, replay, race, and fail-before assertions. | `done` |
| F2-19 | Affected full suites, strict Clippy, format, dependency/effect scans, NNCV035 mutations, docs, and diff checks pass. | `done` |
| F2-20 | Exactly one Sol/xhigh/fast item review runs after F2-01–F2-19 are green. An executable correction permits only one narrow review. | `done` |

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

## Implementation evidence

| Check | Result |
| --- | --- |
| F2-05 fail-before | `teardown_happy_path_orders_withdraw_drain_stop_detach_release_record` fails `0/1`: `phaseDetail.value.terminalExecution` is `null` instead of the stopped execution reference. |
| F2-05 workloads correction | Exact teardown record `1/1` and stopped-successor promotion `1/1` pass; terminal execution survives promotion into replayable `Recorded` truth. |
| F2-05/F2-06 compute outcome | Resource-retirement lifecycle `3/3` passes: exact completed execution is returned after projection cleanup, and an unstarted source reports `SourceFinalized` with no execution, handle, or provider call. |
| F2-05/F2-06 strict wire | Workloads strict wire/reopen cases pass `2/2`: the terminal reference survives pre/post-promotion reopen, explicit `null` remains distinct from a missing required field, and missing evidence fails closed. |
| Compose/coarse-envelope compile | `cargo check --locked -p nimbus-machine -p nimbus-cli` passes after Engine-backed Compose activation and deletion of the direct guest stop envelope. Vendored Brotli warnings are pre-existing. |
| F2-18 frozen behavior set | `cargo test --locked -p nimbus-cli compose::retirement::tests::` passes `9/9`. The tests use the real Engine store, runtime composition, compute retirer, exact five teardown capabilities, provider-owned durable effect markers, and deterministic fault seams. The process case reopens Engine, network, and provider roots after an abort; the cancellation case proves retained completion and replay; ambiguity uses same-attempt Inspect; partial sibling failure preserves unissued source authority. |
| Full-CLI concurrency fail-before | Before correction, the full CLI lane repeatedly failed two to four frozen cases at their initial saga CAS. Temporary diagnostics proved the Engine conflict was `table schema changed during transaction: _workload_sagas (epoch 1 -> 2)`, while the exact record remained missing. |
| Engine schema idempotency correction | `concurrent_identical_schema_declarations_append_one_durable_record` fails before correction because 16 equal concurrent declarations append 16 schema records. The corrected serialized committer rechecks exact schema identity, returns a no-change result without a false observer notification, and the test passes with one durable record. |
| Corrected Compose/full CLI | Frozen Compose retirement passes `9/9`. Full `nimbus-cli` passes `995/995`; three subprocess-only tests are declared ignored. The temporary diagnostic is absent from the candidate. |
| Full affected suites | Workloads passes `221/221`. Compute passes `404/404` with one declared child-only ignore. Machine passes `42` unit plus `5` integration tests. CLI passes `995/995` with three declared subprocess-only ignores. The fixture-disabled serialized Engine suite passes `667/667` with five declared external-provider ignores in `225.33s`. |
| Engine gate configuration | An initial default-feature diagnostic run compiled unconfigured external MySQL/Postgres provider tests and reported `596` pass, `71` fail, and `5` ignored; it is not accepted as the embedded gate. `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` plus `--test-threads=1` supplies the repository's ordinary provider-aware contract and passes as recorded above. |
| Strict affected Clippy | `cargo clippy --locked -p nimbus-engine -p nimbus-workloads -p nimbus-compute -p nimbus-machine -p nimbus-cli --all-targets -- -D warnings` exits `0`. Only unchanged vendored Brotli warnings remain outside the warning-denied Nimbus crates. |
| Dependency and effect contracts | Live NNCV004 and NNCV012 pass: `nimbus-network -> nimbus-core` remains its only workspace edge, and no forbidden transport, provider, policy, naming, or cluster effect enters the crate. The complete live architecture verifier is exact `35/36`; NNCV035 is its only red condition. |
| Source-derived verifier reconciliation | NNCV006, NNCV008, NNCV015, NNCV021, and NNCV024 pass after exact line/count/rationale refresh and deletion of the obsolete assertion that direct Compose cleanup observes sandbox inspection. NNCV024 still detects all `19/19` prohibited inspection/effect mutations. |
| NNCV035 and aggregate mutations | Direct NNCV035 is exact expected-red `0/5`, with only service, physical machine, tenant, compensation, and final behavior domains left to later owners. The pre-review complete aggregate passes `552/552`, including exclusive NNCV035 failure attribution. The additive accepted-review guard increases the focused helper to `139/139`; its direct expected-red control remains exact `0/5`. |
| Format, diff, and modularity | `cargo fmt --all --check` and `git diff --check` exit `0`. New Compose production/test owners are `247` and `1,410` lines. Changed Engine schema/tests owners are `477` and `1,445` lines. No changed handwritten source reaches the ownership-exception threshold. |
| Docs | `scripts/check-docs.sh` passes `108` pages. `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

## Item review and correction evidence

| Review | Result |
| --- | --- |
| Full item input | Staged tree `5da50eb195a673e1e0427d5a5fa0f58af7ca1e88`; binary patch SHA-256 `221221d9d964ff9f5b752b23cbf5004bd32093f18e487ac24f3797c5ed2f59f4`; thread `019ff408-cef0-7a22-a705-8558b4d33f99`; bundle `253,437` bytes. The Nimbus wrapper used GPT-5.6 Sol, xhigh reasoning, fast service tier, one pass, and a clean secret scan. |
| Full item findings | Two findings are accepted. The P2 finding showed that Compose batch context converted source and setup failures to `Internal`; the correction preserves the typed `nimbus::Error` class and metadata while adding service context. The P3 finding showed that the source contract accepted any activation token; the correction requires the exact prepared runtime owner. |
| Full correction proof | Public conversion tests prove missing exact teardown remains `NotFound`, ambiguous stopped-successor state remains `Internal`, and partial sibling failure retains the exact completed/unissued context. The source contract compacts whitespace and requires `prepared.activate(` in the accepted order. A first combined proof used a non-exported crate-root `ComputeError` import; the compile failure was not accepted despite a later shell command returning zero. The import was corrected to `nimbus_compute::state::ComputeError`, and the fail-fast proof then passed. |
| Narrow correction input | Staged tree `73c5d02adcab8d5990512210a61295dd1d0a6a9c`; binary patch SHA-256 `041840ed93ad5e7c46af42415afa6570b0dcccdea211ad996de7cb1108d5068f`; thread `019ff413-f47a-7b82-b8aa-7c729e1aac61`; bundle `257,395` bytes. The wrapper used GPT-5.6 Sol, xhigh reasoning, fast service tier, one pass, and a clean secret scan. |
| Narrow findings | Two P2 findings are accepted. The Compose source contract did not prove that the `Recorded` arm returned its durable result, and two error tests inspected a private wrapper instead of the public conversion boundary. The correction checks exact result dataflow, adds the `compose-recorded-result-discarded` mutation, and asserts the public `nimbus::Error` variants and batch context. |
| Final correction proof | Frozen Compose passes `9/9`; NNCV035 passes `139/139`; the direct control exits one with exact `0/5`; strict CLI all-target Clippy exits zero with only unchanged vendored Brotli warnings; format, JavaScript syntax, shell syntax, and diff checks pass. Final executable/static-proof pre-ledger identity is staged tree `51d7ef91a69f85a5307ca3e54c3b077afa709b56`, binary patch SHA-256 `00e75dfdb9a18de08780f8cd6a6f5747eda08859736888000bb0670d18329f72`, across `49` paths. |
| Cadence | The complete item review and its one authorized narrow correction review are fully dispositioned. Review cadence is exhausted. No third review is authorized or needed. |

NNCV035 now reports five expected-red domains. They are service, physical
machine, tenant retirement, compensation, and final behavior convergence.
NNC6.5f2 made the Compose domain green and deleted the coarse guest envelope.
