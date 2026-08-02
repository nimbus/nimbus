# NNC6.2a Durable Compiled Network Plan

Status: `complete; exact item commit next`

Starting checkpoint: `15544998c20410fec30d89eca187cdc8d6527609`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md` NNC6.2a

NNC6.2a makes the complete workloads-owned `CompiledWorkloadNetworkPlan`
durable in each workload saga generation. It does not add a store, coordinator,
network command, provider adapter, or lifecycle ingress. The server-owned
`EngineWorkloadSagaStore` remains the only durable adapter, and its existing
Engine execution-unit mutation path remains unchanged.

This proof freezes the breaking v2 carrier, strict physical codec, exact
fresh-process reconstruction, and source allowlist before product edits.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Owner worktree | `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit` |
| Owner branch | `codex/nimbus-network-architecture-audit` |
| Starting HEAD | `15544998c20410fec30d89eca187cdc8d6527609` |
| Last completed item | `NNC6.2`, item commit `0977c17d93f3b39f18b33d504193c6eee6e9ba50`; routing commit `15544998c20410fec30d89eca187cdc8d6527609` |
| Current item | `NNC6.2a` |
| Current product state | One strict saga-v2 carrier retains the complete compiled plan, the physical store owns one required `compiledNetworkPlan`, phase evidence retains only derived tuples, and pure recovery returns the exact plan before any reservation effect. |
| Current dirty state | The durable audit checkpoint is `6869dc578`; 25 allowlisted implementation/static/proof paths are dirty before final routing-ledger closeout. No manifest, provider/effect, or `nimbus-network` path changed. |
| Last green | Historical and review reds are corrected; focused process proof `3/3`; affected behavior `844/844` with 29 declared skips; check, strict Clippy, warning-denied rustdoc, format/diff, Bash/ShellCheck; docs 108 and site `17/17`; NNCV029 `23/23` plus `8/8`; aggregate live `30/30`; complete mutation arithmetic `188 + 10 + 7 + 8 = 213`. |
| Next action | Commit the exact 25-path NNC6.2a item, then record its durable hash and route to NNC6.1e1 without another review. |
| Structured review | The one full item review ran as GPT-5.6 Sol/xhigh/fast in thread `019fc276-0ac0-7642-ad9b-f67e213e786d`. It accepted two P2 findings at overall confidence `0.95`: duplicate JSON keys bypassed strict saga-v2 wire rejection, and the pinned predecessor proof could pass when its commit was unavailable. The sole narrow Sol/xhigh/fast correction review confirmed both implementations and accepted one P2 test-only gap at confidence `0.96`: the matrix did not directly repeat a retained-content field. A duplicate `formatVersion` case now closes that proof at focused `1/1` and workloads `96/96`; no executable code changed and no third review is warranted. |
| Candidate identity | Historical 24-path pre-review identity: staged tree `eab90273367fc257b32896e798d8d8ba2c5e56df`, complete staged patch SHA-256 `6c981aa5c6dc247095b5606b27a74321f3a9f35b279b8d2d20c1b721a1ef47a4`, executable/static-proof SHA-256 `b8671625f7786638a0c00ab3c82c774fa4c7fdf13aa65064bc0771d74b82cdc1`. Corrected pre-narrow tree `ad8732e3c89d7f96221dd2636668e8d72a474725`, patch SHA-256 `39b64d749907c03017cf48e20a7b8043f756f45022becf93cac4c76286ba3538`, and executable/static-proof SHA-256 `9599c5ef1fac2e5088bf0288e93b28207105874f29bd32bd1437ee716d738b83`. Final post-narrow pre-ledger tree `40a81f6abbd4b05a182cc7c9562d7a416eb2a202`, patch SHA-256 `31e7d7f12b49922860a96955cbc6bd4590fe09ff90ab9d9c8de62806bee9fb9b`, and executable/static-proof SHA-256 `b98590aacfa16931a808511d504726c1485a663a781a322aa33c7702cafe3993`. |
| Blocker | None. |

## Audit Verdict

The current tuple is an authenticated selector, not durable desired state.
`NetworkPlanDigest` cannot reconstruct retained identity, resource blueprints,
capability and sovereignty requirements, provider selection, readiness
requirements, activation, or publication. A crash after intent commit and
before reservation therefore loses the command material that the future
compute ingress must use.

The existing seams otherwise have the right ownership:

1. `nimbus-workloads::CompiledWorkloadNetworkPlan` already owns a strict,
   versioned, provider-neutral envelope and exact retained content.
2. Its decoder rederives the content digest, plan identity, generation,
   sovereignty, complete capability requirements, readiness requirements, and
   every stable resource identity.
3. `WorkloadSagaRecord` already binds complete active and successor intents
   into the transition identity.
4. `EngineWorkloadSagaStore` already performs one snapshot read and one
   whole-record compare-and-swap through `MutationExecutionUnit`.
5. Embedded storage already applies the document, maintained indexes, and
   journal entry in one transaction.
6. Workload-saga recovery already has a bounded distinct-process Engine-root
   harness and a pure exhaustive action selector.
7. No production network command is dispatched from recovery today. The first
   command-shaped value is the effect-free `ReserveNetwork` action.

NNC6.2a therefore expands one portable value and one strict physical encoding.
It does not invent a new authority.

## Current And Target Data Flow

Current, lossy flow:

```text
CompiledWorkloadNetworkPlan
        |
        | discard retained content
        v
WorkloadNetworkIntent { plan_id, generation, digest }
        |
        v
physical networkPlanId/networkGeneration/networkPlanDigest
        |
        v
fresh process can select a plan but cannot reconstruct it
```

Target, lossless flow:

```text
CompiledWorkloadNetworkPlan
        |
        v
WorkloadNetworkIntent(complete compiled plan)
        |
        +--> WorkloadNetworkReference { derived tuple only }
        |
        v
physical compiledNetworkPlan object
        |
        v
strict saga v2 decode and complete-plan revalidation
        |
        v
pure ReserveNetwork { derived reference, exact compiled plan }
```

The compiled plan occurs once in each active or successor intent. Network and
publication phase evidence stores only the derived plan tuple. Provider
handles, observations, lease epochs, and effect state do not enter the saga
intent.

## Binding Decisions

### D1 — One complete portable carrier

`WorkloadNetworkIntent` owns exactly one `CompiledWorkloadNetworkPlan`. It has
no caller-supplied parallel plan ID, generation, or digest fields. Its
`plan_id`, `generation`, and `digest` accessors derive from the validated plan
envelope.

The carrier is transparent on the portable wire: the saga `network` field is
the strict `{ plan, content }` compiled value. A tuple-only, digest-only,
plan-only, content-only, partial, or unknown-field value fails decoding.

### D2 — Derived references are not desired-state copies

`WorkloadNetworkReference` becomes the direct three-field derived tuple. It is
constructed only from a complete saga intent. `WorkloadPublicationReference`
contains that small reference rather than cloning `WorkloadNetworkIntent`.

Every phase-detail validation rederives the expected tuple from the active
compiled plan. A stale or crossed reference fails. No phase evidence contains
compiled resource bytes.

### D3 — Saga v2 is a clean break

`WORKLOAD_SAGA_FORMAT_VERSION` becomes `2`. The transition identity domain
becomes `nimbus.workloads.saga.transition.v2` because its authenticated payload
shape changes. Saga v1 and unknown future versions fail closed. There is no v1
decoder, compatibility field, optional compiled carrier, or migration shim.

`WORKLOAD_NETWORK_PLAN_FORMAT_VERSION` remains `1`; NNC6.2a does not redefine
the compiled-plan format.

Nimbus is pre-launch. An old local `_workload_sagas` schema/root is rejected as
corrupt and must be explicitly reset by its operator. The adapter never
silently rewrites or guesses old desired state.

### D4 — Correlations are exact and local

`WorkloadSagaIntent::validate` requires:

- workload desired generation equals compiled network generation;
- top-level activation equals compiled content activation;
- top-level publication equals compiled content publication; and
- a stopped intent remains prepare-only and withheld.

`WorkloadSagaRecord::validate` requires both active and successor compiled-plan
tenants to equal the tenant-qualified saga key. The compiled-plan decoder
continues to reject crossed envelope/content identity, generation, digest,
sovereignty, capability, readiness, and resource IDs.

The current portable types cannot rederive the compiler's source-specific
workload-incarnation key from logical `WorkloadId`. NNC6.2a does not invent a
second identity mapping. NNC6.1e1 must pass the compiler result produced from
the same admitted decision into the saga constructor; equal-generation
replacement and the complete transition identity then fence divergence.

### D5 — One physical compiled object

The private `_nimbus._workload_sagas` document replaces
`networkPlanId`, `networkGeneration`, and `networkPlanDigest` with one required
`compiledNetworkPlan` object. No index queries the old tuple, so retaining
denormalized physical projections would add no capability and would create
another correlation surface.

The four existing indexes stay unchanged. The physical record has 19 total
schema fields: 17 required and two optional. Top-level activation and
publication remain query-free projections in the record, but strict portable
validation authenticates them against the compiled content on every decode.

The codec copies the complete active `network` value into
`compiledNetworkPlan` and reconstructs the portable active intent from that
one object. `successorIntent` already stores the complete portable successor.
Decode always routes through the workloads-owned strict decoders; the server
does not implement a second plan decoder.

### D6 — Existing Engine durability remains the only store

NNC6.2a does not change the `WorkloadSagaStore` port or add an implementation.
The server adapter continues to:

1. validate the complete candidate record;
2. open one `MutationExecutionUnit` with the system principal;
3. read the point document in that snapshot;
4. apply missing or exact-update-time preconditions;
5. stage one whole-record write; and
6. commit once.

Ambiguous commit handling remains fresh-read-before-retry. No store result
causes a network command in this item.

### D7 — The first pure action carries exact command material

At `IntentCommitted`, `WorkloadSagaDecision::for_record` first validates the
complete record, then creates:

```text
ReserveNetwork {
  reference: derived plan tuple,
  plan: exact CompiledWorkloadNetworkPlan
}
```

This is a pure value. It gives NNC6.1e1 complete, already validated command
material without a second lookup or reconstruction. It does not call
`LocalNetworkManager`, a lease authority, provider, sandbox, socket, or
forwarder.

### D8 — Bounded distinct-process proof, no snapshot handoff

The proof reuses `nimbus-testing::SubprocessCrashCutHarness` from a
concept-owned server-store test child.

The writer child:

1. opens only the supplied Engine state root;
2. builds one fixed populated compiled-plan fixture;
3. persists one running saga at `IntentCommitted` and verifies `Applied`;
4. emits only a bounded semantic fingerprint and process ID; and
5. reaches `workload-saga.compiled-plan-durable`, where the parent kills and
   reaps it.

The recovery child:

1. receives only the harness role/root plus a fixed mode selector;
2. opens a fresh Engine and `EngineWorkloadSagaStore`;
3. loads the fixed saga key without receiving record or plan bytes;
4. validates `IntentCommitted` and derives the pure reservation action;
5. compares the exact compiled value, serialized envelope bytes, canonical
   content bytes, plan/content digests, provider selection, requirements,
   readiness, and resource collections with an independently constructed
   fixed fixture; and
6. returns one bounded semantic observation.

Neither child imports or constructs a network manager, lease authority,
provider adapter, command sink, or sandbox effect. This is the zero-command
proof by capability absence. A test-only no-op recorder is not accepted as
effect evidence. NNC6.1e1 owns the first real command dispatch proof.

The parent requires distinct writer/recovery PIDs, exact boundary kill/reap,
successful recovery exit/reap, and the pinned observation. No plan bytes,
record JSON, digest, action, snapshot, expected token, stdin payload, or
sidecar file may cross into the recovery child.

## Explicit Non-Goals

NNC6.2a does not:

- change the NNC6.2 compiler or its source identity derivation;
- route service, sandbox, lazy-activation, startup, or retirement requests;
- execute reserve, attach, publish, withdraw, detach, release, or inspection;
- construct a network manager, provider registry, lease authority, or effect
  adapter;
- change the store port, add a store implementation, or add a mutation path;
- move provider handles or observations into desired saga state;
- add compatibility decoding, schema migration, a feature flag, or a fallback
  tuple;
- change cluster transport, egress PDP/PEP, service naming, certificates,
  proxy forwarding, sandbox attachment, or system projection ownership; or
- add a workspace dependency edge.

## Frozen Source Allowlist

Product edits are limited to these paths:

```text
crates/nimbus-workloads/src/network_plan.rs
crates/nimbus-workloads/src/saga.rs
crates/nimbus-workloads/src/saga/network.rs
crates/nimbus-workloads/src/saga/network/tests.rs
crates/nimbus-workloads/src/saga/state.rs
crates/nimbus-workloads/src/saga/tests.rs
crates/nimbus-workloads/src/store/tests.rs
crates/nimbus-compute/src/workload_saga/recovery.rs
crates/nimbus-compute/src/workload_saga/recovery/tests.rs
crates/nimbus-compute/src/workload_saga/tests.rs
crates/nimbus-server/src/workload_saga_store/codec.rs
crates/nimbus-server/src/workload_saga_store/schema.rs
crates/nimbus-server/src/workload_saga_store/tests/mod.rs
crates/nimbus-server/src/workload_saga_store/tests/codec.rs
crates/nimbus-server/src/workload_saga_store/tests/durability.rs
crates/nimbus-server/src/workload_saga_store/tests/recovery.rs
crates/nimbus-server/src/workload_saga_store/tests/tenant_enumeration.rs
crates/nimbus-server/src/workload_saga_store/tests/compiled_plan_durability.rs
crates/nimbus-server/src/workload_saga_store/tests/composition.rs
```

Static proof, evidence, and routing edits are limited to:

```text
scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh
scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
scripts/verify-nimbus-network-control-plane.sh
docs/private/plans/proof/nimbus-network-control-plane/nnc6.2a-durable-compiled-network-plan.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
```

Post-implementation aggregate verification exposed one required predecessor
handoff in `workload-network-plan-compiler-contract.sh`: its NNC6.2
candidate-time `git diff HEAD` check necessarily mistakes the authorized
NNC6.2a `saga.rs` edit for an NNC6.2 scope escape. NNC6.2a may replace only
that transient dirty-worktree assertion with a durable check of NNC6.2's exact
item commit (`0977c17d93f3b39f18b33d504193c6eee6e9ba50`). The predecessor
contract must retain its ban on direct compiled-payload ownership in
`saga.rs`; NNCV029 remains the sole live owner of the complete carrier and
NNC6.2a source allowlist. No other NNCV028 behavior or mutation may change.

No `Cargo.toml`, `Cargo.lock`, `nimbus-network`, `nimbus-tenant`,
`nimbus-sandbox`, `nimbus-services`, `nimbus-system`, `nimbus-proxy`, or
provider-effect path is allowlisted. A newly discovered required path pauses
implementation until this proof records the source evidence and owner reason.

The full item review supplied the required source evidence for adding
`crates/nimbus-workloads/src/network_plan.rs`: saga-v2 had to decode through a
typed, duplicate-detecting decimal-generation wire instead of first collapsing
JSON into `serde_json::Value`. The wire belongs beside the existing private
plan/content/blueprint wire types and reconstructs the same validated
`CompiledWorkloadNetworkPlan`; it does not change NNC6.2's compiler or content
contract. This is the sole post-freeze product-path expansion.

The first full server acceptance run produced `637/638` and an exact stale
observation mismatch in NNC6.1e's existing distinct-process phase matrix:
recovered saga-v2/complete-plan truth deterministically emitted
`matrix-30-9bdcd50077c9fa5db309f2c22610fda321c0062f66459cd9f41e122cb5abb80c`
while the test still pinned the saga-v1/tuple digest. Therefore
`tests/composition.rs` is allowlisted only to replace that one observation
constant. The writer/recovery roles, 30-case matrix, state-root-only handoff,
boundary, timeout, PID, and kill/reap assertions must remain byte-unchanged.

## Fail-Before Packet

The following cases freeze before product edits. Existing green behavior is
named honestly and is not counted as a historical red.

| ID | Proof | Required pre-implementation result |
| --- | --- | --- |
| F1 | Serialize the current tuple and require complete plan/content/resource bytes. | Fails because only plan ID, generation, and digest exist. |
| F2 | Construct `WorkloadNetworkIntent` from one compiled plan and retrieve exact bytes. | Fails because the carrier API does not exist. |
| F3 | Construct a saga with crossed workload/network generation. | Incorrectly succeeds today. |
| F4 | Construct a saga whose activation or publication differs from compiled content. | Cannot be expressed because the compiled value is absent. |
| F5 | Decode tuple-only, digest-only, plan-only, content-only, and truncated compiled shapes through saga v2. | Saga v2 and the compiled carrier do not exist. |
| F6 | Decode crossed tenant, envelope/content, requirements, readiness, and phase-reference values. | Complete carrier/correlation coverage does not exist. |
| F7 | Persist a populated `IntentCommitted`, kill the writer, and reconstruct exact bytes in a fresh process from only Engine durability. | Impossible with the tuple-only record. |
| F8 | Require the pure reservation action to carry exact compiled command material. | Fails because it carries only the tuple reference. |
| F9 | Require the physical schema to contain one compiled object and no flattened tuple fields. | Fails on the current 21-field schema. |
| F10 | Require the complete payload to occur once per active/successor intent and never in phase references. | Cannot pass until full embedding and reference narrowing land together. |
| F11 | Require saga v1 and future versions to reject under the v2 decoder. | v1 is current and therefore still accepted. |
| F12 | Require transition identity v2 to change for every compiled semantic mutation. | v2 domain and complete payload are absent. |

Existing strict green baselines that must remain green:

- digest-only tuple shapes already reject under the current tuple decoder;
- unknown current saga versions already reject;
- the standalone compiled-plan decoder already rejects partial, unknown-field,
  unknown-version, crossed envelope/content, capability, sovereignty,
  readiness, and resource identity values;
- `IntentCommitted` currently has no effect references;
- store CAS, ambiguity, atomic document/index/journal, paging, and distinct
  process recovery suites are green.

NNCV029 must fail the live repository only for the missing NNC6.2a seams and
must self-test every protected seam with named mutations. Missing source input
is a verifier failure.

## Written Acceptance Criteria

| ID | Verifiable success criterion |
| --- | --- |
| A1 | One `WorkloadNetworkIntent` owns one strict `CompiledWorkloadNetworkPlan`; no parallel tuple authority or optional fallback remains. |
| A2 | Plan ID, network generation, and digest accessors derive exactly from the compiled envelope. |
| A3 | Workload generation, activation, publication, and saga tenant cross-correlations reject with stable typed saga errors. |
| A4 | Saga format v2 and transition identity v2 reject v1/future records without a compatibility decoder. |
| A5 | Tuple-only, digest-only, plan-only, content-only, partial, null, unknown-field, and unknown inner-plan version values fail strict decode. |
| A6 | Crossed envelope/content digest, plan identity, generation, sovereignty, capability requirements, readiness, resource identity, record tenant, and phase references fail strict decode or record validation. |
| A7 | Active and successor transition identities bind every compiled semantic field; equal-generation content divergence remains a conflict. |
| A8 | Network and publication phase references contain only one derived tuple and validate exactly against the active compiled plan. |
| A9 | The private physical record stores one required `compiledNetworkPlan`, no flattened network tuple, and the same four indexes. Exact schema has 17 required plus two optional fields. |
| A10 | Codec round-trip preserves complete record bytes, compiled envelope bytes, canonical content bytes, both digests, selection, requirements, readiness, and every resource collection. |
| A11 | Store CAS winner, exact replay, contention, ambiguity fresh-read, paging, and atomic document/index/journal tests remain green with the complete payload. |
| A12 | A bounded crash-cut kills and reaps the writer after `IntentCommitted`; a distinct recovery process reopening only Engine durability reconstructs the exact populated plan and returns the pinned semantic observation. |
| A13 | Process argv/environment/stdin/sidecar scans prove no snapshot or expected payload reaches recovery; both child roles have bounded output and cleanup evidence. |
| A14 | The pure `ReserveNetwork` action carries the exact compiled plan plus its derived reference only after full record validation. All store/decode/validation errors yield no action and no command. |
| A15 | No production network manager, lease, provider, socket, sandbox, proxy, forwarding, or lifecycle effect enters workloads, the durable adapter, or the process proof. |
| A16 | The server adapter still uses the one Engine execution-unit mutation route; document/index/journal atomicity and ambiguous-outcome rules are unchanged. |
| A17 | Metadata proves no new workspace edge and `nimbus-network -> nimbus-core` remains the network crate's only outgoing workspace edge. NNCV029 and the aggregate verifier pass all live and mutation cases. |
| A18 | Focused fail-before corrections, full affected suites, affected checks, strict Clippy, warning-denied rustdoc, format, diff, script quality, docs gates, and the one candidate-frozen structured review pass with exact evidence. |

## Required Behavioral Matrix

| Area | Happy path | Edge path | Error/failure path |
| --- | --- | --- | --- |
| Portable carrier | Populated compiled plan round-trips exactly. | Empty plan and maximum `u64` generation remain lossless. | Tuple/digest/partial/unknown wire shapes reject. |
| Correlation | Tenant, generation, activation, publication, and references match. | Active plus successor each retain one exact plan. | Every named crossing rejects before decision/effect. |
| Transition identity | Exact replay retains identity. | One semantic resource mutation changes identity. | Equal-generation divergent plan conflicts. |
| Physical codec | One compiled object round-trips. | Optional successor/failure remain absent rather than null. | Missing/null/unknown/crossed values return `Corrupt`. |
| Store | Missing CAS applies once; exact replay is unchanged. | Two contenders yield one winner. | Pre-persist failure leaves no document/index/journal; ambiguity requires fresh read. |
| Recovery process | Fresh Engine reconstructs exact populated plan. | Writer is killed exactly after durable intent. | Timeout/wait failure kills and reaps; snapshot handoff mutation fails verifier. |
| Pure decision | Valid intent yields complete reservation value. | Empty plan remains exact. | Corrupt/crossed/store failures yield no action or command. |

## Verification Commands

Focused development gates use bounded commands and the shared target:

```text
timeout 300 cargo nextest run -p nimbus-workloads -E 'test(/network_intent|compiled_network|transition_identity/)'
timeout 300 cargo nextest run -p nimbus-compute -E 'test(/workload_saga/)'
timeout 600 cargo nextest run -p nimbus-server -E 'test(/workload_saga_store/)'
bash scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
bash scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh --self-test
```

Candidate gates run only after A1-A17 are green:

```text
timeout 900 cargo nextest run -p nimbus-workloads
timeout 900 cargo nextest run -p nimbus-compute
timeout 1200 cargo nextest run -p nimbus-server
cargo check -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-targets --all-features
cargo clippy -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-features --no-deps
cargo fmt --all --check
git diff --check
bash -n scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
shellcheck scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Record exact pass, skip, ignored, mutation, schema-field, index, process PID,
cleanup, and verifier counts. A skipped child/provider lane is never reported
as passing evidence.

## Candidate Gate Evidence

| Gate | Exact result |
| --- | --- |
| Focused corrections | Compiled-plan durability `3/3`; its child role remains test-harness-only and ignored in ordinary execution. The bounded parent proves distinct writer/recovery PIDs, `killed-at-boundary-and-reaped`, `exited-and-reaped`, exact stderr cardinality, and a 4 KiB ceiling on both streams. |
| Full affected behavior | `nimbus-workloads` `96/96`; `nimbus-compute` `110/110` with one declared skip; `nimbus-server` `638/638` with 28 declared skips. Total `844/844`, 29 declared skips. |
| Physical contract | Exact schema has 17 required plus two optional fields and the same four indexes. Codec and process proofs retain the populated attachment, route, listener, dependency listener, selection, requirements, readiness, canonical bytes, and both digests. |
| Affected quality | All-target/all-feature check and strict Clippy pass for workloads, compute, and server. Warning-denied rustdoc, `cargo fmt --all --check`, and `git diff --check` pass. |
| Dependency/effect proof | Metadata reports `nimbus-core` as `nimbus-network`'s sole workspace dependency. NNCV029 passes `23/23`; the aggregate live verifier passes `30/30`; no manifest or forbidden effect path changed. |
| Adversarial proof | The bounded full prefix completed every aggregate mutation through NNCV026: 188 cases. Its one-hour outer bound ended during NNCV027's tenth case, after nine clean NNCV027 outcomes. A disjoint canonical tail then passed NNCV027 `10/10`, corrected NNCV028 `7/7`, and NNCV029 `8/8`. Exact complete coverage is `188 + 10 + 7 + 8 = 213`; partial NNCV027 outcomes are not double-counted. |
| Review corrections | Duplicate-field fail-before was exactly `0/1` with 95 filtered, then the typed top-level/carrier-member/generation matrix passed `1/1` with 95 filtered. NNCV028 now fails closed when the pinned NNC6.2 item commit is missing and its mutation matrix passes `7/7`. The narrow review's proof-only gap added a direct duplicate `formatVersion` content case; focused `1/1`, workloads `96/96`, and strict workloads Clippy pass afterward. |
| Script and docs quality | Bash syntax and scoped ShellCheck pass; the aggregate retains its documented inherited `SC2034,SC1091` exclusions. Docs pass 108 pages and the site passes `17/17`. |

## Review Cadence

Do not run structured autoreview during audit, fail-before work,
implementation, cleanup, or acceptance convergence.

After A1-A18 except review are green and the complete item is candidate-frozen,
run exactly one full structured GPT-5.6 Sol review at `xhigh` reasoning in fast
mode. The review unit is all of NNC6.2a, not a diff chunk. If it reports an
accepted finding that materially changes executable code, rerun the affected
proofs and exactly one narrow correction review focused on that accepted
defect. Docs, ledger wording, formatting, elapsed time, or internal review
chunking never trigger another review.

Never use Opus 4.8 for this review.

## Status Ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Source/dependency/store audit | `done` | Three bounded read-only inventories plus owner verification; no agent changed a path. |
| Carrier/reference/version decision | `done` | D1-D4 freeze one complete carrier, derived tuples, strict saga/transition v2, and exact correlations. |
| Physical codec/store decision | `done` | D5-D6 retain one compiled object and the existing Engine execution-unit adapter. |
| Pure action/process decision | `done` | D7-D8 freeze complete effect-free command material and exact crash reconstruction without snapshot handoff. |
| Source allowlist | `done` | The exact product/static/docs paths are frozen above; no manifest or provider/effect path is allowed. |
| NNCV029 expected red | `done` | Direct live exit is exactly `1` on the named missing carrier/v2/correlation/codec/action/process seams; helper mutations pass `8/8`; aggregate live proof is exactly `29 passed, 1 failed` at NNCV029. Bash syntax, direct and aggregate ShellCheck, and diff checks pass. |
| Historical behavioral fail-before | `done` | Workloads carrier/generation cases fail exactly `0 passed; 2 failed; 87 filtered`, exit `101`: the tuple omits plan/content/identity/listener bytes and accepts crossed generation. Server physical/IntentCommitted cases fail exactly `0 passed; 2 failed; 566 filtered`, exit `101`: no compiled physical object or exact plan/content bytes exist. NNCV029's direct live failures cover v2/correlation/codec/action/process absence; mutations are `8/8`. Existing strict compiled-plan and store baselines remain unmodified. |
| Portable carrier and correlations | `done` | A1-A8 pass with one complete carrier, derived references, strict v2, and exact active/successor correlation. |
| Strict physical codec/schema | `done` | A9-A10 pass with 17 required plus two optional fields, four unchanged indexes, and exact populated bytes. |
| Store and distinct-process proof | `done` | A11-A13 and A16 pass, including exact replay, ambiguity/atomicity baselines, distinct PIDs, cleanup, and bounded diagnostics. |
| Pure decision/zero-command proof | `done` | A14-A15 pass; reservation carries the exact validated plan and no effect authority entered. |
| Dependency/static acceptance | `done` | A17 passes at NNCV029 `23/23` plus `8/8`, aggregate `30/30`, and exact 213-case complete coverage. |
| Candidate quality gates | `done` | A18 is green at affected `844/844` with 29 declared skips and every recorded affected quality/docs gate. |
| Full structured review | `done` | The one Sol/xhigh/fast item review accepted two P2 defects at overall confidence `0.95`; both are corrected with exact fail-before and green proofs. |
| Narrow correction review | `done` | The sole narrow Sol/xhigh/fast review confirmed the implementations and found one accepted P2 test-only gap at confidence `0.96`; the direct inner-content duplicate case is green. No third review ran or is warranted. |
| Item commit | `todo` | One exact reviewed NNC6.2a commit; no push or PR. |
