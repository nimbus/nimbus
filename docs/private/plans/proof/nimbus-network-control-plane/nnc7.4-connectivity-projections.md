# NNC7.4 Connectivity Projections

Status: `complete; A1-A14 green; review cadence exhausted`

## Outcome

Make `_nimbus` connectivity observations use stable network identities and
generation-fenced provider evidence. Keep HTTP protocol-route inventory in
`routes`. Add a distinct `connectivity_routes` table. The projection remains
operator evidence only. It cannot allocate, bind, publish, clean up, or change
desired state.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC7.3 is complete at product commit `864cfda1b6fe3464b62a6131ea32ae650e52fad1`; recovery truth-up is `3d8e621a1e4c003768bedbae91586e1641909c40`. |
| Current scope | F1-F7 and A1-A14 are green. The full and narrow reviews each found four P2 contract defects. All eight are corrected and proven. Review cadence is exhausted. |
| Owned paths | `nimbus-system` connectivity records/schema/keys/exports and concept-owned tests; `nimbus-network` route-ID derivation only if the live endpoint-to-route conversion requires it; `nimbus-machine` canonical SSH lease identity; CLI machine SSH request consumption and mechanical fixtures; server listener evidence/construction/router and affected tests; tenant-drift test conversion; this proof and concise plan/index state. |
| Forbidden paths and seams | No network store mutation or manager in `nimbus-system`; no socket, provider, proxy, naming, certificate, policy, workload-saga, cluster, Netavark, nftables, gvproxy, or transport effect changes. No NNC7.5 rebuild/failure-independence implementation and no NNC8 reconciliation. |
| Acceptance | A1-A14 below. Every projected network resource uses a stable ID and generation. HTTP `routes` cannot collide with connectivity routes. Addresses remain observations. |
| Last green | Final system connectivity `7/7`; machine projection `1/1`; server drift `3/3`; system `78/78`; serialized server `752 + 35 ignored`; strict system/server Clippy and Rustdoc; format and diff; live verifier `36/36`. Network, machine, and CLI full suites plus docs `108` and site `17/17` remain green. |
| Dirty paths | `nimbus-network` identity/binding; `nimbus-machine` state/export; CLI machine ports; `nimbus-system` schema/keys/records/exports and connectivity tests; server listener evidence/construction/router/drift paths; this proof and the canonical plan. |
| Next action | Stage this final checkpoint and create the NNC7.4 item commit. |
| Blocker | none |

## Current source audit

| Owner | Current behavior | NNC7.4 target |
| --- | --- | --- |
| `nimbus-system::inventory` | `routes` is a static HTTP method/path/adapter inventory. The server drift scanner depends on this exact meaning. | Leave `routes` and its callers byte-semantically unchanged. Add `connectivity_routes` as a separate `SystemTable` with a different document-ID prefix. |
| `nimbus-system::records::subscription` | `record_listener_state_async` accepts unvalidated strings and keys a listener by adapter plus protocol. | Replace it with a typed connectivity observation keyed by `ListenerId`. Derive lease, generation, epoch, provider, address, conditions, and cleanup fields from authenticated evidence. |
| `nimbus-system::records::machine` | Machine listener and port records use the machine name as identity and omit lease/generation/provider evidence. | Project the persisted `ssh_listener_id`, its canonical `PortLeaseId`, the machine-owned lease fence/provider identity, and the observed loopback address. Keep the Unix Machine API path as a non-port listener observation, never as network lease authority. |
| `nimbus-system::records` service writer | `record_service_handle_async` consumes address-bearing `SandboxHandle`, keys ports by service/endpoint labels, and has no live product caller outside tests. | Replace it with a validated service-connectivity input carrying attachment/endpoint handles and exact listener/lease/provider evidence. Derive service, listener, port, and connectivity-route documents in one conversion locality. |
| `nimbus-server::Router` | Startup writes one `listening` row for each logical HTTP adapter before the main accept future is supervised. These rows describe protocol registration, not distinct physical listeners. | Delete these logical listener writes. Adapter inventory remains in its existing capability and HTTP-route tables. Project the one physical main listener from its active lease evidence. |
| Server sibling listeners | Each concrete bind has an `ActiveServerListenerLease`, but projection discards that evidence and writes raw strings before group activation. | Preserve the current NNC7.1a setup order while converting the exact immutable lease snapshot to the typed system input. NNC7.5 later owns projection-failure independence. |
| Workload ingress | Exact live bind evidence already authenticates listener ID, port lease ID, generation, lifetime, endpoint ID, provider selection, and actual address. | The system adapter accepts this shape and derives a stable connectivity-route key from endpoint identity. Live rebuild/lag wiring remains NNC7.5. |
| `nimbus-machine` / CLI | `MachineRuntimeState` persists stable SSH listener ID. The CLI separately defines the SSH provider key and fixed lease fence. | Move the pure canonical SSH lease identity/fence/provider derivation to `nimbus-machine`; the CLI remains the gvproxy effect owner and consumes that identity when it builds the request. |

The audit found no canonical connectivity projection elsewhere. `nimbus-system`
already sits above network, machine, sandbox, workloads, and engine. It needs no
new reverse edge. `nimbus-network` remains `nimbus-core`-only and receives no
projection API.

## Frozen contract

1. `SystemTable::Routes` remains HTTP protocol inventory. Connectivity routes
   use `SystemTable::ConnectivityRoutes`, table name `connectivity_routes`, and
   document prefix `connectivity-route:`.
2. Listener documents use `ListenerId`. Port documents use `PortLeaseId`.
   Route documents use `IngressRouteId`. Endpoint and attachment fields use
   their stable network IDs. No address, port, adapter label, service label, or
   filesystem path is a document identity.
3. Numeric generations and lease epochs serialize as canonical decimal text so
   the operator projection cannot lose `u64` precision.
4. A port-backed listener input proves that the listener ID derives its lease
   ID. It also proves the request owner and actual binding. The constructor
   rejects crossed evidence before any write.
5. Only a provider registration ID enters the projection. Opaque
   `NetworkProviderHandle` material never enters a document, error, or debug
   value.
6. The actual address is an observed field. Moving it updates one stable
   document and never changes listener, port, route, endpoint, attachment, or
   service identity.
7. Conditions use the bounded `NetworkCondition` vocabulary. The adapter
   derives cleanup state from the phase and conditions. These values cannot
   contradict each other.
8. The service conversion consumes the source `SandboxSpec` and canonical
   `SandboxProvisionNetworkPlan`. It validates the complete source binding set.
   It also validates attachment, endpoint, listener, lease request, generation,
   application protocol, guest port, and address correlation. It then writes
   the service and its child rows.
9. The service conversion removes only stale child rows that carry that exact
   service document ID. It cannot delete another service's rows.
10. The machine SSH request and projection consume one machine-owned pure lease
    identity. gvproxy bind/readiness/stop effects remain in the CLI manager.
11. The Machine API Unix listener carries the forwarder authority's provider
    registration ID and generation, but never its opaque provider handle.
12. The main and sibling server writers consume immutable active-lease
   evidence. They do not receive a network authority or provider handle.
13. No projection write changes desired state, lease/provider state, service
    resolution, or workload saga state. NNC7.5 owns loss, lag, deletion,
    rebuild, stale-update, and non-blocking failure proofs.

## Acceptance matrix

| ID | Verifiable result | Proof |
| --- | --- | --- |
| A1 | `routes` retains the exact HTTP schema and inventory behavior; `connectivity_routes` exists as a distinct table/kind. | Schema and inventory tests. |
| A2 | HTTP route and connectivity-route document IDs cannot collide, including adversarial separator/case values. | Key-injectivity tests. |
| A3 | Listener, port, route, endpoint, and attachment documents expose stable IDs, decimal generation, provider ID, actual address, conditions, and explicit cleanup state as applicable. | Exact document-shape tests. |
| A4 | Changing only the actual address updates the same listener/port/route document IDs. | Two-write address-movement test. |
| A5 | Crossed listener/lease owner, lease ID, generation, tenant, protocol, binding, endpoint, attachment, or route identity fails before any partial write. | Table-driven constructor/write tests. |
| A6 | Opaque provider-handle text does not appear in stored documents or errors. | Redaction test plus source/type scan. |
| A7 | Service replacement removes only its stale listener/port/route rows and preserves another service's rows. | Multi-service replacement test. |
| A8 | The server main listener produces one physical listener/port observation; logical Convex/Firebase/Cloud Functions/Cloudflare registrations do not create duplicate listener authority. | Focused serving/construction test and caller census. |
| A9 | Each sibling wire listener projects its exact `ListenerId`, `PortLeaseId`, fence, provider ID, and bound address from active lease evidence. | Listener-group construction tests. |
| A10 | Machine SSH listener/port projection uses the persisted listener ID and one machine-owned canonical lease identity; its key survives port/address change. | Machine/system/CLI tests. |
| A11 | Machine API Unix-listener observation remains structurally distinct from a host-port lease and never fabricates a `PortLeaseId`. | System machine projection test. |
| A12 | No product caller remains for raw `record_listener_state_async` or address-only `record_service_handle_async`. | Source census. |
| A13 | `nimbus-system` imports no local network store/manager/authority mutation type; `nimbus-network` retains only the `nimbus-core` workspace edge. | Source scan and `cargo metadata`. |
| A14 | Full affected suites, format, strict affected Clippy/Rustdoc, live verifier, proof lint, docs gates, one candidate-frozen full Sol/xhigh/fast review, and the single correction review authorized by accepted executable findings complete. | Closeout ledger with exact counts and dispositions. |

## Fail-before packet

| ID | Expected-red result before implementation |
| --- | --- |
| F1 | Looking up `connectivity_routes` fails because no separate table exists. |
| F2 | Two different listener IDs with the same address overwrite the same label-derived listener row. |
| F3 | A listener row cannot expose stable lease ID, generation, epoch, provider ID, bounded conditions, or cleanup state. |
| F4 | A service endpoint address change produces label-derived child identities without a stable endpoint/route correlation contract. |
| F5 | Crossed listener and lease evidence can be passed to the raw string writer because it has no validation boundary. |
| F6 | Machine SSH rows omit the persisted listener ID and canonical lease/provider fence. |
| F7 | Router preparation creates multiple logical adapter listener rows for one physical socket. |

## Verification ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Source/owner audit | `done` | Current schemas, keys, writer callers, server active-lease evidence, workload ingress observation, service projection, machine state, CLI machine lease authority, drift consumers, dependency graph, active plan routing, and file-size thresholds inspected. |
| Frozen contract | `done` | A1-A14 and F1-F7 above; one coherent item retained. |
| Fail-before | `done` | `cargo test -p nimbus-system connectivity --no-run` exited `101` before implementation because the distinct table, stable key functions, typed observation inputs, and route derivation did not exist. The compile errors map to F1-F7 and did not expose an unrelated baseline failure. |
| Implementation | `done` | A1-A13 are green. System connectivity `7/7`; complete system `78/78`; server router, machine lifecycle/config, main/sibling, and drift focus pass; obsolete writer callers, projection authority/store types, and opaque provider handles are absent; the only `nimbus-network` workspace edge remains `nimbus-core`. |
| Candidate gates | `done` | Final focused `7 + 1 + 3`; system `78`; serialized server `752 + 35 ignored`; strict system/server Clippy and Rustdoc; format/diff; live verifier `36/36`. Earlier affected suites and static scans remain green. |
| Item review | `done` | The full review reported four P2 findings at confidence `0.97`. The one permitted narrow review reported four incomplete-correction P2 findings at confidence `0.98`. All eight are corrected and proven. Review cadence is exhausted. |
| Commit | `in_progress` | Create one exact item commit. No push or PR. |

## Full-review dispositions

The candidate-frozen review used GPT-5.6 Sol with xhigh reasoning and fast
mode. Two earlier wrapper attempts stopped before model invocation. Repository
cadence did not enable the item gate. The wrapper also rejected the private
proof as a dataset. They are not reviews.

| Finding | Disposition and proof |
| --- | --- |
| Service observations did not bind stable identities to the owning service. | Accepted. `SystemServiceConnectivityObservation` now consumes the source `SandboxSpec` and its canonical `SandboxProvisionNetworkPlan`. It retains the exact lease request in port evidence and rejects crossed attachment, endpoint, listener, lease, generation, application protocol, or guest-port correlations before a write. The table-driven crossed-service case passes in connectivity `7/7`. |
| Drift scanning accepted fabricated lease and fencing evidence. | Accepted with one evidence correction. The scanner retains each manifest plan's exact endpoint, listener, lease, generation, epoch, and transport tuple; parses canonical projected IDs; and compares service/port evidence plus the durable plan. A crossed epoch produces `system_port_record_plan_mismatch`. The provider registration is compared across service and port projections, but not against the plan because `SandboxProvisionNetworkPlan` is deliberately provider-neutral and contains no ingress-provider evidence. Inventing that authority would violate the frozen seam. Server drift passes `3/3`. |
| HTTP service ports were compared at the wrong protocol layer. | Accepted. The clean drift fixture now uses an HTTP endpoint over its planned TCP transport. The scanner compares the port row with the plan's `PortProtocol`, while the endpoint and route retain `EndpointProtocol`. The clean scan passes. |
| Machine API listener omitted available provider identity. | Accepted. The Unix observation projects `forwarder_authority.provider_instance().provider_id()` with the authority generation and still omits the opaque handle and every `PortLeaseId`. The exact machine projection test passes `1/1`. |

## Narrow-review dispositions

The sole narrow review used GPT-5.6 Sol with xhigh reasoning and fast mode.
It reviewed staged tree `368a79b12202a104378853049634b1f81f3b279d` and patch
SHA-256 `f7f20b3fece7590104aa05eb1b9a295af81c62fab2ca72b3df246f60cf58b7d5`.
The review thread is `019ffc37-8e52-7142-896b-4c9d6ff3a73a`.

| Finding | Disposition and proof |
| --- | --- |
| The service constructor did not correlate the plan with source bindings. | Accepted. It now requires an exact, unique match for every source binding and planned listener before it accepts attachment or endpoint evidence. A same-tenant crossed source with a valid plan and endpoint fails before any write. Connectivity passes `7/7`. |
| A service projection could add an endpoint without a port child. | Accepted. The scanner compares every projected service endpoint directly with the durable manifest plan. An unexpected endpoint produces `system_service_endpoint_plan_mismatch` even when no port row refers to it. Drift passes `3/3`. |
| The scanner discarded the service attachment generation. | Accepted. It parses the top-level decimal generation and compares it with the manifest plan generation beside attachment identity. Crossed generation produces `system_service_handle_attachment_mismatch`. |
| Reserved and provisioning port evidence bypassed plan validation. | Accepted. Every nonterminal network phase now requires the durable endpoint, listener, lease, generation, epoch, and transport tuple. A provisioning row with a crossed epoch produces `system_port_record_plan_mismatch`. |

NNC7.4 needs no further structured review.

## Corrected affected evidence

- `cargo test -p nimbus-system connectivity -- --nocapture`: `7/7`.
- `cargo test -p nimbus-system record_machine_state_projects_machine_listener_and_port_documents -- --nocapture`: `1/1`.
- serialized tenant-drift focus: `3/3`.
- full `nimbus-system`: `78/78`.
- serialized full `nimbus-server`: `658 + 35 ignored` unit tests plus
  `27 + 23 + 2 + 4 + 36 + 2` integration tests, or `752` passes and `35`
  declared ignores.
- strict affected Clippy and warning-denied Rustdoc: pass.
- format, diff, and live architecture verifier: pass. The verifier reports
  `36/36`.

The handwritten drift scanner is `1,990` lines. It owns the read-only
tenant-isolation reconciliation concept. The new plan-correlation records stay
beside the parsing and comparison invariant that they protect. It remains below
the mandatory 2,000-line extraction threshold. The concept-owned connectivity
adapter and its tests are `909` and `907` lines.
