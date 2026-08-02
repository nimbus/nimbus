# NNC6.2 Admitted Network Plan Compiler

Status: `complete; C1-C18 and full+narrow review cadence reconciled`

Starting checkpoint: `9b2f6f91f5ff429a10dfe1979291806e167d8d8e`

Durable audit checkpoints: `c2dc4b4c0666f4d21dcca1788e3079d3c8bdf4e6`,
`e403741bad68555af4e580b80e04c4fae73ce014`, and
`e183c7b8fe28cf88ff7dcb81f01c25d6b47b4cd1`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md` NNC6.2

NNC6.2 adds one pure compiler in `nimbus-compute`. The compiler converts an
already-admitted workload and its source-owned network values into one exact
`NetworkPlan` and one canonical portable resource payload. The compiler does
not read or write stores. It does not lease resources or call providers. It
does not use sockets, files, clocks, environment variables, or random values.

This proof freezes the implementation contract before product code changes.
The source audit found a separate durability gap. NNC6.2a now owns that gap.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Owner worktree | `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit` |
| Owner branch | `codex/nimbus-network-architecture-audit` |
| Starting HEAD | `9b2f6f91f5ff429a10dfe1979291806e167d8d8e` |
| Last completed item | `NNC6.1e`, commit `2204fa8d7a886b3557709932f02944961c629c4b` |
| Current item | `NNC6.2` |
| Current dirty paths | NNC6.2's frozen compute, workloads, tenant, sandbox, identity, verifier, census, proof, and recovery paths only. The child-process harness moved to its concept-owned compute test child. No Cargo manifest or saga path changed. |
| Product state | The pure compiler now rejects source-sovereignty relaxation before selection, and the portable payload retains the tenant-qualified workload identity, complete capability requirements, exact selection, resources, derived readiness, and every envelope field needed for strict decoding. C1-C18 and the affected correction gates are green. No lifecycle or provider effect is routed. |
| Last green | Affected behavior `1,477/1,477` with 27 declared skips; affected all-feature and workspace all-target checks; strict Clippy; warning-denied rustdoc; focused workloads `14/14`; focused compute `15/15` with one child-only ignore; NNCV028 `18/18` plus `6/6`; aggregate `29/29`; exact aggregate arithmetic `198 + 6 = 204`; complete split-bound mutation coverage; format/diff; Bash/ShellCheck; docs 108; and site `17/17`. |
| Next action | Stage the final ledger-only rows, confirm the final executable/static-proof digest is unchanged and no unstaged path exists, then commit the exact NNC6.2 item. |
| Structured review | The one full Sol/xhigh/fast item review reported six findings at confidence `0.98`: five accepted/corrected and one source-contract rejection. The sole narrow Sol/xhigh/fast review confirmed the five behavioral corrections and reported one accepted P3 exact-count defect at confidence `0.96`. Review cadence is exhausted; no third review is allowed. |
| Corrected candidate identity | The exact 23-path candidate was staged with no unstaged path at pre-ledger tree `0ebb9ba62ce52476b9b060a59946dabd00e4e9cf`; its complete staged patch SHA-256 was `c80a0425150b33a8cb6ec2705244411003cc5d6aa2a0a0f24ab2ced107e53574`. The frozen `crates/` plus `scripts/` executable/static-proof SHA-256 is `37a0eb67766208c4484854da0371884025a670ccba2f4b6788cdac11428fd481`; this ledger-only identity row does not alter it. |
| Final post-review candidate identity | After the sole narrow review's accepted count correction, the exact 23-path pre-ledger staged tree is `1ae690409c2c05c1474b52af7cec5242b97605a0`; complete staged patch SHA-256 `9a749f90cdeaca9b2e210c6359a892693f30dbce5526f48748f1fc136d2fc048`; final `crates/` plus `scripts/` executable/static-proof SHA-256 `c149dd56fc0908581c0a0ac0d168891b52446e20ec86707cdbb2877d43a7e813`. This final ledger-only row does not alter the executable digest. |
| Blocker | None. |

## Audit Verdict

The current network contracts can represent an exact plan envelope. The
current product cannot compile or recover one from admitted workload intent.

The audit found these facts:

1. `NetworkPlan` stores a stable plan ID, generation, content digest,
   capability requirements, and readiness requirements.
2. The upper layer must own the canonical resource encoding behind the content
   digest.
3. `nimbus-compute` already depends on tenant, services, sandbox, workloads,
   machine, and network crates.
4. `nimbus-compute` is the sole cross-domain workload saga coordinator.
5. `nimbus-workloads` stores only the network plan ID, generation, and final
   digest in each saga intent.
6. That tuple cannot reconstruct resource content, capability requirements, or
   readiness requirements after a crash before network reservation.
7. The live OCI compiler uses a backend-generated `SandboxId`, fixed generation
   `1`, and a provider label.
8. The OCI compiler has live reserve, startup recovery, orphan classification,
   and machine publication consumers.
9. Removing the OCI compiler before the compute ingress passes an exact plan
   would break live recovery.
10. Current service and sandbox lifecycle paths call backend effects directly.
11. Those paths do not carry an admitted deployment generation or node
    assignment.
12. Current service admission does not carry the operator network decision.

NNC6.2 therefore compiles values only. NNC6.2a persists the complete compiled
value and proves fresh-process replay. NNC6.1e1 cuts lifecycle callers over to
the durable compute ingress. Later effect items consume issued commands.

## Acceptance-Convergence Corrections

Owner inspection found four proof gaps before candidate freeze. None came from
structured review.

1. The first C5 test covered sandbox listeners but not admitted service routes.
   The missing route case failed `0/1`: changing only the route host and ports
   changed `NetworkPlanId` because `TenantWorkloadUid` transitively includes the
   complete admission decision digest. The compiler now derives a length-framed
   network-incarnation key from the tenant-qualified admitted workload subject,
   adding the standalone sandbox stable resource ID only where the subject does
   not distinguish sibling sandboxes. Decision ID, workload UID, IP addresses,
   and ports are forbidden identity inputs. The corrected proof passes `1/1`.
2. C7 now enumerates and pins all 26 retained semantic leaves in the version-one
   payload. Mutating any one changes the exact retained bytes and content digest;
   valid constructor-level route, listener, sovereignty, activation, and
   publication mutations remain separate behavioral proofs.
3. C12 now isolates locality, external dependency, and offline-restart evidence.
   A source-bearing compile may only refine the source-owned sovereignty floor:
   a broader locality, an added external dependency, or removal of required
   offline restart is rejected before selection and effects. A source-free
   empty plan retains the caller's complete sovereignty baseline, and every
   valid change alters both content and complete plan digests.
4. C14 now places recording store, lease, provider, network-manager-mutation,
   and sandbox-start counters immediately above the pure compiler. A successful
   compile proves the recorder is live. Source correlation, capability
   selection, and portable-payload failures all return with every counter at
   zero. The compiler signature and NNCV028 independently forbid effect owners.

The compiler also requires honest port-forwarding evidence for any listener
with a guest port. Current server listener registration does not claim that
capability, so selection fails closed until a later effect owner registers the
real forwarding composition. NNC6.2 does not invent that evidence or route the
effect.

## Structured Review Disposition

The one full item review used GPT-5.6 Sol with `xhigh` reasoning and fast mode
after C1-C18 and the original candidate gates were green. It reported six
findings and an overall incorrect verdict at confidence `0.98`. The item was
not closed on that candidate.

| Finding | Disposition | Correction or evidence |
| --- | --- | --- |
| P1: source sovereignty could be overwritten | `accepted; corrected` | `aggregate_requirements` now requires monotonic sovereignty refinement before exact provider selection. One combined three-dimension case and each isolated dimension fail with stable typed diagnostics and zero effects; valid stricter refinements remain digest-bound. |
| P2: content digest alone did not authenticate the plan envelope | `accepted; corrected` | Portable content retains complete capability requirements and exact readiness provenance. Construction and decoding compare content digest, plan ID, generation, sovereignty, all capability requirements, and derived readiness. `from_content` is the sole content-derived envelope constructor used by compute. |
| P2: portable resource IDs were not rederived from workload identity | `accepted; corrected` | `WorkloadNetworkPlanIdentity` retains tenant, workload-incarnation key, and generation. Content construction and strict decoding rederive attachment, route, listener, endpoint, and lease IDs; crossed-tenant or crossed-name authority fails. |
| P2: named standalone owner must equal the admitted profile | `rejected` | The API models `profile` and optional standalone `displayName` as different fields. Public SDK documentation creates profile `worker` with display name `batch-job`, and live server tests exercise profile `worker` with independent display names. The compiler already correlates the admitted profile and stable sandbox resource ID; equating the human display label would reject valid requests. |
| P3: child payload was counted as a no-op pass | `accepted; corrected` | The payload moved to a concept-owned child module and is explicitly ignored in normal suites. The parent invokes that exact ignored test with the required marker; ordinary marker-free invocation fails rather than passing vacuously. |
| P3: distinct-process proof was unbounded | `accepted; corrected` | The parent uses a 15-second bounded `try_wait` loop, captures diagnostics, and kills and reaps on timeout or wait failure. |

The material executable corrections justify exactly one narrow correction
review focused on these five accepted defects and their regression surface.
Documentation, ledger wording, formatting, or elapsed time do not justify any
additional review.

That sole narrow review used GPT-5.6 Sol with `xhigh` reasoning and fast mode.
It confirmed the five behavioral corrections and reported one P3 bookkeeping
defect at confidence `0.96`: the aggregate summary retained 203 even though
HEAD's 198 cases plus NNC6.2's six counted mutations equals 204. The finding is
accepted. The summary and every owning ledger now report 204; the affected
arithmetic, static, and docs proofs run once after this correction. No third
structured review is permitted or warranted.

## Prospective Split

The split occurred before product implementation or structured review.

| Item | Unit of value | Acceptance boundary |
| --- | --- | --- |
| NNC6.2 | Pure compute-owned compilation from admitted source values to an exact canonical plan payload. | Determinism, validation, stable identity, exact generation, capability proof, and zero effects. |
| NNC6.2a | Workloads-owned durable carrier for the complete compiled value. | Strict codec and fresh-process exact plan reconstruction before any network command. |
| NNC6.1e1 | Compute-owned lifecycle ingress and caller cutover. | Persist admitted intent before commands and remove direct service lifecycle authority. |
| NNC6.3 through NNC6.6 | Provision, activation, restart, teardown, and resolution fencing. | Existing named effect-order and recovery obligations. |

NNC6.2 does not claim fresh-process recoverability. The NNC6.2a expected-red
test records that missing capability.

## Ownership And Dependency Decision

```text
nimbus-tenant
  policy decision and validated endpoint projection
          \
nimbus-services                    nimbus-sandbox
  logical service source           sandbox desired source and attachment facts
          \                         /
           \                       /
            v                     v
                nimbus-compute
          pure admitted-plan compiler
                      |
                      v
               nimbus-workloads
       portable compiled resource payload
                      |
                      v
               nimbus-network
       plan envelope and lifecycle contracts
```

The dependency graph stays acyclic:

```text
nimbus-network -> nimbus-core
nimbus-sandbox -> nimbus-network
nimbus-tenant -> nimbus-sandbox + nimbus-network
nimbus-workloads -> nimbus-tenant + nimbus-network
nimbus-services -> nimbus-workloads + nimbus-tenant + nimbus-sandbox + nimbus-network
nimbus-compute -> nimbus-services + nimbus-workloads + nimbus-tenant + nimbus-sandbox + nimbus-network
```

The compiler does not belong in these crates:

| Candidate | Rejection |
| --- | --- |
| `nimbus-network` | It would import policy or upper workload values and reverse required edges. |
| `nimbus-workloads` | It cannot import services or sandbox without a cycle. |
| `nimbus-tenant` | Tenant owns policy admission, not connectivity resource composition. |
| `nimbus-services` | Services owns logical names and resolution, not cross-domain desired state. |
| `nimbus-sandbox` | Sandbox owns provider effects and the transitional fixed-generation compiler. |
| `nimbus-system` | System owns rebuildable observed projections only. |

Do not add a compiler trait. One direct pure compiler has one current
consumer in NNC6.1e1. A trait requires two real substitutable implementations.

## Current Authority And Gap Map

| Concern | Current source | Audit result |
| --- | --- | --- |
| Plan envelope | `crates/nimbus-network/src/plan.rs` | Sound low-dependency contract. Resource bytes remain an upper-layer responsibility. |
| Stable plan and attachment IDs | `crates/nimbus-network/src/identity.rs` | Plan and attachment derivations exist. Endpoint and route deterministic derivations are missing. |
| Tenant admission | `crates/nimbus-tenant/src/decision.rs` | Decision ID binds tenant, workload, location, generation, and policy when callers supply them. |
| Tenant network input | `crates/nimbus-tenant/src/policy_input.rs` | Endpoint fields lack read accessors. Direct programmatic input can bypass operator endpoint validation. |
| Service desire | `crates/nimbus-services/src/catalog.rs` | Logical name, backend, and generation exist. Sandbox-backed services carry a `SandboxSpec`. |
| Sandbox desire | `crates/nimbus-sandbox/src/spec.rs` | Backend and port bindings exist. Host address and port are desired values, not identity. |
| Sandbox capability facts | `crates/nimbus-sandbox/src/backends/capabilities.rs` | The pure backend mapping exists but remains private. |
| Saga network intent | `crates/nimbus-workloads/src/saga.rs` | The stored tuple is not a reconstructable plan. NNC6.2a owns the durable correction. |
| Compute composition | `crates/nimbus-compute/src/state.rs` | One network manager and one saga coordinator already share the owner. |
| Transitional plan | `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/plan.rs` | Fixed generation and provider-shaped content remain live until caller cutover. |
| Service lifecycle | `crates/nimbus-services/src/manager/activation.rs` | The manager admits and starts directly. NNC6.1e1 owns the cutover. |
| Standalone sandbox lifecycle | `crates/nimbus-services/src/manager/sandboxes.rs` | The manager admits and starts directly. The backend mints identity after the effect boundary. |

## Frozen Compiler Inputs

The compiler accepts a closed source variant. It does not accept arbitrary
canonical bytes or a caller-supplied content digest.

```text
AdmittedWorkloadNetworkSource
  = Empty
  | Sandbox {
      stable_resource_id,
      profile,
      generation,
      sandbox_spec
    }
  | SandboxBackedService {
      service_name,
      service_generation,
      sandbox_spec
    }
```

Every variant also receives these exact values:

- one `TenantIsolationDecision`.
- one explicit `NetworkCapabilitySelection` when resources require providers.
- one immutable `NetworkCapabilityRegistry` snapshot for pure exact
  satisfaction.
- one explicit sovereignty requirement value.
- one activation intent.
- one publication intent.

The compiler derives `TenantWorkloadSpec` from the admitted decision. The
compiler does not accept a caller-supplied workload UID, decision ID, tenant,
node assignment, or network generation.

The decision must carry a deployment generation and assigned node. The source
generation must equal that admitted generation. A missing generation or node
assignment returns a typed error.

The compiler accepts only sandbox-backed services. Built-in and external
services do not create a sandbox workload saga. A truly network-empty admitted
workload uses the explicit `Empty` variant.

## Source Validation

Tenant admission remains the first validation owner. NNC6.2 adds narrow tenant
read accessors and admission checks where current direct input can bypass the
operator policy validator.

The tenant owner must reject these values before compilation:

- an empty service or endpoint name.
- surrounding or embedded whitespace in a required name.
- a malformed endpoint host.
- host port `0`.
- guest port `0`.
- a duplicate service and endpoint-name pair.
- an endpoint whose service is not in the admitted service grant.

The two unused private booleans in `TenantNetworkPolicyDecision` have no
constructor, reader, or producer. NNC6.2 removes them instead of assigning new
policy meaning.

The compiler then checks source correlation:

1. The decision tenant equals the source tenant.
2. The decision workload kind equals the source variant.
3. The decision workload name equals the service name or sandbox profile.
4. The admitted sandbox backend equals the desired sandbox backend.
5. A service sandbox owner names the same service.
6. A standalone sandbox owner is not service-owned.
7. The admitted generation equals the source generation.
8. The admitted node assignment exists.
9. The admitted egress policy equals the sandbox desired egress policy.
10. Every canonical resource name is unique in its resource domain.

An error returns before capability selection. It also returns before every
store, lease, provider, sandbox, proxy, or network lifecycle call.

## Endpoint And Listener Semantics

Tenant network endpoints and sandbox port bindings are different values.

| Value | Meaning | Owner |
| --- | --- | --- |
| Tenant network endpoint | Admitted connectivity to a logical service endpoint. | Tenant policy, with logical service correlation retained by services. |
| Sandbox port binding | Desired listener and publication request for the workload. | Sandbox desired source, compiled by compute. |
| Published endpoint | Observed reachable location after provider effects. | Sandbox or machine provider observation. |

The compiler does not join tenant endpoints to sandbox bindings. It
canonicalizes both dimensions independently. The compiler leaves logical
service names unresolved. It does not query DNS or bind a port. It copies no
observed address.

## Stable Identity And Fencing

The compiler derives identity from a compiler-local, length-framed network
workload-incarnation key. Its base is the tenant-qualified admitted
`WorkloadIdentity::subject()`. A standalone sandbox also contributes its
admitted stable resource ID because the placement-free subject deliberately
does not include a sandbox ID. A service name, workload kind, backend, tenant,
and generation are already part of the admitted subject.

`TenantWorkloadUid` remains saga and execution evidence. It is not a network
resource-identity input because it binds the complete decision digest, including
mutable network address and port content.

```text
NetworkPlanId
  <- TenantId + network workload-incarnation key

NetworkAttachmentId
  <- network workload-incarnation key + attachment name

ListenerId
  <- TenantId + network workload-incarnation key + listener name

PortLeaseId
  <- ListenerId

PublishedEndpointId
  <- network workload-incarnation key + endpoint name

IngressRouteId
  <- network workload-incarnation key + service name + route name
```

NNC6.2 adds deterministic endpoint and route constructors to the portable
identity contract. The constructors accept neutral strings only. They do not
import tenant, service, sandbox, or compute types.

An admission decision ID, `TenantWorkloadUid`, IP address, host port, guest
port, provider ID, provider handle, process ID, or backend-generated
`SandboxId` never forms network resource identity. Changing an address or
requested port changes desired content, but it does not change the stable
resource ID. Same-profile standalone sandboxes remain distinct through their
admitted stable resource IDs.

`NetworkResourceGeneration` equals the admitted `WorkloadGeneration` without a
lossy conversion. The compiler does not assign a lease epoch. The reservation
authority assigns and persists epochs after intent commit.

## Canonical Portable Payload

`nimbus-workloads` owns one strict and versioned
`CompiledWorkloadNetworkPlan` value. NNC6.2 creates the value. NNC6.2a embeds
it in durable saga intent.

The value contains:

- the complete `NetworkPlan`.
- an optional named attachment blueprint.
- canonical admitted service endpoint routes.
- canonical desired listener blueprints.
- stable published endpoint IDs.
- exact port request modes without an epoch or selected address.
- the explicit capability selection used for the plan.
- the explicit sovereignty requirements.
- the exact activation and publication intent inputs needed to interpret the
  resource set.

The value excludes:

- provider handles.
- selected or observed IP addresses.
- allocated numeric ports when the request used provider assignment.
- lease epochs.
- sockets or file descriptors.
- service binding snapshots.
- DNS results.
- proxy policy bytes.
- certificate material.
- cluster membership or transport state.
- system projection rows.

The canonical encoding uses a format-version field, strict unknown-field
rejection, stable field order, and canonical sorted resource arrays. Duplicate
resource identities fail. They are not silently collapsed.

The compiler serializes this retained payload once. It computes
`NetworkPlanContentDigest` from the exact retained canonical bytes. It then
constructs `NetworkPlan`. A second hand-written digest encoding is forbidden.

The final plan digest binds these dimensions:

```text
canonical retained resource bytes
+ capability requirements
+ readiness requirements
```

Every semantic field has a mutation test. Observed fields have an exclusion
test.

## Capability And Sovereignty Contract

The compiler states needs. Source-owned registrations state facts.

`nimbus-sandbox` projects its current attachment requirements without effects.
The projection reuses the existing container and krun mapping.
Compute does not copy backend provider keys or attachment mode tables.

The compiler aggregates these needs:

- the source-owned attachment requirements.
- address families from desired listeners.
- host bind realm for current local listeners.
- requested exposure values.
- TCP transport for TCP, HTTP, and HTTPS endpoint protocols.
- exact or provider-assigned port mode.
- required ingress features.
- port-forwarding capability for every listener that targets a guest port.
- durable inspect, reconcile, and delete lifecycle features.
- the explicit sovereignty requirements.

The first supported source is the local Nimbus-host-managed sandbox path.
Provider-managed machine modes remain a separate machine-owned projection.
Supplying a provider-managed selection to a host-managed sandbox requirement
returns a typed capability error. The compiler never falls back to another
registered provider.

The initial sovereignty input must state all three dimensions:

- maximum control-plane locality.
- allowed external dependencies.
- offline restart requirement.

No backend default can silently broaden those dimensions. Local Nimbus
composition passes `LocalOnly`, no external dependency, and offline restart
required.

## Readiness Contract

Readiness requirements form part of the plan digest.

The initial compiler emits:

1. One attachment `Ready` requirement for an attachment-bearing plan.
2. One ingress `Ready` requirement for each desired published listener.
3. One sandbox-owned PEP `Ready` requirement when the admitted sandbox source
   requires the PEP lifecycle.
4. No readiness requirement for a tenant service endpoint route. Services
   retains logical resolution and route readiness.
5. No readiness requirement for an explicit empty plan.

The exact attachment and ingress provider IDs come from the admitted
`NetworkCapabilitySelection`. The PEP provider ID comes from the sandbox-owned
pure projection. The compiler does not invent or auto-select a provider.

Each requirement names a stable resource ID, exact provider ID, and condition
kind. Canonicalization rejects exact duplicates.

## Empty Plan Semantics

An explicit empty plan has no attachment, route, listener, endpoint, port, or
readiness resource. It still has a tenant-qualified plan ID, admitted
generation, canonical empty payload, explicit empty capability needs, and
explicit sovereignty constraints.

A sandbox with no published port is not empty. It still has an attachment and
PEP readiness requirements. A sandbox-backed service follows the same rule.

A stopped successor does not erase active network authority. The saga retains
the active compiled plan for teardown. A new terminal intent can carry an
explicit empty plan only when it has no resource to provision or release.

## Transitional OCI Compiler

The current live compiler remains at:

```text
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/plan.rs
```

Its production consumers remain:

```text
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/state.rs
crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence/classifier.rs
crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs
```

NNC6.2 adds no new caller to that compiler. NNC6.2 also adds no caller from the
OCI compiler to the compute compiler.

NNC6.1e1 and the later effect items must pass the durable compiled plan into
the sandbox command path. The deletion gate removes the fixed generation,
provider-label digest, and recovery reconstruction only after all three live
consumers use the exact issued plan.

The static verifier records the current caller baseline. Any new production
caller fails the NNC6.2 gate.

## Frozen Source Allowlist

### Pure compiler

```text
crates/nimbus-compute/src/lib.rs
crates/nimbus-compute/src/workload_network_plan.rs
crates/nimbus-compute/src/workload_network_plan/tests.rs
```

The final child name can change before its first edit. It must remain a
concept-owned compiler module. `helpers.rs`, `common.rs`, `misc.rs`, and
`utils.rs` are not allowed.

### Portable compiled payload

```text
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/network_plan.rs
crates/nimbus-workloads/src/network_plan/tests.rs
```

NNC6.2 must not edit `crates/nimbus-workloads/src/saga.rs`. NNC6.2a owns that
durable carrier change.

### Source-owned pure projections

```text
crates/nimbus-tenant/src/decision.rs
crates/nimbus-tenant/src/policy_input.rs
crates/nimbus-tenant/src/operator_policy/validation.rs
crates/nimbus-tenant/src/tests.rs
crates/nimbus-sandbox/src/lib.rs
crates/nimbus-sandbox/src/backends/mod.rs
crates/nimbus-sandbox/src/backends/capabilities.rs
crates/nimbus-sandbox/src/backends/capabilities/tests.rs
crates/nimbus-sandbox/src/backends/oci/port_lease.rs
```

The allowlist admits the tenant validation path only to share one field
validator between direct decision input and operator policy input. It admits
the OCI lease path only to consume one sandbox-owned PEP provider key. Neither edit may
change admission meaning, allocate a port, or call a provider.

The `backends/mod.rs` edit is the minimal two-symbol visibility seam required
to re-export the pure projection without making the concept-owned
`capabilities` module public.

### Portable identity

```text
crates/nimbus-network/src/identity.rs
```

### Verification and recovery state

```text
scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.2-admitted-network-plan-compiler.md
```

The source-contract edit removes NNCV024's completed-item dirty-worktree
freeze while retaining its source-derived effect scan and mutation. That
freeze incorrectly rejected every later transport-free `nimbus-network`
contract addition. The census JSON accepts only the regenerated source line
for the existing attachment-registration authority after this item adds the
pure projection above it; the authority identity and classification do not
change.

Do not edit Cargo manifests. All required edges already exist.

An edit outside this allowlist requires an audit amendment before the edit.

## Explicit Non-Goals

NNC6.2 does not:

- persist the compiled payload in a workload saga.
- route a lifecycle caller through compute.
- reserve a segment, address, port, or listener.
- create or inspect provider state.
- bind a socket.
- start, stop, inspect, restart, or attach a sandbox.
- resolve a service name.
- publish an endpoint.
- move egress PDP rules or proxy PEP behavior.
- select certificates or move the interception CA.
- change system projections.
- add DNS, xDS, Consul, Iroh, overlay networking, or cluster transport.
- implement WSL2 or another provider-managed machine network.
- remove the transitional OCI compiler.
- add a compatibility shim or feature flag.

## Fail-Before Packet

No compiler implementation starts before the following expected-red tests and
static cases fail for their named missing behavior.

| ID | Expected-red proof | Required failure |
| --- | --- | --- |
| F1 | Compile the same admitted sandbox source twice. | The compute compiler or compiled payload type does not exist. |
| F2 | Serialize and decode one compiled value in a distinct process. | No strict reconstructable compiled payload exists. |
| F3 | Permute tenant routes and sandbox bindings. | No canonical compiler exists. |
| F4 | Submit duplicate route, listener, and endpoint identities. | No named compile-time rejection exists. |
| F5 | Cross tenant, workload kind, name, backend, generation, and node input. | No pure source correlation gate exists. |
| F6 | Submit direct programmatic invalid tenant endpoints. | Tenant admission accepts at least one value that the operator path rejects. |
| F7 | Compare empty, attachment-only, and attachment-plus-listener plans. | No exact production compiler distinguishes them. |
| F8 | Mutate each retained semantic field. | No retained canonical payload or field-complete digest proof exists. |
| F9 | Change address or port while retaining the logical resource name. | Deterministic endpoint and route IDs do not exist. |
| F10 | Replay an equal generation with changed content. | No admitted compiler output reaches `NetworkPlan::classify_update`. |
| F11 | Request an unregistered or unsatisfied exact provider composition. | No compiler capability check exists. |
| F12 | Substitute provider-managed facts for host-managed sandbox need. | No compile boundary rejects the substitution. |
| F13 | Reject before effects with recording upper-boundary counters. | No compile entrypoint exists to prove zero store, lease, provider, and sandbox calls. |
| F14 | Scan production `oci_attachment_plan` callers. | The baseline exists and the verifier has no NNC6.2 guard. |
| F15 | Decode the current saga tuple and reconstruct a full plan. | Reconstruction is impossible. This remains expected red for NNC6.2a. |

The expected-red checkpoint records each command, exit status, test count, and
failure assertion. Missing tests or missing scan inputs are failures.

## Written Acceptance Criteria

| ID | Criterion | Verifiable proof |
| --- | --- | --- |
| C1 | Compute owns one direct pure compiler. | Source and dependency scans find one compiler and no compiler trait. |
| C2 | Tenant admission validates every consumed endpoint field. | Direct input tests reject malformed names, host, ports, duplicates, and ungranted services before compilation. |
| C3 | Source correlation is exact. | Crossed tenant, kind, name, owner, backend, and generation cases plus a missing admitted node return named errors. The source cannot supply a second node authority. |
| C4 | Compilation is deterministic. | Replays, permutations, serialization, and a distinct process produce byte-identical payloads and plan digests. |
| C5 | Resource identities are stable and tenant-qualified. | Same names across tenants do not alias. Address and port changes preserve IDs. |
| C6 | Generation and fencing are exact. | Network generation equals admitted workload generation. The compiler never assigns an epoch. |
| C7 | Canonical content is retained. | The content digest is computed from the exact retained bytes. Every semantic field mutation changes it. |
| C8 | Empty and attachment-bearing plans differ honestly. | Empty, attachment-only, and attachment-plus-listener matrix tests pass. |
| C9 | Tenant routes and sandbox listeners remain separate. | Tests preserve both canonical dimensions without name resolution or observed endpoint copying. |
| C10 | Capability needs come from source-owned projections. | Container and krun need tables reuse sandbox projection. Compute contains no backend provider key table. |
| C11 | Exact selection is pure and fail closed. | Registered satisfying selection passes. Missing, mixed, or unsatisfied selection returns typed diagnostics with no fallback. |
| C12 | Sovereignty is explicit and digest-bound. | Locality, external dependency, and offline restart mutations change the plan digest and selection result. |
| C13 | Readiness requirements are complete. | Attachment, ingress listener, and PEP matrices contain exact stable resource and provider IDs. Routes and empty plans add none. |
| C14 | Admission failure makes zero effects. | Recording counters remain zero for store, lease, provider, network manager mutation, and sandbox start on every error. |
| C15 | The low dependency seam remains exact. | Metadata shows `nimbus-network -> nimbus-core` as its only workspace edge. Forbidden import and effect scans pass. |
| C16 | Transitional authority does not spread. | The source-derived OCI compiler caller set has no additions. The proof names its later deletion gate. |
| C17 | The durability gap stays truthful. | The current saga tuple remains expected red for full reconstruction, and NNC6.2a owns the exact fresh-process proof. |
| C18 | Candidate quality gates pass once. | Focused and full affected tests, checks, strict Clippy, rustdoc, verifier, format, diff, and docs gates pass before the separately recorded candidate-frozen review. |

## Acceptance Evidence Ledger

| ID | Status | Evidence |
| --- | --- | --- |
| C1 | `green` | NNCV028 finds exactly one concrete `WorkloadNetworkPlanCompiler` and one direct `compile` entrypoint in `nimbus-compute`, with no compiler trait or effect import. |
| C2 | `green` | `nimbus-tenant` direct-input tests reject empty/whitespace names, URL-shaped hosts, zero host/guest ports, duplicate route keys, and ungranted services; the full crate suite passes `97/97`. |
| C3 | `green` | `missing_generation_node_and_crossed_source_fields_are_typed`, `source_correlation_fails_closed_before_capability_selection`, and the service/standalone ownership cases return typed failures before selection. |
| C4 | `green` | Replay, input permutation, strict serialization, and the child-process compiler proof produce byte-identical content and plan digests. |
| C5 | `green` | The retained tenant-qualified workload identity rederives plan, attachment, route, listener, endpoint, and lease IDs during construction and strict decoding. Crossed tenant/name IDs fail; IP addresses, ports, decision ID, and `TenantWorkloadUid` remain forbidden identity inputs. |
| C6 | `green` | The compiler copies the admitted deployment generation exactly, replacement generations receive a new workload-incarnation plan, and source/static scans find no lease-epoch assignment. |
| C7 | `green` | `semantic_field_mutations_change_the_exact_canonical_bytes` pins every retained semantic leaf; every valid leaf mutation changes canonical bytes and the SHA-256 content digest. Constructor and strict-deserialization tests authenticate the complete envelope and reject crossed content, identity, generation, sovereignty, capability, and readiness values. |
| C8 | `green` | Explicit empty, attachment-only, and attachment-plus-listener plans have distinct exact resource/readiness matrices. |
| C9 | `green` | `service_routes_remain_separate_from_published_listeners` and route permutation tests retain routes and listeners as separate canonical dimensions without name resolution or observed copying. |
| C10 | `green` | `sandbox_network_plan_requirement_projection_is_exact_and_effect_free` projects container/krun requirements in `nimbus-sandbox`; compute consumes that projection and defines no backend provider-key table. |
| C11 | `green` | Exact registered selection succeeds; missing, unknown, mixed, unsatisfied, and substituted attachment selections fail with typed errors and no fallback. |
| C12 | `green` | Source-bearing locality, external-dependency, and offline-restart relaxation each fail before selection/effects, including one stable three-dimension diagnostic. Valid stricter refinements and source-free caller baselines remain fully retained and digest-bound. |
| C13 | `green` | Attachment, listener, and PEP readiness rows carry exact stable resource/provider IDs. Empty and route-only dimensions add no readiness rows. |
| C14 | `green` | A live upper-boundary recorder observes all five effect classes after successful compilation and zero on source correlation, sovereignty refinement, selection, and payload failures; the compiler type and NNCV028 admit no effect capability. |
| C15 | `green` | Metadata and NNCV004/NNCV012 prove `nimbus-network -> nimbus-core` is the sole workspace edge and no forbidden dependency, import, or provider effect entered the contract crate. |
| C16 | `green` | NNCV028 pins the three-file/four-call production `oci_attachment_plan` baseline and its caller-growth mutation fails closed. NNC6.1e1/NNC6.3 retain the later cutover/deletion gate. |
| C17 | `green` | `saga.rs` is unchanged and still cannot reconstruct the complete plan tuple; NNCV028 reserves durable embedding and the distinct-process replay proof exclusively for NNC6.2a. |
| C18 | `green` | The candidate gate ledger below records every required test, check, lint, static, format, diff, and docs result. The full review's five accepted executable defects are corrected and proven; final candidate refreeze and the one narrow correction review remain separate closeout gates. |

## Candidate Gate Evidence

| Gate | Exact result |
| --- | --- |
| Full affected behavior | `nimbus-network` `236/236` with one child-only skip; `nimbus-tenant` `97/97`; `nimbus-sandbox` `948/948` with 25 declared child/provider/scale skips; `nimbus-workloads` `88/88`; `nimbus-compute` `108/108` with one child-only ignore. Total `1,477/1,477`, 27 declared skips. |
| Focused review corrections | Workloads portable-plan behavior `14/14`; compute compiler behavior `15/15`, with the explicitly ignored child payload invoked only by the bounded parent process proof. |
| Post-inspection focused correction | `newer_generation_creates_a_new_workload_incarnation_and_exact_fence` `1/1`; 108 unrelated compute tests filtered, not claimed as skips. |
| Affected all-feature check | `cargo check` for network, tenant, sandbox, workloads, and compute with all targets/all features: exit `0`. |
| Workspace check | `cargo check --workspace --all-targets`: exit `0` under the supported default feature set. |
| Strict Clippy | Five affected crates, all targets/all features, `--no-deps -- -D warnings`: exit `0`. Vendored dependency warnings are outside the no-deps lint boundary. |
| Warning-denied rustdoc | Five affected crates, all features, no dependencies, `RUSTDOCFLAGS=-Dwarnings`: exit `0`. |
| NNCV028 | Live contract `18/18`; missing-compiler, missing-payload, caller-growth, decision-bound-identity, uncorrelated-envelope, and uncorrelated-resource-ID mutations `6/6`. |
| Aggregate static proof | Live verifier `29/29`. The 1,800-second top-level run passed every sequential case through 8/9 NNCV020 crash mutations before its outer bound terminated the child without an assertion failure. An exact continuation began at the sole unrun `missing-pre-crash-witness` mutation, ran every later helper through NNCV028, and ended `SELFTEST TAIL PASS`; together the two disjoint sweeps cover the aggregate's complete 204-case contract. |
| Script quality | `bash -n` passes; the NNC6.2 helper passes direct ShellCheck; the aggregate passes with its documented inherited `SC2034,SC1091` exclusions. |
| Format and diff | `cargo fmt --all --check` and `git diff --check`: exit `0`. Bash syntax and scoped ShellCheck pass. The production compute composition root is below 1,500 lines; the child-process harness is concept-owned rather than inflating its parent test module. |
| Documentation | `check-docs` passes 108 pages; docs-site verification passes `17/17`. |
| Workspace all-feature disposition | The deliberately attempted workspace-wide all-feature check exits `101` only because dormant `nimbus-fs/fuse` scaffolding requires unavailable system `fuse.pc`. The affected all-feature and workspace all-target gates above provide the supported coverage; no lane is reported as skipped/passing. |

## Behavioral Test Matrix

### Happy paths

- explicit empty admitted workload.
- container standalone sandbox with no published listener.
- krun standalone sandbox with several canonical listeners.
- sandbox-backed service with admitted upstream service routes.
- exact local provider selection.
- local-only offline sovereignty.
- same input replay in one process and a child process.

### Edge paths

- IPv4 and IPv6 desired listener addresses.
- exact and provider-assigned host ports.
- TCP, HTTP, and HTTPS endpoint protocol mapping.
- tenant routes and listener arrays supplied in reverse order.
- identical resource names in different tenants.
- same logical resource with changed requested address or port.
- newer generation with unchanged resource content.
- stopped or publication-withheld intent.

### Error paths

- missing deployment generation.
- missing node assignment.
- crossed tenant, workload kind, source name, sandbox owner, or backend.
- stale or mismatched generation.
- invalid or duplicate admitted route.
- duplicate listener or endpoint identity.
- exact selection not registered.
- exact selection registered but unsatisfied.
- provider-managed selection for host-managed need.
- unsupported public or TLS need for a provider without that evidence.
- duplicate readiness requirement.
- equal-generation content conflict.

## Static Proof Obligations

The item helper must fail closed on these checks:

1. The exact compiler and payload owners exist.
2. Exactly one production compiler entrypoint exists.
3. No compiler trait exists.
4. The compiler has no effect, store, clock, environment, filesystem, socket,
   random, async, or provider-handle import.
5. `nimbus-network` keeps only its `nimbus-core` workspace edge.
6. `nimbus-network` imports no tenant, service, sandbox, compute, machine,
   proxy, egress, server, system, cluster, transport, or provider SDK.
7. `nimbus-workloads` imports no service or sandbox crate.
8. No Cargo manifest changes.
9. No new `oci_attachment_plan` caller exists.
10. The compiler assigns no lease epoch.
11. No decision ID, `TenantWorkloadUid`, IP address, port, provider handle, or
    `SandboxId` forms a stable network resource ID.
12. NNC6.2a remains the only owner of durable compiled-plan embedding.
13. NNC6.1e1 remains the only owner of lifecycle ingress cutover.
14. Portable content retains tenant-qualified identity, complete capability
    requirements, exact selection, and every resource needed to derive the
    complete envelope.
15. Construction and strict decoding rederive every plan/resource ID and
    compare digest, plan ID, generation, sovereignty, complete capabilities,
    and readiness.
16. Source-bearing sovereignty is monotonic and cannot be relaxed before
    exact selection.
17. Every completed acceptance row has a recorded command and result.
18. The child-process determinism proof is explicit, bounded, and reaped.

The helper directly evaluates all 18 obligations. Six mutation cases prove
that missing compiler input, missing payload input, OCI caller growth,
decision-bound identity, an uncorrelated envelope, and an uncorrelated resource
ID each fail closed; every missing or empty scan target also fails.

## Verification Plan

Focused implementation loops use these gates:

```text
cargo nextest run -p nimbus-network <NNC6.2 identity filters>
cargo nextest run -p nimbus-tenant <NNC6.2 admission filters>
cargo nextest run -p nimbus-sandbox <NNC6.2 capability filters>
cargo nextest run -p nimbus-workloads <NNC6.2 payload filters>
cargo nextest run -p nimbus-compute <NNC6.2 compiler filters>
```

Candidate closeout uses these gates:

```text
cargo nextest run -p nimbus-network
cargo nextest run -p nimbus-tenant
cargo nextest run -p nimbus-sandbox
cargo nextest run -p nimbus-workloads
cargo nextest run -p nimbus-compute
cargo check -p nimbus-network -p nimbus-tenant -p nimbus-sandbox -p nimbus-workloads -p nimbus-compute --all-targets --all-features
cargo check --workspace --all-targets
cargo clippy -p nimbus-network -p nimbus-tenant -p nimbus-sandbox -p nimbus-workloads -p nimbus-compute --all-targets --all-features --no-deps -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-network -p nimbus-tenant -p nimbus-sandbox -p nimbus-workloads -p nimbus-compute --all-features --no-deps
cargo fmt --all --check
bash scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Each long command uses a bounded timeout. Each result records pass, fail,
ignored, and skipped counts. A skipped environment lane is not a pass.

The originally frozen workspace-wide `--all-features` command is not a valid
macOS local gate: it activates the documented dormant `nimbus-fs/fuse`
scaffolding, whose `fuser` build script requires a system `fuse.pc`; no Nimbus
code is gated on that feature and CI does not build it. The command was still
run and failed exactly at that external package check. Candidate evidence uses
the stronger relevant combination above: all features for every affected
crate, plus every target in the workspace under its supported default feature
set. This is an explicit environment disposition, not a skipped passing lane.

## Review Cadence

NNC6.2 is one review unit.

Run no structured review during audit, fail-before work, implementation,
cleanup, or acceptance convergence. Run exactly one full structured review
after C1-C18 and all gates pass on a candidate-frozen diff.

The review command must use GPT-5.6 Sol with `xhigh` reasoning. It must enable
fast mode. It must not use Opus 4.8. If the review finds an executable defect,
rerun the affected proofs and one narrow correction review. Do not rerun
for proof wording, ledger updates, formatting, or elapsed time.

## Item Status Ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Source and dependency audit | `done` | Three independent read-only inventories plus owner source verification. No paths changed. |
| Prospective split | `done` | NNC6.2, NNC6.2a, and NNC6.1e1 have separate acceptance boundaries. |
| Owner and source allowlist | `done` | This proof freezes compute, workloads, tenant, sandbox, identity, verifier, and plan paths. |
| Expected-red tests | `done` | Missing identity/payload/compiler seams failed to compile; direct tenant admission, permutation, capability projection, and route address-independence failed on their named behavior before correction. F15 remains deliberately red for NNC6.2a. |
| Static verifier expected red | `done` | NNCV028 failed on the missing compiler and records the exact three-file OCI caller baseline. Six mutation cases now add uncorrelated-envelope and uncorrelated-resource-ID proofs to the original missing compiler, missing payload, caller growth, and decision-bound identity cases. |
| Compiler implementation | `done` | One concrete effect-free compute compiler emits the strict workloads-owned payload from admitted source values. |
| Acceptance C1-C18 | `done` | Every row is reconciled after the five accepted review corrections; total affected behavior is `1,477/1,477` with 27 declared skips, and the affected quality/static gates are green. |
| Candidate gates | `done` | Affected checks/Clippy/rustdoc, workspace all-target check, format/diff, Bash/ShellCheck, NNCV028 `18/18` plus `6/6`, aggregate `29/29`, exact `198 + 6 = 204` arithmetic, complete split-bound aggregate coverage, docs 108, and site `17/17` are green after the count-only correction. |
| Full structured review | `done` | One Sol/xhigh/fast item review reported six findings at confidence `0.98`; five accepted executable defects are corrected and one source-contract finding is rejected with evidence. No second full review is allowed. |
| Narrow correction review | `done` | The sole Sol/xhigh/fast correction review confirmed the five behavioral corrections and reported one P3 arithmetic defect: HEAD's 198 cases plus six new counted mutations equals 204, not 203. The finding is accepted; no third review is permitted. |
| Item commit | `pending` | Commit only after the ledger and recovery header contain final evidence. |

## NNC6.2a Handoff

NNC6.2a must make the compiled value durable before lifecycle ingress.

Its first expected-red test must:

1. Persist a saga at `IntentCommitted`.
2. Kill the writer process before network reservation.
3. Start a distinct process with only the Engine root.
4. Decode the saga record.
5. Reconstruct the exact complete compiled plan and resource payload.
6. Compare the bytes and digest with the writer's expected value.
7. Prove zero network command ran before exact reconstruction.

The current tuple-only `WorkloadNetworkIntent` must fail this test. NNC6.2
does not weaken or bypass that expected-red result.
