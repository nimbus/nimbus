# NNC6.5f3 physical-machine admission barrier

Status: `complete; F3-01–F3-24 green; review cadence exhausted`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Frozen source: `nnc6.5f-compose-machine-caller-substitution-audit.md`
§ “NNC6.5f3 physical-machine barrier”. This proof executes that accepted
scope. It does not reopen the architecture or add another audit.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Item base | NNC6.5f2 item commit `40fac9aa9dd0ea36e7b89bf209d8935e7db4b269`. |
| Reconciled main | `8877eaff43a36d9606a1feaa0ab31d0377539d9d`; `0 behind / 149 ahead` after a clean fetch. |
| Current state | F3-01–F3-24 are green. The one full and one narrow GPT-5.6 Sol/xhigh/fast reviews are complete. All eight accepted P2 findings are corrected and proven. Review cadence is exhausted. |
| Dirty paths | The item commit candidate owns the plan, proof, routing update, compute authority/coordinator and admission guard, server Engine adapter/composition, confirmed-provider barrier and permit, machine stop authority, callers, tests, and source verifier. No unrelated path is dirty. |
| Last green | Full CLI `1007 passed / 4 ignored`; strict CLI Clippy; warning-denied CLI Rustdoc; restart successor `1/1`; NNCV035 physical-machine stage `1/1`; helper mutations `150/150`; aggregate mutations `552/552`; live architecture `35/36` with only four NNC6.5g-owned diagnostics; strict proof lint; docs `108`; site `17/17`; format, syntax, ShellCheck, Prettier, and diff checks. |
| Next action | Commit the exact NNC6.5f3 item, then resume NNC6.5g from the plan recovery header. |
| Blocker | None. |

## Frozen ownership

NNC6.5f3 owns:

- the compute-owned machine workload authority query, exhaustive decision,
  typed conflicts, and unforgeable stop authorization.
- one server Engine adapter that lists canonical workload-saga authority.
- one confirmed-publication child that owns the process-safe admission lock,
  durable stop barrier, and provider witnesses. It also owns conditional
  clear, retention, and exact machine-incarnation authentication.
- admission fencing around the initial and restart Engine CAS operations.
- barrier authentication before forwarded provision, restart, and
  publication request/effect boundaries.
- the one authorized machine-manager stop seam and its five real callers.
- the 14 frozen behavioral tests plus the source-derived NNCV035 contract.

It does not own Compose, guest phase semantics, compensation, or tenant
retirement. It does not own service naming, network policy, or provider effects
outside the existing machine owner. It cannot add provider-owned stop policy,
a machine desired-state registry, or a `nimbus-network` effect.

## Acceptance ledger

| ID | Verifiable success criterion | State |
| --- | --- | --- |
| F3-01 | Recovery records the clean NNC6.5f2 base, current-main divergence, exact dirty paths, expected-red evidence, and next command. | `done` |
| F3-02 | Compute defines one exhaustive `MachineWorkloadStopDecision` with exact empty-fence, active, unavailable, ambiguous, corrupt, stale, and crossed outcomes. | `done` |
| F3-03 | A server-owned Engine adapter lists complete canonical saga authority for the forwarded execution provider without using system projection, address, socket, or provider handle as workload identity. | `done` |
| F3-04 | One provider-owned durable barrier binds provider instance, forwarder generation, barrier epoch, state, format version, and checksum. | `done` |
| F3-05 | Barrier claim authenticates the exact machine incarnation and persists under the existing process-safe confirmed-publication lock before the Engine scan. | `done` |
| F3-06 | Provider-backed witnesses are exact, complete, generation-fenced, and read under the same lock; corrupt, stale, crossed, or unavailable evidence fails closed. | `done` |
| F3-07 | Initial desired-intent admission authenticates barrier absence and holds the provider lock through the Engine CAS result. | `done` |
| F3-08 | Restart admission authenticates barrier absence and holds the same provider lock through the Engine CAS result. | `done` |
| F3-09 | Compute claims the barrier before it lists Engine authority and joins canonical desire with exact provider witnesses. | `done` |
| F3-10 | Active authority clears only the same unchanged effect-free barrier under the provider lock, then returns typed `ActiveWorkloadTeardownRequired`. | `done` |
| F3-11 | Unavailable, ambiguous, corrupt, stale, crossed, partial, or failed outcomes retain the barrier and return typed failure before machine or network effects. | `done` |
| F3-12 | Forwarded provision, restart, and publication authenticate barrier absence before their first journal, reservation, Machine API, or provider-effect boundary. | `done` |
| F3-13 | Only compute can construct the exact stop authorization; the machine manager authenticates its machine generation before withdrawal or provider effects. | `done` |
| F3-14 | Direct CLI stop, server stop, restart, bootc restart, and OS-apply restart all reach the same authorization seam. A caller without canonical Engine/store authority fails closed. | `done` |
| F3-15 | Active or unresolved authority makes zero publication, SSH, listener, port, process, VMM, helper, machine-state, or network effect. | `done` |
| F3-16 | Thread and two-process contention prove exactly one legal ordering: desire CAS first and stop conflicts, or barrier first and desire fails before CAS. | `done` |
| F3-17 | Fresh-process cuts after barrier persistence and before each physical effect reopen fenced; failed or ambiguous stop never permits admission or reuse. | `done` |
| F3-18 | A later machine generation cannot overwrite an unresolved earlier barrier; observed projection and IP address never substitute for identity. | `done` |
| F3-19 | All 14 frozen physical-machine tests are substantive and pass with exact order, error-class, zero-effect, contention, and reopen assertions. | `done` |
| F3-20 | NNCV035 checks real body-local dataflow for both CAS paths, three provider admissions, five callers, policy/provider ownership, and barrier-before-effect order. Each assigned mutation fails closed. | `done` |
| F3-21 | Focused and full affected suites pass with exact counts, including compute, server, CLI, machine, workloads, and process harnesses. | `done` |
| F3-22 | Format, strict Clippy, Rustdoc, dependency/effect scans, proof lint, docs, site, and diff checks pass. `nimbus-network -> nimbus-core` remains its only workspace edge. | `done` |
| F3-23 | Exactly one GPT-5.6 Sol/xhigh/fast item review runs only after F3-01–F3-22 are green. An accepted executable correction permits one narrow review. | `done` |
| F3-24 | The exact product, tests, proof, routing, and ledger transition commit as one recoverable item. No push or PR occurs. | `done` |

## Fail-before evidence

- All 14 exact behavior names are absent from the source tree at `40fac9aa9`.
  No existing test can falsely satisfy F3-19.
- The live architecture verifier is exact `35/36`. NNCV035 alone is red, and
  its machine diagnostic states that guest or physical-machine teardown lacks
  exact phase and active-workload fences.
- Current `machine/manager/stop.rs` withdraws publications and SSH authority,
  then calls provider/process effects without a compute authorization.
- Current initial and restart admissions call `commit_loaded` without a
  provider admission guard.
- At the fail-before baseline, the planned provider concept child
  `machine/publication_authority/confirmed/stop_barrier.rs`, compute decision
  `machine_stop_authority.rs`, and server Engine adapter
  `workload_saga_store/machine_authority.rs` did not exist.

## Current implementation evidence

- F3-02 passes `7/7` focused compute tests. The policy accepts explicit
  complete, unavailable, ambiguous, and corrupt results. It validates exact
  providers, generations, digests, duplicates, and crossed evidence. Only the
  policy constructs the opaque stop authorization.
- F3-03 passes `2/2` real Engine adapter tests. The server scans both required
  `recoveryEligible` Boolean partitions through the existing composite index.
  It decodes the lookahead row on each bounded page. It filters active and
  successor intents only after strict decode. The server accepts the union only
  when Engine sequence is stable before and after the complete scan. The
  130-record proof uses three composite-index queries, zero full scans, and
  reproduces the same authority after Engine reopen.
- The shared compute admission-guard seam is object-safe and provider-neutral.
  Initial running intent and restart desire changes lock admission before the
  Engine CAS. They retain the opaque permit through the exact CAS result.
  Deterministic rejection and permit-lifetime suites pass `12/12` and `10/10`.
- The confirmed provider now has a strict v4 barrier envelope and retained
  epochs. It also has exact provider witnesses, a process-held desire permit,
  and stop transitions.
  The compute coordinator claims the barrier before its Engine query, and the
  server stop/restart callers use its opaque authorization. Compute passes
  `10/10`. CLI check/test compilation and manager stop cleanup `8/8` pass.
- Provision, publication, and restart authenticate barrier absence in the same
  confirmed mutation that stages their authority. Barrier rejection precedes
  provider journals and effects. Forwarded restart passes `5/5`. The full
  forwarded provision slice passes `31 passed / 1 ignored`.
- The 14 frozen behavior names each exist once and pass the substantive-test
  scanner. Compute authority is `15/15`. The authority-less standalone caller
  proof is `1/1`. The two-process Engine-CAS contention plus fresh-process
  crash-cut parents are `2/2`.
- Strict validation rejects truncated, unknown-version, checksum, and
  inner-digest corruption without rewriting evidence. A `StopMayExist` barrier
  cannot clear or admit a desire. A later generation cannot cross it or replay
  it as effect-free.
- NNCV035 now checks the real methods and body-local ordering. Its dedicated
  physical-machine stage is `1/1`, and all `150/150` aggregate helper mutations
  fail closed. NNC6.5g owns the four live aggregate diagnostics.
- Full affected suites pass. Workloads pass `221/221`. Compute passes `423`
  with one ignored test. Machine passes `42/42`. Server passes `635` with 33
  ignored tests. CLI passes `1007` with four ignored tests.
- Warning-denied affected Clippy and Rustdoc pass. Rust format, JavaScript and
  shell syntax, ShellCheck, Prettier, JSON parsing, and diff checks pass.
- The architecture verifier is exact `35/36`. NNCV001–NNCV034 pass.
  NNCV035 has only the four NNC6.5g-owned service, tenant, compensation, and
  aggregate-behavior diagnostics. Its physical-machine stage is green.
- Strict proof lint reports `24` unique ordered criteria with valid states and
  nonempty contracts. Docs pass `108` pages, and the site passes `17/17`.
- Current next action: create the exact item commit, then resume NNC6.5g.

## Full item review dispositions

The one full item review used GPT-5.6 Sol with xhigh reasoning and fast mode.
It found five P2 defects. The owner accepted and corrected all five:

| Finding | Disposition and proof |
| --- | --- |
| Production standalone stop rebuilt an Engine from default start settings. | Production fallback now has no inferred persistence config and rejects stop or restart before provider/network composition. The direct regression passes `1/1`. |
| Provider witnesses were filtered by the presented provider instance. | Every witness persists canonical machine identity; claims return all nonterminal witnesses for that machine, and compute classifies a crossed provider as `Crossed`. Regression `1/1`. |
| Restart-witness replay did not retain source execution. | The durable witness retains and validates the source execution. Exact replay passes and same-target crossed-source replay fails before effects. Regression `1/1`. |
| The process proof serialized contenders and the unit proof used a sleep. | Test-only semantic hooks report the first actual `WouldBlock`. Both process orderings and the unit lock-wait proof require that signal; no timing sleep remains. Regressions `2/2`. |
| NNCV035 accepted marker-only digest and admission bodies. | The contract now checks body-local digest bindings, barrier traversal, terminal selection, and exact provider/generation comparisons. Seven new mutations fail closed; aggregate is `146/146`. |

These executable corrections authorize exactly one narrow correction review.
The review cadence forbids another full review.

## Narrow correction review dispositions

The narrow review used the staged tree
`6cf518dd8fc10325e3f06996e67e304c516211d7`. Its binary patch SHA-256 was
`0e8176e1f004b0b28fcd0e88c14bc7bfca8bb4bf6d4c89d545f90a27c6920bb3`.
The wrapper confirmed GPT-5.6 Sol, xhigh reasoning, and fast service tier. It
reported three P2 defects at confidence `0.96`. The owner accepted and
corrected all three:

| Finding | Disposition and proof |
| --- | --- |
| A witness from the first restart could not advance to a second restart. | Restart authentication now requires every candidate source to equal the current execution, including when the current witness came from a restart. Exact first transition, replay, second successor, and crossed-source rejection pass `1/1`. |
| Digest fields could remain in an unused payload while a constant was hashed. | NNCV035 now traces the bound `DigestPayload` through serialization, SHA-256, the stable digest constructor, and the returned result. The disconnected-payload mutation fails closed. |
| Provider and generation comparisons could return success, and the matching barrier could fail open. | NNCV035 now requires each comparison branch and the final matching-barrier path to return an error. Three fail-open mutations fail closed. |

After correction, full CLI passes `1007` tests with four declared ignores.
Strict CLI Clippy and warning-denied Rustdoc pass. NNCV035 passes its physical
stage `1/1` and helper suite `150/150`. The aggregate mutation suite passes
`552/552`, and the live verifier remains the exact planned `35/36`. Only the
four diagnostics that NNC6.5g owns remain. The review cadence now forbids a
third review.

## Modularity dispositions

No changed handwritten file reaches 2,000 lines. The changed files in the
1,500–1,999 ownership band have these exact dispositions:

| File | Lines | Ownership reason |
| --- | ---: | --- |
| `crates/nimbus-cli/src/machine/publication_authority/confirmed.rs` | 1,968 | One confirmed-publication journal and transaction owner. NNC6.5f3 moves the complete stop-barrier state machine and process tests into its `confirmed/stop_barrier` child; the root keeps only transaction composition. It cannot grow again before a complete concept extraction. |
| `crates/nimbus-server/src/tests/service_manager/definition_retirement.rs` | 1,780 | Test-only owner for definition-retirement behavior. NNC6.5f3 adds only the required composition field and no production authority. |
| `crates/nimbus-cli/src/machine/backend/provision.rs` | 1,640 | Existing parent-host exact-phase adapter. Restart behavior stays in its child; this item adds only the shared admission guard and barrier authentication. The frozen extraction tripwire remains before 2,000 lines. |
| `crates/nimbus-cli/src/machine/backend/provision/tests.rs` | 1,634 | Test-only owner for forwarded provision admission and effect ordering. Restart and subprocess matrices remain in separate concept children. |
| `crates/nimbus-compute/src/state.rs` | 1,591 | Compute composition root. Machine-stop policy, admission behavior, and their tests are in concept-owned children; this root only wires the narrow capabilities. |

## Failure and recovery obligations

| Case | Required proof |
| --- | --- |
| Desire wins the provider lock | Its Engine CAS settles before lock release. Stop waits, persists the barrier, observes the desire, conditionally clears the unchanged effect-free barrier, and returns typed conflict. |
| Stop wins the provider lock | The barrier is durable before Engine scan. Later initial or restart admission rejects before its Engine CAS. |
| Desire process dies under the lock | The operating system releases the lock. A committed CAS is visible to stop. An uncommitted CAS created no desired authority. |
| Stop process dies after barrier persistence | A fresh process observes the same fenced barrier and cannot admit workload work. |
| Engine listing is unavailable or ambiguous | Retain the barrier and return typed authority-unavailable or ambiguous failure with zero effects. |
| Provider witness or barrier is corrupt, stale, or crossed | Retain evidence and fail closed. Do not infer absence. |
| Active authority exists while the machine appears stopped | Return the same typed conflict. Observed machine state cannot override durable workload authority. |
| Provider stop is failed or ambiguous | Retain barrier, publications, ports, witnesses, and all unresolved authority for reconciliation. |
| Later generation crosses the barrier | Reject before start, admission, publication, or physical stop effects. |

## Complexity controls

- Keep `confirmed.rs` as the deep provider-evidence owner. The barrier state
  machine and its tests are in the `stop_barrier` child. At 1,968 lines, the
  root cannot grow again before a complete concept-owned extraction.
- Keep Engine enumeration in the server adapter and policy in compute. The
  provider child cannot import or reproduce Engine desired-state rules.
- Keep `manager/stop.rs` effect-oriented. It accepts and authenticates the
  unforgeable authorization before its first withdrawal. It does not classify
  workload authority.
- Do not add a generic machine teardown provider, a compatibility path, or a
  marker-only function for the source verifier.

## Review cadence

Use focused tests during implementation. Run full affected gates only when
F3-01–F3-22 are candidate-green. Run one structured GPT-5.6 Sol/xhigh/fast
review only after F3-01–F3-22 pass. Run one narrow correction review only if an
accepted finding materially changes executable code. Do not review partial
chunks or rerun for proof wording, formatting, or ledger closeout.
