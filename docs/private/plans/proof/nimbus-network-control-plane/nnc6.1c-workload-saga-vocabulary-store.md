# NNC6.1c Workload Saga Vocabulary And Store Port

Status: `complete; durable item commit is this checkpoint`

Starting checkpoint: `78ebdc3328ddb13fdb32f9f71da900144d118f1e`

NNC6.1c implements the portable workload saga vocabulary, state machine, store
port, and an uncomposed compute coordinator. It adds no Engine adapter,
production store, `ComputeState` injection, service lifecycle cutover, provider
effect, or compatibility path.

## Audit Result

The read-only substitution audit found one ordering constraint in the previous
task wording. A required production store cannot enter `ComputeState` before
the server-owned Engine adapter exists. Supplying a no-op or production
in-memory store would violate the frozen architecture.

The plan therefore uses these acceptance-bearing steps:

1. NNC6.1c defines and proves portable types, transitions, the store port, and
   the uncomposed compute coordinator.
2. NNC6.1c1 replaces the operational node identity and deletes the two false
   in-memory desired-state authorities.
3. NNC6.1d implements the server Engine adapter and injects that required
   adapter into workload-capable compute composition.
4. NNC6.1e routes lifecycle decisions through the injected coordinator.

This split occurs before implementation. It keeps every checkpoint compilable
without a temporary store, optional no-op, feature flag, or duplicate durable
authority.

## Current Census

The census is source-derived at the starting checkpoint.

| Evidence | Current result | NNC6.1c target |
| --- | --- | --- |
| `nimbus-workloads` direct workspace dependencies | `nimbus-core`, `nimbus-tenant` | Add only `nimbus-network`. |
| `nimbus-compute` direct `nimbus-workloads` dependency | absent | Add the direct edge. |
| Reverse dependencies of `nimbus-workloads` | 7 | Add only `nimbus-compute`; require the exact target set of 8 and an acyclic graph. |
| Portable saga/store modules | absent | Add `saga.rs` and `store.rs` with concept-owned test children. |
| Compute saga coordinator | absent | Add one `workload_saga.rs` owner that requires a store port. |
| Product saga-store implementation | 0 | Remain 0 until NNC6.1d. |
| Product saga coordinator construction | 0 | Remain 0 until NNC6.1d. |
| Old `DesiredWorkloadStore` implementation | 1 | Remain unchanged for NNC6.1c; NNC6.1c1 deletes it. |
| Old product in-memory authorities | 2 | Remain unchanged for NNC6.1c; NNC6.1c1 deletes both. |
| Old physical desired-state writes | 3 | Remain unchanged for NNC6.1c; NNC6.1c1 deletes them. |
| `TenantWorkloadId` source files | 7 | NNC6.1c1 performs the breaking cutover. |
| `TenantWorkloadGeneration` source files | 7 | NNC6.1c1 performs the breaking cutover. |
| NNCV026 mutations | 14 | Remove the obsolete early-dependency mutation, preserve 13 node-coordinator mutations, and add exact struct and non-struct saga-coordinator cardinality mutations: 15 total. |

The current old store has no recovery reader. NNC6.1c does not use it as a
test adapter for the new port and does not connect the new coordinator to it.

## Frozen Ownership

| Concern | Owner in this item |
| --- | --- |
| Portable identities, counters, digests, intent, phases, evidence, and transition validation | `nimbus-workloads::saga` |
| Object-safe asynchronous persistence port, CAS vocabulary, paging, and errors | `nimbus-workloads::store` |
| Cross-domain transition request orchestration without effects | `nimbus-compute::workload_saga` |
| Engine codec, schema, OCC adapter, ambiguity translation, and required production composition | NNC6.1d |
| Operational node identity and old desired-store deletion | NNC6.1c1 |
| Network state and observations | `nimbus-network` |
| Node, sandbox, service, and publication effects | Their current upper crates |

`nimbus-network` remains unchanged and retains only its core workspace edge.
`nimbus-workloads` imports portable network types, never a manager or effect.

## Module And API Contract

NNC6.1c owns these concept modules:

```text
crates/nimbus-workloads/src/saga.rs
crates/nimbus-workloads/src/saga/state.rs
crates/nimbus-workloads/src/saga/tests.rs
crates/nimbus-workloads/src/store.rs
crates/nimbus-workloads/src/store/tests.rs
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/tests.rs
```

Production files remain below 1,500 lines. Large exhaustive matrices live in
the matching private test child. No generic helper or utility module enters the
crate.

### Portable identities and wire values

`saga.rs` defines these values:

- `WorkloadSagaKey` from exact `TenantId` and `WorkloadId`.
- `WorkloadSagaId` under `nimbus.workloads.saga.id.v1`.
- `WorkloadExecutionId` under `nimbus.workloads.execution.id.v1` from
  `TenantWorkloadUid`, `NodeIdentity`, and `WorkloadGeneration`.
- `WorkloadGeneration` and `WorkloadSagaRevision` with explicit construction,
  `as_u64`, and `checked_next`.
- `WorkloadDesiredDigest`, `WorkloadOwnerEvidenceDigest`, and
  `WorkloadTerminalEvidenceDigest` as canonical lowercase SHA-256 values.
- `WorkloadSagaTransitionId` under
  `nimbus.workloads.saga.transition.v1`.

Stable IDs use length-delimited components. Addresses, ports, PIDs, manifests,
provider handles, and observed names cannot enter an identity derivation.

Every `u64` serializes as canonical unsigned decimal text. Decoding accepts
`0` or a non-zero digit followed by digits, rejects leading zeroes and
overflow, and round-trips `u64::MAX` exactly.

`NodeIdentity`, `TenantWorkloadUid`, and `TenantIsolationDecisionId` gain
validated deserialization needed by the portable record. They cannot gain an
unchecked string constructor.

### Intent and record

The implementation follows the complete format-version-1 record in the
NNC6.1b proof. `WorkloadSagaIntent` contains workload kind, desired state,
generation, and desired digest. It also contains the complete network tuple,
activation intent, publication intent, and admitted evidence.
`WorkloadSagaRecord` contains one active intent and at most one complete
successor. It also contains revision, phase, exact phase detail, the last
transition, and optional stable failure evidence.

The public API exposes validated constructors and transition methods. It does
not expose public fields, an unchecked phase setter, or a raw serde wire type.
Deserialization validates the complete record before returning it.

`WorkloadSagaTransitionId` hashes a canonical encoding of every semantic field
in the next transition, excluding only its own output slot. Field order,
collection order, and optional values are explicit. A changed intent, network
tuple, phase detail, evidence digest, revision, or failure value changes the
ID.

### Phase and evidence validation

The transition function implements the complete NNC6.1b matrices. Tests cover
every allowed edge and every required or forbidden evidence combination.

The important branch rules are:

- `Ready -> Published -> Observed` for `PublishWhenReady`.
- `Ready -> Observed` for `Withheld`, with publication evidence forbidden.
- `PrepareOnly` cannot enter `WorkloadActivated` in that generation.
- a higher generation before `Recorded` becomes one complete successor and
  forces withdrawal of the active generation.
- `Recorded` promotes a Running successor to `IntentCommitted` and records a
  Stopped successor without acquiring effects.
- `CleanupPending` contains a non-empty retained reference set and exactly one
  inspection requirement for each network, execution, or publication subject.

The implementation rejects missing, duplicate, extra, crossed, stale, or
out-of-order evidence. It also rejects backward transitions, partial successor
intent, equal-generation divergent content, lower generations, release before
terminal network evidence, and successor promotion with retained references.

### Store port

`store.rs` defines an object-safe `WorkloadSagaStore: Send + Sync + 'static`.
It returns boxed `Send` futures and adds no Tokio dependency.

The port exposes only:

```text
load(key)
compare_and_swap(expected, next)
list_recoverable(page_request)
```

Expected state is `Missing` or `Revision`. A commit returns `Applied` or
`Unchanged`. Errors are `Conflict`, `Ambiguous`, `Corrupt`, `Unavailable`, or
`InvalidTransition`. The port exposes no mutable store, delete, unconditional
upsert, whole-map restore, Engine value, provider effect, or internal adapter.

Recovery pages use a typed cursor ordered by recoverable phase then saga ID.
The limit is non-zero and at most `256`. Page construction rejects unsorted,
duplicate, terminal, over-limit, or cursor-regressing records. A returned next
cursor equals the final record when another page may exist.

Private tests implement at least two substitutable stores and run the same
port contract. The crate exports no in-memory implementation.

### Compute coordinator

`WorkloadSagaCoordinator` requires `Arc<dyn WorkloadSagaStore>` in its only
constructor. It exposes load, validated CAS transition, and bounded recovery
page methods. It does not expose the store or hold a network manager. It cannot
call a provider, retry a conflict blindly, write a system projection, or
execute a lifecycle effect.

The coordinator derives expected revision from the loaded record and delegates
all legal-edge validation to `nimbus-workloads`. It returns typed conflict and
ambiguity outcomes to the later lifecycle owner.

No production code constructs this coordinator in NNC6.1c. NNC6.1d owns the
first production construction with the durable server adapter.

## Prospective NNC6.1c1 Cutover

NNC6.1c1 is a separate acceptance-bearing item because its mechanical call-site
surface does not belong in the portable state-machine review unit.

It must:

1. Replace `TenantWorkloadGeneration` with `WorkloadGeneration` across
   workloads and node status enforcement.
2. Replace node-owned `TenantWorkloadId` with the generation-scoped
   `WorkloadExecutionId` across node, CLI Machine API, and the live test.
3. Delete `DesiredWorkloadStore`, `InMemoryDesiredWorkloadStore`,
   `DesiredWorkloadSnapshot`, `WorkloadController`, and their false restart
   test.
4. Delete all three `ServiceManager` desired-state writes, the state field,
   snapshot API, and assertions that treated memory as durable evidence.
5. Convert the CLI boot planner to a pure ordered desired-intent collection.
6. Leave server adapter and compute production injection absent for NNC6.1d.

The cutover breaks existing APIs. It adds no alias, deprecated name,
compatibility re-export, or feature flag.

## NNCV026 Adjustment

The existing node-coordinator verifier currently rejects the planned compute
dependency and every second name containing `Saga`. NNC6.1c changes it narrowly:

- remove only the `early-workloads-dependency` mutation and live rejection.
- allow exactly `WorkloadSagaCoordinator` beside `NodeWorkloadCoordinator`.
- keep `second-coordinator` injecting another node-workload coordinator and
  failing only NNCV026.
- preserve the other 13 node capability, bypass, restart, and authority
  mutations.
- add exclusive struct and enum mutations for exact
  `WorkloadSagaCoordinator` cardinality and ownership.
- continue forbidding workload, network, and projection authority inside the
  narrow `node_workloads.rs` adapter.

NNCV027 remains standalone in this item. Its implementation mode must improve
from seven failures to exactly three: old production in-memory authority,
missing server adapter, and lazy activation bypass. NNC6.1c1, NNC6.1d, and
NNC6.1e close those gaps in that order before registration.

## Owned Paths

NNC6.1c may edit only these paths. The owner must amend the ledger before
editing another path:

```text
Cargo.lock
crates/nimbus-tenant/src/decision.rs
crates/nimbus-workloads/Cargo.toml
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/desired.rs
crates/nimbus-workloads/src/tenant.rs
crates/nimbus-workloads/src/saga.rs
crates/nimbus-workloads/src/saga/state.rs
crates/nimbus-workloads/src/saga/tests.rs
crates/nimbus-workloads/src/store.rs
crates/nimbus-workloads/src/store/tests.rs
crates/nimbus-compute/Cargo.toml
crates/nimbus-compute/src/lib.rs
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/tests.rs
scripts/nimbus-network-control-plane/compute-node-workload-coordinator-contract.sh
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/verify-nimbus-network-source-contract.mjs
scripts/verify-nimbus-network-control-plane.sh
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1b-workload-saga-vocabulary-store-durable-home.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1c-workload-saga-vocabulary-store.md
```

NNC6.1c must not edit services, CLI, node, server, system, network source,
Engine, sandbox, proxy, provider, listener, or cluster paths.

## Fail-Before Contract

Before implementation:

1. The standalone implementation helper reports the original seven named
   gaps.
2. The new transition, store, and coordinator tests fail because their modules
   and exports do not exist.
3. Adding the compute dependency without the NNCV026 correction fails only
   NNCV026.
4. A synthetic second node coordinator still fails only NNCV026 after the
   correction.

The owner records exact fail-before output before replacing test scaffolding
with implementation.

## Acceptance Freeze Verification

The preimplementation checkpoint is exact:

| Gate | Result |
| --- | --- |
| Decision census | `7/1/2/3/54/0` |
| Decision contract | `1 passed, 0 failed` |
| Target implementation contract | Expected red: `0 passed, 7 failed`; all seven frozen diagnostics appeared. |
| Live control-plane verifier | `27 passed, 0 failed`; the new task and checkpoint rows remain one-to-one. |
| Technical-writing lint | `1` file passed with `0` diagnostics. |
| Rust format and diff hygiene | `cargo fmt --all --check` and `git diff --check` passed. |
| Docs | `108` pages passed. |
| Docs site | `17/17` conditions passed. |

No structured review ran. R15 permits that review only after R1-R14 are green
and the complete item is candidate-frozen.

## Implementation Verification

The portable implementation is complete inside the frozen owned paths. It adds
the two direct dependency edges, the validated saga/state-machine and store
vocabulary, and one deliberately uncomposed compute coordinator. It does not
construct a product store or coordinator, edit an effect owner, or cross into
NNC6.1c1 or NNC6.1d.

| Gate | Current result |
| --- | --- |
| Saga behavior | `31 passed, 0 failed, 0 ignored` |
| Store behavior and two-implementation conformance | `19 passed, 0 failed, 0 ignored` |
| Complete `nimbus-workloads` library | `66 passed, 0 failed, 0 ignored` |
| Compute coordinator behavior | `10 passed, 0 failed, 0 ignored` |
| Full affected libraries | `243 passed, 0 failed, 0 ignored`: compute `84`, tenant `93`, workloads `66` |
| Affected all-target/all-feature check | passed |
| Strict affected Clippy | passed with `-D warnings`; only inherited vendored Brotli build warnings were emitted |
| Warning-denied affected rustdoc | passed; only inherited vendored build warnings were emitted |
| Metadata/dependency proof | all three feature profiles are acyclic; `nimbus-network` has only `nimbus-core`; workloads has only core, network, tenant, and workspace-hack; compute directly depends on workloads; exact reverse-dependent set is `8` |
| NNCV026 | all `15` mutations failed exclusively at NNCV026; `15 passed, 0 failed`; final correction artifacts are in `/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T//nnc61c-nncv026-narrow-correction.V3BtAO` |
| Live control-plane verifier | `27 passed, 0 failed` |
| NNCV027 decision mode | census `8/1/2/3/54/0`; `1 passed, 0 failed` |
| NNCV027 implementation mode | expected red `0 passed, 3 failed`; only the NNC6.1c1 in-memory authority, NNC6.1d server adapter, and NNC6.1e lazy-activation gaps remain |
| Authority preservation | the four old CLI/services authority files retain their starting SHA-256 values; product saga-store implementations and coordinator constructions remain `0` |
| Effect/source scans | no Tokio or effect dependency in workloads; no provider, manager, projection, Engine, server, system, socket, or runtime effect in the new portable/coordinator modules |
| Script quality | changed Node and Bash syntax plus ShellCheck pass |
| Modularity | production modules are `1,386`, `1,143`, `226`, and `63` lines; exhaustive test children are `1,554`, `801`, and `330` lines |
| Executable/script digest | SHA-256 `087907f9669c4673343d2011c3caeab3bd9bcb3ba066eabc65855e640baaeac7` over the ordered owned source, manifest, and verifier paths |

## Structured Review Disposition

The sole full item review used GPT-5.6 Sol with `xhigh` reasoning and fast mode.
It returned eight findings. The owner accepted all eight after source review.

| Priority | Finding | Disposition and proof |
| --- | --- | --- |
| P1 | A `Recorded` saga with a queued successor was omitted from recovery. | Accepted. `recorded_successor_remains_recoverable_across_promotion_crash_window` failed before the fix and now passes. The recovery cursor now admits every record for which `requires_recovery` is true. |
| P2 | `CleanupPending` allowed successor replacement. | Accepted. Source tracing showed that the prior successor validator allowed this change. `validated_record_rejects_cleanup_successor_replacement` now exercises the validator directly and passes. |
| P2 | Entering `WithdrawalCommitted` or `CleanupPending` did not bind persisted detail to the transition source. | Accepted. `validated_record_binds_withdrawal_and_cleanup_details_to_source_phase` failed before the fix and now passes. The corrected entry-only rule also preserves valid same-phase successor replacement. |
| P2 | A provision-phase record could deserialize with a queued successor. | Accepted. `validated_record_rejects_successor_while_provision_remains_active` failed before the fix and now passes. |
| P2 | Standalone execution and publication reference deserialization bypassed intrinsic validation. | Accepted. `effect_reference_deserialization_enforces_intrinsic_invariants` failed before the fix and now passes. Execution IDs are rederived, and endpoint lists must remain nonempty, sorted, and unique. |
| P2 | NNCV026 checked exact saga-coordinator cardinality only in compute and CLI sources. | Accepted. NNCV026 now scans every production Rust source. Independent struct and enum mutations inject into `nimbus-server`, outside the previous scan, and all `15/15` mutations fail exclusively at NNCV026. |
| P3 | The verifier self-test total changed from `187` to `188` although one mutation replaced another. | Accepted. The full-review correction restored `187`. The later narrow-review fix added one independent enum mutation, so the final total is now legitimately `188`. |
| P3 | The owned-path ledger omitted the edited NNC6.1b durable-home proof. | Accepted. The durable-home proof is now an explicit owned path in this item. |

The full affected suites pass `243/243` after these corrections. Check, strict
Clippy, warning-denied rustdoc, dependency profiles, authority and effect scans,
format, diff, script quality, live verifier, and targeted mutation gates also
pass.

### Narrow correction review

The one permitted narrow review ran through GPT-5.6 Sol with `xhigh` reasoning
and fast mode. Thread `019fbe55-8a24-7280-9c11-70a8c31f6694` returned one P2
at confidence `0.87`: the all-production owner scan matched only struct items.
The owner accepted the finding. The prior matcher returns false for both an
enum and a `pub(crate)` type alias. The corrected matcher accepts struct, enum,
union, trait, and type declarations, including restricted visibility.

The new enum mutation and the retained struct mutation each fail exclusively
at NNCV026. The complete targeted set passes `15/15`. The live verifier passes
`27/27`. Script syntax and ShellCheck pass. The review artifact is
`/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T//nnc61c-narrow-review.nuyMzb`.
No third review ran under the item-level cadence.

The final item transition exposed one ledger-parser gap. NNCV008 recognized
letter-suffixed items but not the existing letter-plus-digit form
`NNC6.1c1`. The corrected item grammar recognizes that canonical ID, requires
its complete recovery fields, and passes in the final `27/27` live verifier.

The owner stopped one broad historical verifier diagnostic after an unrelated
NNCV005 exclusivity failure and slow NNCV006 mutations.
It is not an NNC6.1c acceptance gate and is not reported as green. The item's
written verifier obligations are the live `27/27` result and the targeted
NNCV026 `15/15` mutation contract above.

## Acceptance Ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| R1 | Metadata shows `workloads -> network` and `compute -> workloads`; network retains only its core edge; every graph profile remains acyclic. | green: compiler-resolved metadata reports `profiles=3`, `acyclic=3`, the exact core-only network edge, the planned workloads edge, and compute's direct workloads edge |
| R2 | Stable saga and execution IDs are domain-separated, length-delimited, deterministic, tenant-qualified, and independent of addresses or provider values. | green: deterministic/domain-separation/length-frame and malformed-identity behavior tests pass |
| R3 | Generations, revisions, and every nested counter round-trip `0`, `2^53`, and `u64::MAX` as canonical decimal text; malformed values fail. | green: boundary, malformed, lossy, and nested-counter wire tests pass |
| R4 | Every digest and transition ID rejects malformed text and changes for every semantic payload mutation while exact replay remains stable. | green: digest-decoder, semantic-payload mutation, exact-replay, and deserialization-revalidation tests pass |
| R5 | Active/successor generation rules cover direct terminal intent, in-flight replacement, exact replay, divergence, staleness, overflow, and promotion without retained effects. | green: all named active/successor behaviors and overflow checks pass |
| R6 | Exhaustive provision and teardown matrices prove every allowed edge plus missing, extra, duplicate, crossed, and forbidden reference or observation case. | green: exhaustive provision/teardown and evidence-integrity matrices pass |
| R7 | CleanupPending requires exact one-to-one inspection for every retained subject and cannot release or promote. | green: cleanup inspection/fencing matrix passes |
| R8 | The object-safe store exposes only load, CAS, and bounded recovery pages; two private implementations pass one conformance suite; no Tokio or exported in-memory store exists. | green: object-safety proof and both private store conformance suites pass; source/dependency scans are clean |
| R9 | Paging rejects zero or over-256 limits, terminal phases, duplicate or unsorted records, and cursor regression; valid pages are deterministic. | green: all `17` named paging/construction cases plus both store conformance paths pass |
| R10 | The compute coordinator requires a store, validates before CAS, preserves typed conflict and ambiguity, and has no provider, network-manager, system, Engine, or production-construction authority. | green: coordinator `10/10` and production source/effect/construction scans pass |
| R11 | NNCV026 accepts the planned dependency and enforces exactly one canonical saga coordinator while all 15 mutations fail exclusively. | green: live NNCV026 passes; all `15/15` isolated mutations fail exclusively, including independent struct and enum duplicate saga-coordinator cases |
| R12 | The old two in-memory authorities and three writes are byte-unchanged and explicitly deferred to NNC6.1c1; no new production saga store or coordinator construction exists. | green: exact starting hashes match; census remains `1/2/3`; new production implementation/construction counts remain `0/0` |
| R13 | NNCV027 implementation mode improves from seven failures to exactly the three assigned later gaps. | green: expected red is exactly `0 passed, 3 failed` with the three assigned diagnostics |
| R14 | Focused behavior, affected crate suites, check, strict Clippy, rustdoc, dependency/effect scans, format, diff, script quality, main verifier, and docs gates pass with exact counts. | green: behavior is `31/19/10`, affected libraries are `243/243`, live verifier is `27/27`, docs are `108`, site conditions are `17/17`, and every named quality, dependency, effect, script, format, diff, and writing gate passes |
| R15 | Exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast item review runs after R1-R14 are green, and every finding is dispositioned. | green: the sole full review's eight accepted findings and the sole narrow review's one accepted finding are corrected and proven; no third review ran |

## Recovery Checkpoint

The sole full and sole narrow GPT-5.6 Sol/xhigh/fast reviews are complete. All
nine accepted findings have fail-before or direct source evidence and corrected
proof. R1-R15 are green. This proof and its 23 owned paths form the durable
NNC6.1c item commit. Record the resolved commit hash before NNC6.1c1 edits.
There is no blocker.
