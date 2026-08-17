# NNC7.3 Portable Provider Status Handles

Status: `done; A1-A12 green; review cadence exhausted`

## Outcome

Return portable endpoint and attachment handles from sandbox and machine
status. An address change must not change resource identity. Provider handles
must remain opaque and redacted outside their effect owner.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC7.2 is complete at `d031a1bdeee0e322fa822254668a1f971169067d`. |
| Current scope | Frozen portable attachment/endpoint status substitution plus the accepted A3 correction that authenticates the canonical listener-to-endpoint mapping independently of provider reservation input. |
| Owned paths | `nimbus-network` portable handle source/export; sandbox inspection/provision/state/inspection adapters and concept-owned tests; compute provision-plan construction and projection validation/tests; Machine API status DTO/tests; CLI Machine API provision/status adapters/tests; this proof and concise plan/routing state. |
| Forbidden paths and seams | No provider effect, transport, tenant policy, logical naming, NNC7.4 projection schema, or NNC8 recovery change. |
| Acceptance | A1-A12 below. Address change does not change resource identity. Opaque provider handles do not enter portable status, diagnostics, or Machine API wire data. |
| Last green | Focused crossed-construction/decode `1/1`; compute provision `3/3`; full sandbox `1,172 + 45 ignored`; compute `472 + 1 ignored`; CLI `1,007 + 4 ignored`; serialized server `752 + 35 ignored`; strict affected quality; live verifier `36/36`; docs `108`; site `17/17`; proof lint zero. |
| Next action | None. NNC7.3 is complete; NNC7.4 owns the next read-only projection audit. |
| Blocker | none |

## Frozen source audit

| Current owner | Current state | Target for NNC7.3 |
| --- | --- | --- |
| `nimbus-network` | `PublishedEndpointHandle` separates endpoint ID and generation from an observed address. Durable `NetworkProviderHandle` is opaque and diagnostic-redacted. There is no portable attachment handle. | Add `NetworkAttachmentHandle` with only `NetworkAttachmentId` and `NetworkResourceGeneration`. Do not include provider identity, provider handle, address, status authority, or effects. |
| `nimbus-sandbox` provision | `SandboxProvisionNetworkPlan` already persists exact tenant, plan, generation, and attachment identity. Each listener persists listener and lease identity, but not its compiler-issued endpoint ID. | Persist the existing compiler-issued `PublishedEndpointId` on each provision listener. Reject duplicate endpoint IDs. Never derive one from an address or `SandboxId`. |
| `nimbus-sandbox` observation | `SandboxHandle` and `SandboxInspection` expose only address-bearing `PublishedEndpoint` values. Exact Container/Krun manifests retain the provision plan and actual attachment ID. | Add one validated `SandboxNetworkStatus` to inspection. It contains an optional portable attachment handle and portable endpoint handles. Handle-only external/test providers report no exact network status. |
| Container/Krun state views | Persisted summaries deserialize only the coarse handle, specification, and lifecycle fields. | Read the already-persisted plan and attachment ID, then emit the same validated portable status. Crossed durable evidence fails the status read. |
| Machine API | Inspect transports `SandboxInspection`; list/lookup summaries copy address-only endpoints. | Transport the sandbox-owned portable status in inspect and summary responses. Machine remains a wire adapter and creates no identity. |
| `nimbus-compute` | The compiler already owns exact attachment/endpoint IDs. Projection validates execution and ingress independently, then creates endpoint handles from ingress bind evidence. | Authenticate sandbox status attachment/endpoint identities and generation against the compiled plan. Continue to use ingress bind evidence for reachable publication. Compare stable identities, not addresses. |

`OciAttachmentReadinessEvidence` remains provider-authenticated readiness
evidence. Netavark, IPAM, nftables, gvproxy, WSL2, socket, and forwarding
effects remain in their present owners. `nimbus-services` keeps logical naming
and readiness. NNC7.4 keeps system projection schemas.

## Frozen contract

1. `NetworkAttachmentHandle` is the portable resource reference. Its identity
   tuple is `(attachment_id, generation)`.
2. `PublishedEndpointHandle` remains the portable endpoint reference. Its
   identity tuple is `(endpoint_id, generation)`. `endpoint.address` is only an
   observation.
3. `SandboxNetworkStatus` contains rebuildable observations. It grants no desired,
   lease, attach, publish, cleanup, or provider-effect authority.
4. A non-empty endpoint set requires an attachment handle. Endpoint IDs and
   names are unique, and every endpoint generation equals the attachment
   generation.
5. Exact plan projection also proves the attachment ID, endpoint IDs,
   names, protocols, and guest ports against the persisted provision plan.
   It does not use the observed host address or host port as identity.
6. A provider-reported inspection without an exact persisted network plan
   carries no portable network status. Compute fails closed on missing, stale,
   or crossed evidence for an active compiled network plan.
7. `NetworkProviderHandle` remains durable provider-owner material. Portable
   status contains neither that type nor its opaque value.

## Owned and forbidden seams

Product ownership covers only these paths:

- `crates/nimbus-network/src/attachment_handle.rs` and its `lib.rs` export.
- `crates/nimbus-sandbox/src/{inspection,provision,lib}.rs`, Container/Krun
  inspection and persisted-state adapters, and their concept-owned tests.
- `crates/nimbus-compute/src/workload_saga/provision_sandbox.rs` plus workload
  projection validation and tests.
- `crates/nimbus-machine/src/api.rs` and status-wire tests.
- `crates/nimbus-cli/src/machine/api/service_workloads/provision.rs`,
  `machine/api/state.rs`, and their status/provision tests.

Forbidden changes include provider effects, durable provider handles,
Netavark/IPAM/nft/gvproxy behavior, sockets, and transports. They also include
tenant policy, service naming/readiness, certificate authorities,
`nimbus-system` projection schemas, cluster transport, and NNC8 recovery.

## Acceptance matrix

| ID | Verifiable result | Proof |
| --- | --- | --- |
| A1 | `NetworkAttachmentHandle` round-trips with only stable attachment ID and generation. | Focused `nimbus-network` unit test and source/type inspection. |
| A2 | Changing an endpoint address preserves its endpoint and attachment identity tuples; changing either ID or generation does not. | Deterministic network/sandbox unit tests. |
| A3 | Compute and forwarded guest compilation supply the existing blueprint endpoint ID; sandbox provision rejects duplicate or crossed endpoint identity. | Provision-plan constructor tests and source scan for address-derived identity. |
| A4 | `SandboxNetworkStatus` rejects endpoint-without-attachment, duplicate ID/name, mixed generation, unknown endpoint, protocol mismatch, and guest-port mismatch. | Focused sandbox validation table. |
| A5 | Exact Container and Krun inspection return the persisted attachment handle and only currently visible endpoint handles. Non-ready/withdrawn endpoints stay empty. | Existing inspection fixtures plus new exact-status assertions. |
| A6 | Container and Krun persisted state summaries return the same portable identity and fail closed for crossed plan/config evidence. | State-view tests over real manifest JSON. |
| A7 | Machine inspect, list, and lookup JSON round-trip portable status without deriving or rewriting identity. | `nimbus-machine` wire tests and CLI Machine API state tests. |
| A8 | Compute rejects missing, stale, crossed, partial, or unexpected sandbox network status before projection. | Workload projection rejection tests. |
| A9 | Compute accepts an address change when the exact attachment/endpoint IDs and generation remain unchanged, while ingress bind evidence still owns the published address. | Workload projection address-change test. |
| A10 | No portable sandbox/machine status field contains `NetworkProviderHandle`; serialized status and diagnostics contain no opaque provider value. | Type/source scan plus redaction/wire tests. |
| A11 | `nimbus-network` retains only the `nimbus-core` workspace edge and contains no provider effect. | Dependency/effect verifier and package metadata check. |
| A12 | Full affected behavior, format, strict Clippy/Rustdoc, architecture verifier, proof lint, docs gates, and one candidate-frozen Sol/xhigh/fast item review pass. | Closeout command ledger with exact counts and review disposition. |

## Fail-before cases

| ID | Expected-red behavior before implementation |
| --- | --- |
| F1 | A portable attachment-handle construction and wire round-trip cannot compile because the type does not exist. |
| F2 | A provision listener cannot carry or validate its compiler-issued endpoint ID. |
| F3 | Sandbox status cannot prove address-independent attachment/endpoint identity or reject crossed internal status. |
| F4 | Container/Krun and Machine API summaries expose only address-bearing endpoints. |
| F5 | Compute projection cannot authenticate exact sandbox attachment/endpoint status against its compiled plan. |

## Verification ledger

The tests captured each expected-red case before the implementation added its
owner behavior.
The implementation spans only the frozen network, sandbox, compute, Machine
API, CLI, server-fixture, plan, and proof paths.

| Checkpoint | Result |
| --- | --- |
| Portable attachment handle | Focused `nimbus-network` round-trip test passed `1/1` after the missing-type fail-before. |
| Sandbox validation and exact providers | Status validation, duplicate and crossed endpoint identity, exact Container/Krun inspection, real persisted-state projection, crossed evidence, and withdrawn endpoint cases pass. Corrected full sandbox passes `1,172`. The suite ignores `45` declared environment or child-process cases. |
| Compute authentication | Missing, stale, crossed, partial, and unexpected status fails before ingress. Exact stable identities accept address movement. Full compute passes `472`. The suite ignores one child-process case. |
| Machine and CLI wire | Machine inspect/list/lookup and live CLI list/current round-trip portable status without opaque values. Full machine passes `48`. Full CLI passes `1,007`. The suite ignores four declared cases. |
| Remaining full affected behavior | Full network passes `276` with one declared ignore. Final serialized server passes `752` with `35` declared ignores. |
| Quality | After the correction, `cargo fmt --all --check`, `git diff --check`, strict all-target Clippy for sandbox/compute/CLI/server, and warning-denied Rustdoc for the same crates exit `0`. Vendored Brotli compile warnings remain dependency output and do not defeat the strict Nimbus gates. |
| Dependency and authority | `cargo metadata --format-version 1 --no-deps` reports `nimbus-network -> nimbus-core` as its only workspace edge. The portable-status source scan finds no `NetworkProviderHandle`. The live architecture verifier passes `36/36`. |
| Proof and docs | After the correction, strict proof lint passes one file with zero diagnostics. Docs pass `108` pages. The site passes `17/17` conditions. |
| Candidate review | One full GPT-5.6 Sol/xhigh/fast review ran on tree `eb3609f339312ef2a86f35320f6242a794a449eb`, patch SHA-256 `924fbd0e34c2c57e9430119ddcc98dafa263537a9c9971bb0eec45a005d52893`, thread `019ffb9f-8a73-7a13-a8b2-1e56884d40b3`. It accepted one P2 A3 endpoint-correlation defect at confidence `0.97`. The sole narrow correction review ran on tree `cb2bc40e162b576b99edaec82e23f559a5069b16`, patch SHA-256 `820e2a28dfc5ced8978982eafa5a46b354c2585d6cbed4450fa8e786a961055b`, with GPT-5.6 Sol/xhigh/fast. It reports no finding and rates the correction correct at confidence `0.98`. Review cadence is exhausted. |

## Acceptance convergence findings

The full affected gates found two contract gaps in shared test providers. They
did not find a second product authority.

1. The first full compute run rejected 32 shared exact-provider fixtures that
   returned no portable status. One compute test-support projection now builds
   exact status from the request plan and provider-visible endpoints. The final
   compute suite passes `472 + 1 ignored`.
2. The first full CLI run passed `998`, failed `9`, and ignored `4`. All nine
   failures used one status-less Compose retirement provider. One shared fixture
   correction made the focused `9/9` and final `1,007 + 4 ignored` gates pass.
3. A non-serialized server invocation passed `615`, failed `43`, and ignored
   `35`. Every failure was the expected duplicate-process-authority guard. The
   required serialized run then exposed 16 status-contract failures in two
   shared providers. One canonical managed-provider observation helper plus an
   exact tenant-harness guest-port projection made focused service-manager
   `37/37`, tenant conformance `1/1`, and final server `752 + 35 ignored` pass.
4. The first live verifier run passed `35/36` because the in-progress ledger
   row lacked the literal `Owned paths` recovery field. The corrected recovery
   checkpoint passes `36/36`.

These corrections stay test-only except for the frozen NNC7.3 product changes.
They make fake providers honor the same exact observation contract as real
Container and Krun providers. They do not mint provider effects from desired
state in production.

## Item review disposition

The full item review found one accepted P2 defect. The provision envelope
validated endpoint uniqueness, but it did not authenticate each listener's
endpoint against an independent compiler-owned mapping. Provider reservation
input could therefore cross two unique endpoint identities before sandbox or
Machine status.

The correction adds a canonical listener-to-endpoint identity map to the
provision envelope. Compute and forwarded-guest lowering derive the map from
the compiled plan, separately from provider reservation listeners. The
sandbox constructor rejects missing, duplicate, or crossed mappings. Custom
deserialization calls the same constructor, so persisted JSON cannot bypass
the invariant. The focused construction and decode regression passes `1/1`.
The focused compute provision suite passes `3/3`.

The correction gates pass without another product finding. Sandbox passes
`1,172` with `45` ignored. Compute passes `472` with one ignored. CLI passes
`1,007` with four ignored. Serialized server passes `752` with `35` ignored.

Strict affected Clippy and Rustdoc pass. The live architecture
verifier passes `36/36`. The crate metadata still reports only
`nimbus-network -> nimbus-core`, and the portable status scan contains no
`NetworkProviderHandle`.
