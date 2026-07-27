# NNC3.4 Sandbox, PEP, And Machine Port Migration Proof

Date: 2026-07-24; updated 2026-07-27

Status: `complete; final actual-Sol review dispositioned, all written acceptance criteria pass, and the current commit is the NNC3.4 completion checkpoint`

Starting checkpoint:
`4c63c5ba963b1b310b8efdd692a6f2ea019e7df3`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The sandbox endpoint, per-sandbox egress PEP, and OCI
`MachinePortProxy` paths now consume the one crash-safe, cross-process
`LocalPortLeaseAuthority`. Allocation authority remains in
`nimbus-network`; sockets, Netavark, gvproxy, namespace, proxy, and provider
effects remain in their existing owner crates.

The migration has one authority boundary:

1. sandbox composition derives a tenant-scoped, address-free `ListenerId` and
   `PortLeaseId`;
2. one store transaction reserves every explicit and image-derived published
   endpoint plus the launch's internal PEP listener in caller order;
3. the complete immutable `PortLeaseRequest` set, including exact
   transport-free host publication intent, is persisted in the OCI manifest;
4. the effect owner proves the request still belongs to the exact
   tenant/sandbox/listener generation and durable record;
5. every Nimbus-owned socket bind first owns an attempt-unique durable
   `PortBindClaim`, so an exact-request replay cannot fail, release, or adopt
   another attempt's effect;
6. a successful real effect is adopted with concrete endpoint/provider
   evidence plus the exact historical attempt receipt and is activated only by
   that claim before its accept loop or workload publication; and
7. final teardown withdraws first and releases only after provider and
   namespace removal are confirmed.

The Pass-33/34 convergence audit tightened that boundary without moving
provider effects into `nimbus-network`:

- conmon inspection distinguishes explicit runtime absence from an unknown
  command failure, so a diagnostic failure cannot manufacture cleanup
  authority;
- initial launch cleanup carries a typed convergence state and persists
  `Stopping` while exact provider or pre-effect cleanup remains pending;
- krun creates and durably acknowledges the no-replace claim manifest before
  placement, records `Adopting` before allocator adoption and `Adopted`
  afterward, and holds one lifecycle lock across every inspect/start/stop
  effect plus the corresponding durable publication;
- krun persists cleanup or shutdown intent before sending a stop signal,
  detaching a provider, or releasing exact authority, and ambiguous manifest
  publication uses exact readback plus parent-directory sync before retry;
- machine forwarding joins the first copy worker if the second worker cannot
  start, and its bounded accept proof waits on the semantic provider state
  rather than a fixed polling count; and
- the old test-only `ReservedPortBindings` compatibility adapter is deleted,
  leaving the complete `ReservedLaunchPorts` request set as the only launch
  reservation vocabulary.

The stable identity derivation includes the validated tenant, sandbox
incarnation, and logical listener name. It never includes an IP address or
numeric port. Equal tenant-local sandbox IDs therefore cannot alias
host-global authority, and a manifest cannot borrow a current lease belonging
to another tenant, sandbox, or logical listener.

## Atomic Reservation And Manifest Authority

`LocalPortLeaseAuthority::reserve_batch` evaluates the ordered request group
inside the network store's single lock and transaction. Any port conflict,
range exhaustion, or divergent identity aborts the complete group without
publishing an earlier reservation. Identical replay returns records in caller
order.

`PortManager::reserve_launch_ports_for_sandbox` uses that batch for explicit
bindings, image-derived ports, and the internal PEP listener. Production
allocation and quota enforcement no longer scan sandbox manifests. Admission
supplies an explicit tenant and maximum; durable
`PortLeaseAccounting::{TenantPublished, HostInternal}` classifies usage, and
the port authority counts every same-tenant nonterminal published request
inside the same transaction that reserves new requests. Exact replay adds zero
usage even after a limit is lowered, PEP listeners do not consume publication
quota, and a crash-retained reservation remains counted without any manifest.
Policy ownership stays above network: the store enforces the caller-supplied
decision but never chooses its value. If later deterministic planning fails
before any provider adoption, one authenticated all-or-nothing compensation
transaction releases the complete batch. If any member has reached provider
adoption, the transaction rejects the entire compensation without partial
release. Terminal `Failed` members carry confirmed no-effect evidence, so they
remain unchanged while the same transaction releases still-`Reserved`
siblings; they cannot strand unrelated capacity.

Execute-mode container and krun manifests persist the complete endpoint
request set. Plan-only rendering returns deterministic preview bindings but an
empty request set and makes no port-authority mutation. The runner-owned
transition re-reserves preview-generated listeners as range requests rather
than converting their rendered numbers into exact authority, then updates the
manifest, visible endpoint status, and bundle from the selected ports before
it creates the PEP assignment. Before the first runner provider effect, the
handoff durably changes the manifest from `PlanOnly` to `Execute` while
retaining the exact launch claim. The effect-owning runner performs final
withdraw/stop/detach/release when the workload exits, persists the exit code
and `Stopped` status, and leaves crash-ambiguous authority fenced for NNC3.8
instead of letting an upper plan-only observer claim false provider absence.
It releases the handoff lock only for the workload wait. Exit and wait-failure
finalization then reacquire that exact cross-process lifecycle lock, reread the
current durable manifest, authenticate immutable execution identity plus the
`Execute/LifecyclePublished` decision, and hold the lock through any cleanup
and terminal persistence. An ordinary stop that already published a terminal
manifest wins: the runner adopts it without a second cleanup or stale write.
Changed identity fails before effects or persistence. Decision and phase
publication use one bounded staging name per family, reconcile crash residue
under the lock, and fsync the parent after unlink; injected post-create and
post-write/pre-sync failures leave no stage or false decision and retry
cleanly.
Serialized container manifests must carry the
canonical operator-requested binding list. The runner recomputes the complete
ordered automatic suffix from that list plus the image-exposed-port metadata,
and rejects a missing field, a truncated rendered list, a noncanonical suffix,
or an explicit binding forged as automatic before reserving any listener.
Missing, truncated, cross-tenant, cross-sandbox, cross-listener, or wrong-port
request sets fail before Netavark namespace/provider effects.

Container and krun host-preflight failures, plus krun VM-config
materialization failures, run the same
never-bound compensation before returning. They also finalize the
identity-fenced segment hold through a no-provider-effect path that performs no
bridge, namespace, or transport cleanup call. The original launch failure
remains primary, and every compensation failure is retained in the returned
diagnostic. Container and krun planning now apply that segment compensation to
every error after execute-mode placement: internal-listener derivation, atomic
port admission/reservation, and all later bundle/launch-plan construction.
If exact never-realized segment finalization fails after its last attachment
was removed, the durable allocation retains the reservation claim and
reconstructs the same identity/epoch cleanup token only for that claim.
Foreign and generic retries remain byte-unchanged and fenced.

## PEP Prepare, Adopt, And Activate

`nimbus-proxy` now exposes `PreparedWorkloadPep`, a narrow socket-effect seam:

- `prepare` validates configuration, binds, retains, and configures the real
  listener without starting the accept loop;
- only after owning that socket does sandbox publish the trust anchor, validate
  the exact logical/durable lease, record the concrete listener address and
  sandbox-owned PEP provider handle, and activate the lease; and
- only then does `start` launch request handling.

The proxy crate still owns forwarding, policy enforcement, TLS interception,
decision logging, and the accept loop. `nimbus-network` owns none of them.
`EgressEngine::with_attachment` lets the sandbox registry compare its
caller-owned lifecycle evidence without exposing it to request handling.
Concurrent or replayed registration succeeds only for the same request;
another listener's lease cannot borrow the PEP registration.

Permanent teardown accepts only an exact process-local attachment and
atomically replaces the running entry with an engine-owned `Stopping`
tombstone. The tombstone denies readiness and replacement while retaining the
mutable PEP handle, exact lease/binding evidence, trust-anchor path, tenant
fairness pin, disposition, and cleanup progress through every fallible step.
Release fences new use before shutdown; both release and restart require an
explicit retryable `WorkloadPep::shutdown` acknowledgement before deleting the
anchor. Final teardown then releases the exact request; restart performs a
fenced `Active -> Reserved` transition that preserves the selected numeric
port but clears obsolete provider binding/claim evidence for the next normal
claim/bind/adopt/activate sequence. Only after the durable transition completes
does the engine remove the tombstone. Interrupted cleanup resumes the same
evidence, and a completed release is never driven backward merely because
tombstone removal was interrupted.

If final registration commit fails after activation while another lifecycle
already occupies the workload ID, the engine does not overwrite the primary
or return a live PEP to an error branch. It atomically installs a separate
quarantine tombstone containing the exact provider handle and attachment.
Readiness, request-facing PEP access, and replacement registration fail closed
while any quarantine exists. Cleanup selects the one cell whose exact durable
lease matches the caller, and completion removes only that cell; the
pre-existing primary remains byte-for-byte and lifecycle-independent. A
secondary shutdown, trust-anchor, or lease-transition failure releases only
the cleanup executor token, not the retained evidence, so an exact retry can
resume.

A fresh or overlapping registry with only a persisted manifest locator has no
provider-stop evidence: it returns an explicit ambiguity without withdrawing,
deleting, or releasing. An absent registry is idempotent only when exact
durable inspection proves the request is terminal `Failed` (confirmed no
effect) or `Released` (previous confirmed teardown); every non-terminal phase
remains ambiguous and fenced. A process crash still loses process-local stop
evidence and remains fenced for NNC3.8 rather than manufacturing absence.

A real external listener collision produces structured
`EgressProxyError::BindFailed`, is translated into durable
`AddrInUse`/no-effect evidence, cannot register or activate a PEP, and never
mutates the shared trust-anchor path because it never owned the socket. An
independent overlapping registry losing that collision therefore leaves the
live PEP's anchor byte-identical and its registration intact.

Every fallible PEP preparation first owns both the process-local engine
registration slot and an attempt-unique durable bind claim. A concurrent
same-request invocation can therefore neither compensate nor release another
preparation. Failures before socket ownership cannot mutate the trust anchor.
A trust-root write failure after socket preparation removes only the anchor
written by that attempt while the owned listener remains exclusive, then drops
the listener and abandons the exact claim. An adoption failure follows the
same provider-before-authority cleanup but treats the durable outcome as
ambiguous and retains the request.

`Reserved` is lifecycle state, not release authority. Only the launch
coordinator's explicit, non-persisted `FreshLaunch` capability may release a
request after anchor cleanup, listener removal, and exact claim abandonment
all succeed. Direct PEP helpers and restart reconstruction carry `Retain`, so
their proven no-effect failures leave the exact numeric reservation fenced for
retry. Dedicated proofs establish both sides: a fresh launch retires an invalid
pre-bind preparation as `Released`, while the direct unwritable-root and
restart-policy failures remain `Reserved`. An external bind collision remains
terminal `Failed` evidence.

## Netavark, MachinePortProxy, And Teardown

Both OCI backends validate the complete current endpoint lease set before
creating a persistent namespace or invoking Netavark. After successful
Netavark setup, the sandbox adapter adopts the concrete endpoint observations
and activates the requests. A failure during adoption tears the provider and
namespace effects back down.

Every persisted `OciNetworkConfig` also carries the exact
`NetworkReservationClaim` that reserved its attachment, segment hold, IPAM,
and launch port batch. Live IPAM allocations and released-allocation
tombstones are keyed by stable `NetworkAttachmentId` and retain that immutable
claim plus the stable segment ID. Setup requires exact live evidence.
Teardown authenticates the exact live generation and returns its addresses in
one read, or authenticates the exact terminal tombstone and performs no
provider replay. A new allocation atomically supersedes the old tombstone, so
an earlier generation cannot borrow replacement live or terminal evidence
even when the same attachment and IPv4 address are reused. The tombstone is a
comparison witness for the owning cleanup saga, not a generic never-bound
cleanup capability, and no IP address is workload or generation identity.

Container and krun cleanup authenticate that network generation before
touching runner pointers, PEP or machine listeners, port leases, segment
quarantine, namespaces, Netavark, status projections, or network authority.
Successful Netavark detach precedes confirmed IPAM release and namespace
removal. An ambiguous detach retains the namespace and exact evidence for
inspect-before-retry instead of manufacturing absence.

The provider-managed machine path validates the same logical and durable
authority before `MachinePortProxy` binds. The complete batch first retains
inert real listeners; only after exact durable adoption and activation may the
accept loops start. Gvproxy publication completes under the same process-local
lifecycle lock as validation, start, registration, and withdrawal, so teardown
cannot withdraw between `Active` validation and publication. The exact
external host address is immutable gvproxy publication intent authenticated by
the lease request and normalized route plan; it is distinct from the
process-local provider socket. `MachinePortProxy` binds and records the IPv4
wildcard guest listener that gvproxy targets, and the lease conflict target
describes that concrete wildcard socket. The exact external publication
endpoint remains separately authenticated and cannot be substituted, but it is
not a second listener-overlap model and does not use an IP address as identity.
A specific-address PEP therefore conflicts only according to the portable
address/family overlap model, while the machine guest listener deliberately
retains numeric-global wildcard exclusion. A real internal
`AddrInUse` collision becomes durable failed-bind evidence against the actual
attempted socket and leaves no registered proxy. If a later binding
in a proxy group collides during inert preparation, every already-bound inert
sibling is dropped before one atomic no-effect compensation retires the
still-`Reserved` siblings; the collision member remains `Failed`.

Caller sandbox identity must equal the manifest handle identity, and the
manifest handle tenant must equal the spec tenant before publication or any
provider effect. The registry also retains and compares the normalized
`bind_addr -> guest target_addr` route plan, not only lease IDs and proxy
count, so a changed assigned IP or guest target cannot reuse a stale live
forwarder.

The process-local registry retains the exact request set, concrete bindings,
normalized routes, and provider handles, not merely the tenant/sandbox key.
Teardown atomically replaces a matching `Running` registration with an
engine-owned `Stopping` tombstone. It closes the accept listeners, signals and
joins every tracked bidirectional worker, then records each exact external
unexpose acknowledgement without discarding either the provider handles or
durable binding evidence. Replacement startup and a different teardown
disposition fail closed while the tombstone exists. Only after every provider
and publication effect confirms absence does one atomic batch transition
rebind all restart leases or release all final leases, after which the exact
tombstone may be removed. Partial listener startup returns every already
running handle to the same compensation path instead of losing it.

A fresh overlapping registry cannot treat its empty map as provider absence:
teardown must prove the exact local provider registration before durable
withdrawal, so it leaves the live request `Active` and the original socket
occupied until the owning registry confirms drain and stop.
The matching-registry fast path revalidates exact durable `Active` provider
evidence on every use, so concurrent withdrawal cannot preserve stale serving
authority. An absent registry is idempotent only when every exact batch member
is `Failed` or `Released`; mixed or non-terminal batches remain explicit
reconciliation inputs.

Activation failure drops the complete inert listener batch before changing any
claim. A proven pre-commit error atomically abandons the exact claim set. If
claim abandonment rejects after a lost activation acknowledgement, the adapter
inspects the complete exact `Active` binding set and uses the confirmed-stop
rebind transition only after every inert socket is gone. If neither exact
outcome can be authenticated, compensation fails closed and retains the
durable fence. Deterministic proofs cover both ordinary activation failure and
the commit-succeeded/acknowledgement-lost branch, including same-manifest
retry.

Accepted-connection setup and forwarding failures close only that connection.
They emit connection-local diagnostics but cannot mutate the provider-wide
shutdown signal or return a fatal completion to the accept loop. A live
listener proof injects failure into its first accepted connection, observes a
second accepted connection on the same socket, and then verifies explicit
provider shutdown still wakes and joins cleanly. Accept, worker-spawn, and
thread-panic failures remain provider-level outcomes. The idle accept loop
reaps completed workers on every bounded poll, so a worker panic stops the
listener without requiring later traffic. A retained exited process-local
provider fails closed immediately before publication; fresh-process
provider/effect reconciliation remains NNC3.8.

PEP and machine socket attempts use attempt-unique, redacted
`PortBindClaim`s. Claims are acquired durably and atomically for a machine
batch before any bind. Claimed adoption, no-effect failure, and abandonment
accept only that exact provider attempt. Activation clears the live
effect-creation claim but retains an exact historical `adoption_claim` for as
long as the binding exists. Single activation and atomic Active-batch replay
must authenticate that receipt; a foreign opaque attempt from the same
provider registration fails without rewriting any sibling or authority byte.
A crash leaves the request `Reserved` with its live claim fenced for NNC3.8
rather than allowing a replay to manufacture terminal evidence for the
in-flight effect.
Both Netavark and MachinePortProxy no-effect abandonment now pass one shared
adapter preflight: the configured manager mode, batch cardinality, and every
claim provider are authenticated before reservation lookup or mutation.

Final container and krun teardown follows the safety-relevant order:

```text
withdraw endpoint requests
stop PEP and MachinePortProxy effects
unexpose provider-managed forwarding
remove Netavark and persistent namespace effects
release endpoint requests
```

The PEP performs the equivalent withdraw-before-stop and
release-after-effect-removal ordering for its own distinct request. A forced
gvproxy unexpose failure returns an error and leaves final teardown
`Withdrawing`; restart retains its exact `Active` fence while unexpose is
unacknowledged. In both modes the tombstone preserves the retry evidence and
blocks replacement. Only acknowledged absence permits final `Released` or
restart `Reserved` transition for exact same-incarnation reconstruction.

## Fail-Before And Behavioral Proofs

The three NNC3.4 safety predicates were captured before the production
migration:

- `machine_port_proxy_rejects_bind_without_port_lease` failed because the real
  proxy could bind and serve with no durable request;
- `active_manifest_is_observation_not_host_port_authority` failed because an
  active manifest displaced allocation from `15000` to `15001`; and
- `two_real_allocator_processes_expose_sandbox_pep_port_collision` failed
  because the sandbox and PEP processes both selected `41337`.

Each exact expected-red run exited `101` at its named safety assertion. After
the migration the same predicates are ordinary green tests. Additional proofs
cover:

- all-or-nothing reservation and ordered replay;
- no partial reservation when one sandbox binding conflicts;
- tenant-scoped stable identity and cross-tenant substitution rejection;
- complete request-set validation before container and krun Netavark effects;
- prepared PEP socket hold with no serving before activation;
- exact registry attachment evidence and divergent-listener rejection;
- durable PEP and MachinePortProxy `AddrInUse` evidence;
- exact published bind-scope authentication, including mapped-IPv6
  normalization and rejection of manifest scope widening;
- one atomic publication-plus-PEP reservation with no partial hold;
- range re-selection for authority-free plan previews at runner handoff;
- deterministic never-bound batch compensation after later planning failure;
- mixed `Failed`/`Reserved` batch compensation without partial mutation;
- krun preflight and VM-config materialization compensation for both ports and
  unrealized segment holds;
- container and krun early port-admission plus later bundle-planning failures
  finalize their placed-but-unrealized segment holds;
- a later machine-proxy collision drops and releases an earlier bound sibling
  while retaining `Failed` evidence for the colliding request;
- a non-bind PEP preparation failure abandons only its exact durable claim;
  explicit fresh-launch authority may then release proven no-effect state,
  while restart/direct preparation retains the request;
- an overlapping restart/bind collision cannot delete or replace the live
  PEP's trust anchor or registration;
- an overlapping live-registry teardown cannot delete the live trust anchor or
  release authority without exact local attachment evidence and acknowledged
  worker shutdown;
- persisted automatic-listener provenance is required, so an older or corrupt
  manifest cannot turn a plan preview into exact port authority;
- a prepared machine listener holds the socket but cannot forward until its
  exact lease is `Active`, and explicit shutdown drains tracked connections;
- an empty overlapping machine-proxy registry cannot release another
  process's live listener or withdraw its lease;
- machine wildcard authority conflicts with a same-port specific PEP while
  retaining external reachability as distinct desired exposure metadata;
- both matching-registry reuse and listener start revalidate exact `Active`
  durable provider evidence;
- absent PEP and machine registries are idempotent only for exact
  `Failed`/`Released` records, including mixed terminal machine batches;
- the two-port collision proof holds the first kernel-selected listener until
  the second is selected, eliminating ephemeral-port reuse flakiness;
- machine-provider ambiguous unexpose fencing; and
- plan-only manifests carrying no port authority;
- exact-request PEP and machine replays cannot fail or release another
  attempt's claimed effect, including a real two-process claim race;
- a foreign machine backend cannot withdraw an exact live provider without
  its process-local registration; and
- gvproxy publication and machine withdrawal are linearized by the same
  lifecycle lock;
- restart teardown returns the exact acknowledged-stopped PEP request from
  `Active` to `Reserved`, and the same generation can bind and activate again;
- a fallible post-stop trust-anchor cleanup retains a non-ready,
  non-replaceable tombstone and resumes against the same exact provider
  evidence;
- tenant publication quota is atomic across independent managers, counts
  crash-retained durable requests without manifests, treats exact replay as
  zero additional usage, and excludes host-internal PEP listeners;
- plan-only manifests remain authority-free and contribute zero durable quota
  usage; and
- machine publication rejects caller/manifest identity substitution and stale
  normalized forwarding targets before exposure;
- generic withdrawal rejects an in-flight `Reserved` bind claim without any
  durable mutation;
- restart PEP preparation retains exact `Reserved` authority, while only an
  explicit fresh-launch capability may release proven no-effect preparation;
- machine activation failure drops every inert socket before exact claim
  abandonment, and an activation acknowledgement loss inspects exact `Active`
  evidence before confirmed-stop rebind; and
- connection-local setup failure leaves provider shutdown false, returns
  bounded worker capacity, and permits another connection through the same
  listener before explicit shutdown;
- failed final PEP registration installs an engine-owned `Stopping` tombstone
  before any fallible publication or provider cleanup, so a secondary cleanup
  error retains the exact provider, trust anchor, and durable lease evidence
  for repair and retry;
- a conflicting primary PEP forces failed-registration evidence into a
  separately addressable quarantine tombstone; readiness fails closed,
  cleanup failure retains the exact quarantine, retry removes only it, and
  the original primary remains independently stoppable;
- runner exit waits for an ordinary lifecycle owner, adopts its current
  terminal result without rewriting the exit receipt, preserves that result
  after a wait-observation failure, and rejects changed immutable identity
  without persistence or cleanup;
- bounded runner decision staging recovers a crash-left stage, removes
  injected post-create and post-write failure residue, and publishes exactly
  one authenticated decision on retry;
- machine claim abandonment rejects a mismatched manager and a real mixed
  provider batch before any durable record changes, while a uniform exact
  machine batch clears only its bind claims;
- a machine accept-worker error or panic becomes a sticky provider-stop
  failure: every later cleanup retry returns the same failure rather than
  treating a consumed join handle as acknowledged stop;
- accept-worker unwind signals the provider shutdown token and joins every
  tracked connection worker before returning, so no connection worker can
  detach from the provider lifecycle; and
- a machine accept-worker panic leaves the exact durable request `Active` and
  its process-local registry entry `Stopping` across repeated cleanup attempts.
- the same attachment and `/30` IP can be released and reallocated under a new
  claim, but the prior claim cannot load, pre-effect-delete, confirmed-detach
  delete, or borrow the replacement's terminal evidence;
- stale container and krun network cleanup fail before PEP, port, segment,
  IPAM, namespace, Netavark, status-projection, or authority mutation; and
- missing serialized network-generation evidence fails closed rather than
  defaulting into cleanup authority.

## Explicit Deferred Reconciliation

This checkpoint does not claim NNC3.8 or NNC6 semantics early:

- a known synchronous planning/materializing failure compensates only while
  the complete batch is durably proven `Reserved` and never adopted;
- a crash or ambiguous outcome after durable reservation remains fenced for
  the NNC3.8 reconciler rather than being guessed safe to release;
- a crash after durable bind-claim acquisition remains `Reserved` with its
  attempt identity fenced for NNC3.8 reconciliation;
- a persisted PEP assignment or MachinePortProxy request is not proof that an
  absent process-local effect stopped; only exact local removal plus confirmed
  shutdown authorizes release in NNC3.4;
- sequential multi-request activate/withdraw/release may stop after a partial
  durable transition, but every completed transition remains fenced and
  idempotent;
- an `Active` record whose restart-time provider reconstruction fails remains
  an explicit active/effect-gap input rather than being silently reused; and
- persisted krun `Reserved`, `Adopting`, `Adopted`, and `ProviderOwned`
  nonterminal launch phases retain their exact claim and shutdown intent.
  NNC3.4 serializes new effects and refuses terminal publication or restart
  inference from those phases; NNC3.8 owns their fresh-process
  inspect-before-retry cleanup consumer after every producer migration; and
- generation and epoch are initially `1`, with the sandbox ID acting as the
  incarnation key until the durable workload saga supplies replacement
  generations.

NNC3.8 owns crash/ambiguous abandoned-reservation, cleanup-pending, and
effect-gap reconciliation. NNC6 owns desired workload generation and the
cross-domain saga. Those constraints are recorded here so this item cannot be
misread as full lifecycle completion.

## Modularity And Dependency Proof

The production PEP composition module is a 1,452-line concept owner after
moving its private behavior matrix to the 1,943-line concept-owned
`oci/egress/tests.rs` parent, its intact registration-failure lifecycle group
to the 248-line `oci/egress/tests/registration_failure.rs` child, its
post-activation acknowledgement-loss/cleanup group to the 786-line
`oci/egress/tests/post_activation_cleanup.rs` child, and its
retryable teardown state machine into the 432-line
`oci/egress/cleanup.rs` concept owner. The sandbox port adapter is a
focused effect-translation module; `PortManager` passes an upper-layer quota
decision into allocation, and provider effects remain in their provider
owners. The PEP parent matrix is deliberately in the 1,500–1,999
explicit-justification band: the restart/fresh-launch authority, overlapping
claim, trust-anchor exclusivity, and stopping-tombstone cases inspect the same
private registry lifecycle. It may receive no production logic or generic
fixtures. The registration commit and compensation proofs moved as one
coherent group when the parent crossed the hard threshold; future growth must
follow the same concept-owned extraction rule.

The complete port-lease lifecycle remains one concept-owned deep module, but
its production parent is an explicitly justified 1,918 lines. When launch
coordinator authentication pushed it over the hard threshold, named transition
vocabulary and operation-local diagnostics moved intact to the 138-line
`nimbus-network/src/port_lease/operation.rs` concept child. The private
behavioral matrix is now 1,977 lines in
`nimbus-network/src/port_lease/tests.rs`; its exact durable bind-claim
requirements and atomic tenant-publication quota behavior moved intact into
40- and 151-line concept children, while exact adopted-attempt activation and
Active-batch replay live in a 137-line concept child. Atomic tenant usage and coordinator
authentication must share the reservation transaction, and exact
confirmed-stop rebind remains a transition of the same record through the
214-line `port_lease/rebind.rs` concept child; it invokes the parent's one store
transaction and does not create another authority. Request/overlap and
bind-evidence vocabulary remain in `request.rs` and `binding.rs`; no child
owns a second state store or transition authority. The `oci/port_manager.rs`
production composition root is 1,434 lines after its intact private behavioral
matrix moved to the 1,875-line
`oci/port_manager/tests.rs` child. That child is deliberately in the
1,500–1,999 band because its initial/restart, Netavark/MachinePortProxy, quota,
cross-tenant, and mixed terminal-batch proofs inspect the same private adapter
state machine; it may receive no production logic or generic fixtures. One
shared preflight authenticates the
configured manager mode, batch cardinality, and every claim's provider before
either provider-specific abandonment path can mutate durable authority. The
krun VM behavior matrix is now a 1,974-line parent plus a 1,395-line
concept-owned
`tests/launch_compensation.rs` child, 201-line `tests/natural_exit.rs` child,
519-line `tests/explicit_stop.rs` child, and 353-line
`tests/lifecycle_locking.rs` child. The 217-line
`tests/startup_fencing.rs` child proves retained startup failure fences
relaunch, permits exact stop and non-restarting terminal cleanup, and keeps
restart-eligible inspection byte-for-byte read-only. The new exact attachment-generation
cleanup proof moved intact to the 87-line
`tests/generation_fencing.rs` child when it would otherwise have pushed the
parent over the 2,000-line hard threshold. The parent is an explicit test-band
exception because its remaining cases inspect one private VM lifecycle state
machine; it may receive no production logic or generic fixtures. Its next
growth must continue moving intact lifecycle groups to concept-owned children
rather than rebuilding the parent. The restart teardown
proof remains in the parent because it belongs to the same coherent VM
lifecycle behavior matrix.
The segment allocation state machine is a 1,268-line production owner after
moving attempt-scoped reservation/adoption/compensation into the 891-line
`segment/reservation.rs` concept child and its intact private behavioral matrix
into the 1,194-line `segment/tests.rs` child. The container runtime composition
root is 1,460 lines after its machine-proxy registry and lifecycle state
machine moved intact to the 1,067-line `runtime/machine_ports.rs` child, its
attachment-before-IPAM launch preparation moved to the 151-line
`runtime/network_launch.rs` child, its pre-effect and artifact compensation
moved to the 162-line `runtime/artifact_cleanup.rs` child, and its ordered
provider-backed teardown moved to the 362-line
`runtime/execution_cleanup.rs` child. The 149-line `runtime/creator.rs` child
durably records an owned asynchronous creator before spawn, contains its
controlled process group on failed handoff, and refuses cleanup while the
creator could still materialize a runtime. The 90-line
`runtime/effect_fence.rs` child bounds the shared pre-provider phase
publication protocol at four attempts. The 1,963-line `runtime/runner.rs` owner
now
uses one bounded advisory OS lock around Execute, Cancel, status, and inspect;
atomically creates and fsyncs one durable execution decision through bounded,
crash-reconciled staging; replays an existing Execute after owner death;
fingerprints the prepared manifest; revalidates persisted `PlanOnly` state
before changing the manifest to `Execute`; and reacquires the same lifecycle
authority after the workload wait before it rereads and finalizes current
durable state. Its test-only lock observation moved to the 96-line
`runtime/runner/test_probe.rs` concept child. The 220-line
`runtime/direct_execution.rs` child consumes the same effect-fence protocol
without duplicating its store or decision authority. This keeps
provider-specific concurrency, activation, linearized
publication/withdrawal, stop-proof, terminal-idempotence, and exact
reservation rollback in their concept owners while the composition root
remains below the repository's review threshold. The test-only container
lifecycle matrix is 1,943 lines and remains deliberately in the
explicit-justification band because its private restart/final ordering,
identity-substitution, ambiguous-unexpose retry,
activation-acknowledgement-loss, runner cancellation, and tombstone
concurrency proofs form one coherent behavior suite. It may not receive
production logic or generic fixtures. The 1,937-line launch-cleanup matrix is
also an explicit test-band exception because its owner-death, cancellation,
provider-ambiguity, staging-fault, and ordered-compensation cases inspect that
same private lifecycle. Its intact runner-finalization and bounded
effect-fence group moved to the 1,343-line
`runtime/tests/runner_reliability.rs` child; its provider-cleanup group,
including creator-pending fencing, remains in the 1,429-line
`runtime/tests/provider_cleanup.rs` child plus its 271-line startup-fencing
child. Future growth must remain
concept-owned; no additional cases may silently push a private matrix across
the hard threshold.
The 1,755-line `krun/vm/lifecycle.rs` production owner is a deliberate
1,500–1,999 exception. Runtime-stop observation, Netavark detach, exact
restart-claim compensation, namespace removal, attachment finalization, and
terminal manifest publication share one ordered error accumulator and one
`detach_confirmed` fence. Splitting twenty lines mechanically would obscure
that lifecycle invariant. New planning, provider implementation, fixtures, or
unrelated status logic may not enter it; further growth must extract one
complete phase with its invariant-preserving inputs and tests.
The machine-port provider effect is a 1,038-line
`oci/network/proxy.rs` production owner. Its accept loop now caps concurrent
connection workers at 128, reaps completed join handles before each admission,
checks all four directional polling timeouts before copy-pump admission,
classifies connection-local failures without terminating the provider, makes
provider-stop failure sticky, and drains the remaining set even when the
accept scope unwinds. Its intact 1,456-line private provider matrix moved to
the concept-owned `oci/network/proxy/tests.rs` child when the combined file
crossed the hard threshold. No socket, claim, routing, copy, shutdown, or
provider authority moved or was duplicated.
Its stream copier preserves the already-acknowledged write offset across
`Interrupted`, `WouldBlock`, and `TimedOut` retries, so bounded polling cannot
truncate a forwarded stream.

The proxy registry is a 1,505-line production lifecycle owner with a 1,427-line
private matrix. Poison recovery removes only the exact preparation capability,
preserves quarantined provider evidence, clears poison only after restoring
that invariant, and admits an exact replacement registration. Commit failures
carry one atomic provider/attachment/slot bundle: explicit retention consumes
it, while implicit drop installs the same stopping tombstone before releasing
the temporary cleanup executor.

`nimbus-network` still has exactly one workspace dependency,
`nimbus-network -> nimbus-core`. It contains no socket, proxy, Netavark,
namespace, nftables, gvproxy, Axum, Pingora, Iroh, or cluster transport effect.

## Verification

Owner verification on the complete post-finding diff:

- `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing`: 790 passed, 0 failed, 14 expected
  skips. The skips are existing environment/provider and child-process helper
  lanes, not new NNC3.4 coverage;
- the pass-24 correction set is 8/8:
  `container_preflight_failure_compensates_all_unstarted_launch_artifacts`,
  `adopted_container_attachment_cleanup_releases_never_bound_launch_authority`,
  `runner_launch_failure_after_attachment_adoption_compensates_network_authority`,
  `runner_exit_persists_execute_mode_after_owned_network_cleanup`,
  `adopted_krun_attachment_cleanup_releases_never_bound_launch_authority`,
  `plan_only_service_workload_prepares_runner_manifest_pointer_and_proxy_env`,
  `reserved_segment_cleanup_retry_reconstructs_pending_cleanup_after_finalization_failure`,
  and `stop_for_restart_rebinds_exact_active_lease`. The segment retry also
  passes alone and in the 31-test segment/reservation/reaper group; the PEP
  replay passes in the 39-test egress group;
- focused post-review proofs
  `machine_port_copy_retries_backpressure_without_losing_written_prefix` and
  `machine_connection_set_reaps_completed_workers_and_reuses_capacity` pass.
  They prove retryable partial writes are lossless and historical connection
  workers neither accumulate nor permanently consume bounded capacity.
  `machine_proxy_withdrawal_waits_for_inflight_active_validation` proves the
  registry lock linearizes durable withdrawal after a validated startup path;
  `machine_proxy_withdrawal_waits_for_inflight_publication` proves the same
  lock covers gvproxy publication; the complete lifecycle group is 16
  passed/0 failed/2 expected ignored.
  `runner_handoff_rejects_truncated_automatic_port_provenance` and
  `runner_handoff_rejects_explicit_port_forged_as_automatic` prove corrupt
  preview authority fails closed; the complete planning group is 21/0/0.
  All three new proofs first exited `101` against the reviewed pass-11 code;
- pass-12 fail-before proof
  `restart_network_teardown_retains_exact_segment_hold` exited `101` because
  restart reset quarantined, released, and finalized its durable segment hold.
  The explicit `Restart` teardown mode now removes only restartable provider
  effects while `Final` remains the sole authority-releasing mode;
- pass-12 fail-before proofs
  `pep_pre_adoption_cleanup_keeps_prepared_socket_exclusive` and
  `pep_pre_adoption_cleanup_failure_keeps_reserved_lease_fenced` both exited
  `101`: the first observed a replacement binder instead of `AddrInUse`, and
  the second observed `Released` instead of `Reserved`. Bound compensation now
  retains the prepared socket through trust-anchor removal and releases the
  lease only after confirmed removal. The complete PEP egress group is
  30/0/0, and the complete krun VM group is 52/0/2;
- pass-13 fail-before proof
  `pep_pre_adoption_release_follows_prepared_socket_removal` exited `101`
  because a replacement authority could reserve the released port while its
  real bind still lost to the old prepared socket. Compensation now preserves
  anchor-cleanup exclusivity, drops that socket, and only then releases the
  exact lease; the replacement reservation and bind both succeed afterward;
- pass-13 fail-before proof
  `plan_only_range_exhaustion_creates_no_durable_segment_allocation` exited
  `101` with an allocated tenant segment after an authority-free preview
  failure. Plan-only preview admission now precedes segment resolution, which
  avoids both an orphan allocation and unsafe tenant-wide compensation where
  no attachment hold exists. The complete planning group is 22/0/0;
- pass-13 fail-before proof
  `machine_port_io_polling_rejects_each_configuration_failure` exited `101`
  because all four injected provider failures were discarded. Operation-typed
  setup now fails before stream cloning or copy-pump admission, and
  `machine_port_connection_surfaces_io_polling_failure_before_copy_pumps`
  proves the cause returns through the connection worker without an unbounded
  join. The complete machine-proxy behavior group is 8/0/0;
- pass-14 fail-before proof
  `independent_machine_backend_cannot_withdraw_another_process_provider`
  exited `101` because a backend without the live process-local provider
  registration changed the exact lease from `Active` to `Withdrawing`. Exact
  registration proof now precedes durable withdrawal, and the test passes with
  the foreign backend leaving both durable authority and the real socket
  unchanged;
- pass-14 review found that exact-request PEP and machine replays could report
  terminal failure or release after another attempt had reserved but before it
  adopted its socket. `PortBindClaim` now gives one attempt exclusive durable
  authority for claim, bind, adoption/failure, and abandonment. Deterministic
  same-request PEP and machine tests pass, and
  `two_real_processes_same_request_get_exactly_one_bind_attempt_claim` proves
  exactly one real process wins the durable attempt identity. This correction
  was source-trace-derived from the structured review; no pre-correction
  behavioral exit was recorded;
- pass-14 fail-before proof
  `plan_only_range_exhaustion_creates_no_durable_segment_allocation` in the
  krun backend exited `101` with `10.0.0.0/24` durably assigned after preview
  rejection. Krun now performs pure preview admission before segment
  resolution, matching the container ordering;
- the owner-added publication proof
  `machine_proxy_withdrawal_waits_for_inflight_publication` exited `101`
  because withdrawal completed while the publication barrier was held.
  Publication now runs under the provider registry lifecycle lock and the
  exact test passes;
- pass-15 fail-before proofs
  `stop_for_restart_rebinds_exact_active_lease`,
  `post_deregister_cleanup_failure_retains_retryable_evidence`,
  `tenant_quota_is_atomic_across_independent_managers`,
  `crash_retained_reservation_consumes_tenant_quota_without_manifest`,
  `machine_proxy_rejects_caller_manifest_identity_mismatch_before_effect`, and
  `machine_proxy_reuse_requires_exact_normalized_forwarding_plan` each exited
  `101` against the reviewed pass-14 behavior, then passed after correction.
  These prove exact restart rebind, retryable PEP tombstones, transactional
  durable quota, and exact machine identity/routing evidence. Network unit
  tests additionally prove all nonterminal quota phases, terminal exclusion,
  attribution rejection without mutation, and zero-cost replay below a
  lowered limit. Container and krun plan-only proofs explicitly establish
  that manifest-only previews create no durable quota usage;
- pass-16 fail-before proofs
  `machine_proxy_lease_authenticates_exact_external_publication_address`,
  `machine_port_proxy_route_retains_exact_external_publication_address`,
  `machine_publication_rejects_external_address_substitution_before_proxy_or_forwarder_effect`,
  `machine_proxy_restart_rebinds_exact_active_lease`, and
  `machine_forwarder_unexpose_failure_keeps_port_lease_fenced` each exited
  `101` against the reviewed pass-15 behavior, then passed after correction.
  They prove that publication intent is exact but separate from wildcard
  conflict authority, restart cannot leave a stopped provider behind an
  `Active` durable lease, and failed final unexpose retains both
  `Withdrawing` fencing and the process-local retry handle;
- `machine_proxy_restart_waits_for_external_unexpose_before_rebind` proves the
  production restart path retains `Active` while external unexpose is blocked,
  rejects replacement and teardown-disposition substitution through the
  `Stopping` tombstone, and reaches `Reserved` only after the acknowledgement.
  Network tests prove the multi-lease confirmed-stop rebind is atomic,
  exact-request fenced, and idempotent. Partial machine-listener startup
  returns already-running handles for the same tombstone compensation path;
- the first complete affected nextest run executed 640 tests: 638 passed and
  two failed. The failures exposed two real edge cases rather than being
  weakened: an empty listener group consulted an irrelevant provider mode,
  and final teardown tried to substitute restart disposition after the exact
  lease had already reached `Withdrawing`. Empty groups are now authority-free
  no-ops, while exact `Active`/`Withdrawing` final teardown resumes the release
  disposition. Both focused proofs passed before the complete rerun;
- the post-pass-16
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` run executed 640 tests: 640 passed,
  14 skipped.
  Nextest marked one pre-existing `nimbus-testing` seed-farm test leaky; it
  passed and is unrelated to the NNC3.4 owned paths;
- pass-17 review accepted four P1 findings at overall confidence `0.97`:
  generic withdrawal erased an in-flight durable claim; PEP inferred release
  authority from `Reserved`; machine activation errors stranded exact claims;
  and one connection-local setup error shut down the entire listener while
  durable/registry state remained `Active`/`Running`.
  `withdraw_rejects_durable_reserved_claim_without_mutation`,
  `stop_for_restart_rebinds_exact_active_lease`,
  `machine_proxy_activation_failure_drops_listeners_and_abandons_exact_claims`,
  and
  `machine_port_connection_setup_failure_is_isolated_from_listener_shutdown`
  each exited `101` against the reviewed pass-16 candidate, then passed after
  correction. Withdrawal now preserves claimed `Reserved` state; PEP uses an
  explicit non-persisted fresh-launch capability; machine activation
  compensates only after provider absence; and connection completion cannot
  own provider shutdown;
- owner follow-up added exact ambiguous-outcome and availability proofs:
  `machine_proxy_activation_ack_loss_inspects_active_binding_and_rebinds`
  commits activation before injecting acknowledgement loss, authenticates the
  exact `Active` binding, drops every inert socket, and returns to `Reserved`
  before retry;
  `machine_listener_accepts_after_connection_local_setup_failure` observes a
  second connection on the same live listener before explicit bounded
  shutdown; and
  `fresh_launch_capability_releases_claimed_preparation_failure` proves the
  positive release-authority branch. The complete egress, machine-proxy, and
  container-lifecycle groups are 35/0/0, 12/0/0, and 23/0/0 respectively;
- the complete post-pass-17 owner rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 646 tests: 646 passed,
  14 skipped. Nextest retained the one unrelated pre-existing leaky
  annotation;
- pass-18 actual-Sol/xhigh/fast review reported two NNC3.8 recovery gaps and
  two actionable NNC3.4 findings. Lost process-local PEP and machine registries
  leave `Active`, claimed, or effect-gap records fenced: that is the already
  explicit NNC3.8 restart/cleanup-pending success criterion, and manufacturing
  provider absence in NNC3.4 would violate the serialized producer-migration
  order. The actionable PEP finding was exact: `RegistrationSlot` held the
  node-global engine map lock across durable state, policy, filesystem,
  cryptographic, socket, trust-anchor, and adoption work.
  `registration_preparation_does_not_block_unrelated_workload_lifecycle`
  reproduced that stall with a bounded behavioral failure (nextest exit
  `100`). The engine now publishes a short-lock, per-workload preparation
  marker; unrelated lifecycle calls proceed, while same-workload contenders
  wait and re-check exact commit or withdrawal. Three engine proofs plus
  `concurrent_ensure_running_registers_exactly_one_pep` cover unrelated
  progress, same-key commit, same-key withdrawal, and one-winner behavior.
  The final review finding identified a real listener-test race: shutdown may
  close the socket before its best-effort wake connect. The test now treats
  that wake like production stop does and relies on the bounded worker join;
- the post-pass-18 corrected complete affected rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 649 tests: 649 passed,
  14 skipped. The focused engine/listener set is 13/0/0 and the complete PEP
  group remains 35/0/0;
- pass-19 actual-Sol/xhigh/fast review reported four actionable findings and
  one formatter-only claim. Three exact behavioral fail-before runs exited
  `100`: `overlapping_live_registry_teardown_without_provider_evidence_retains_fence_and_anchor`
  showed restart cleanup returning success without process-local evidence;
  `activation_ack_loss_rebinds_after_confirmed_pre_start_provider_drop`
  observed `Active` after the prepared PEP socket was dropped; and
  `restart_route_failure_retains_reserved_machine_listener_authority`
  observed `Released` after a restart-only route failure. Restart teardown now
  authenticates the persisted assignment before any cleanup transition. PEP
  compensation drops the unpublished prepared socket, inspects the exact
  `Active` provider binding after abandonment reports an ambiguous commit, and
  atomically returns that confirmed-stopped effect to `Reserved`. Machine
  preparation consumes an explicit `MachinePortPreparationReleaseAuthority`:
  launch composition supplies `FreshLaunch`, restart supplies `Retain`, and
  only the former may retire proven-never-bound requests. Positive coverage
  proves the fresh-launch branch still reaches `Released`;
- the fourth actionable pass-19 finding corrected test proof rather than
  production behavior. The three engine preparation-concurrency cases now use
  a `Barrier` plus an observable Condvar-wait boundary. `try_recv` establishes
  that a same-key contender remains blocked only after it has semantically
  entered that wait; the one-second timeout is retained solely as a bounded
  failure guard after commit/withdrawal. The focused correction filter ran
  6/6 green. The review's P3 indentation claim is rejected: the exact reviewed
  bytes passed `cargo fmt --all --check`, and the post-correction bytes pass
  again;
- the post-pass-19 corrected complete affected rerun
  `timeout 900 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 651 tests: 651 passed,
  14 skipped. Nextest retained one unrelated pre-existing leaky annotation;
- pass-20 actual-Sol/xhigh/fast review threads
  `019f977c-828d-7ee0-9144-7c8728d7c770` and
  `019f9783-f626-7361-be1d-5773b8805513` reported two actionable P2
  findings at overall confidence `0.96`; the second review chunk was clean at
  `0.85`. The first exact fail-before,
  `failed_registration_commit_returns_caller_owned_cleanup_evidence`, exited
  `100` because a rejected registration commit consumed and dropped both the
  activated PEP and its attachment. `RegistrationCommitFailure` now returns
  both caller-owned artifacts on every commit failure. Sandbox compensation
  then preserves the primary commit error while it confirms provider
  shutdown, removes the exact trust anchor, authenticates the durable
  `Active` binding, and atomically prepares same-request rebind. The positive
  `registration_commit_failure_compensates_activated_provider_and_publication`
  proof confirms the anchor disappears, the durable record returns to
  unclaimed `Reserved`, and the real socket is reusable;
- the second pass-20 fail-before,
  `checksum_valid_semantically_corrupt_authority_fails_closed`, exited `100`
  because checksum-valid `TenantPublished`-without-tenant and
  `HostInternal`-with-publication records reopened as authority.
  `PortLeaseState::validate` now applies the same publication/accounting
  invariant used by construction to every durable record during reload, maps
  either contradiction to `CorruptAuthority`, and publishes no mutation. The
  focused post-correction set is 3/3;
- the post-pass-20 corrected complete affected rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 653 tests: 653 passed,
  14 skipped;
- pass-21 actual-Sol/xhigh/fast review threads
  `019f9797-1f9e-7743-9c57-4c2ae4aec408` and
  `019f979b-be46-7bb3-91ec-2486483031fa` produced one clean review chunk at
  `0.76`, then two actionable findings at overall confidence `0.95`.
  `registration_commit_compensation_failure_retains_retryable_tombstone`
  exited `100` before correction because a secondary trust-anchor cleanup
  failure discarded the activated provider and publication evidence. Failed
  registration now installs an engine-owned `Stopping` tombstone before
  inspecting or cleaning anything and runs the existing exact stop/rebind
  state machine against that retained evidence;
- `machine_proxy_worker_panic_remains_failed_on_retry` exited `100` before
  correction because the first failed join consumed its handle and the second
  stop returned success. `MachinePortProxy` now carries explicit `Running`,
  `ConfirmedStopped`, and sticky `Failed` stop states, so provider failure
  never becomes an acknowledgement by retry;
- `panicking_accept_scope_drains_tracked_connection_workers_before_unwind_returns`
  exited `100` before correction because accept-worker unwind detached its
  tracked connection worker. `MachinePortConnectionSet::drop` now signals the
  provider shutdown token and non-panickingly joins every remaining worker.
  The integration proof
  `machine_proxy_accept_worker_panic_remains_fenced_across_cleanup_retry`
  additionally confirms repeated cleanup returns the exact same failure while
  durable state remains `Active` and the process-local registry remains
  `Stopping`. The focused post-correction set is 5/5;
- the post-pass-21 corrected complete affected rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 657 tests: 657 passed,
  14 skipped;
- pass-22 actual-Sol/xhigh/fast review threads
  `019f97b5-7806-7a40-bbd1-5af765649142` and
  `019f97bc-82a4-7380-93ff-1f1970457644` produced two actionable P2 findings
  at overall confidence `0.96`, then one clean chunk at `0.82`.
  `concurrent_replay_cannot_release_another_coordinator_reservation` exited
  `100` before correction because an exact request replay inherited the first
  coordinator's no-effect release capability. `NetworkReservationClaim` is now
  durable compensation-only authority: reservation replay, atomic batch
  admission, verification, and no-effect release authenticate the exact
  claim; adoption and provider activation clear it. Crash/reopen, crossed
  claims, later-member rollback, redacted wire round-trip, and
  checksum-valid semantic-corruption proofs cover that invariant;
- `retained_restart_bind_failure_returns_to_reserved_and_retries` exited
  `100` before correction because restart-time `AddrInUse` terminalized a
  retained PEP lease that no longer had fresh-launch compensation authority.
  PEP and machine preparation now receive an explicit
  `FreshLaunch(claim)` or `Retain` authority. Fresh no-effect bind failure
  records durable `Failed`; retained restart failure abandons only the exact
  bind claim and returns to retryable `Reserved`. The analogous retained
  machine-proxy case was found by the owner audit and has its own bind-fail,
  retry, activate, and exact cleanup proof;
- container and krun initial launch now authenticate one persisted reservation
  claim across the complete publication-plus-PEP batch before namespace,
  Netavark, socket, test-probe, or command effects. Both provider families
  reject a substituted claim with byte-semantically unchanged durable
  `Reserved` records. Runner handoff preserves the exact authority root,
  port range, quota inputs, and claim; a missing claim or substituted root
  fails closed. The required nullable manifest field distinguishes
  post-adoption `null` from an omitted legacy/default authority;
- every adapter projection failure after reservation now claim-authenticates
  complete-batch compensation while retaining the primary diagnostic. A
  second sandbox manager cannot reserve or release an exact request owned by a
  different launch coordinator, and published and PEP batch claims cannot be
  crossed. The focused correction set is 9/9, the complete
  `nimbus-network` suite is 103/103, and the sandbox suite is 351/351 with
  9 expected skips;
- the corrected complete affected rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 665 tests: 665 passed,
  14 skipped. Strict Clippy initially exposed an eight-argument
  machine-preparation call; the correction introduced one private typed
  preparation request rather than suppressing the lint. The two new krun
  provider-preflight proofs moved intact into the concept-owned
  `tests/launch_compensation.rs` child, keeping its parent below 2,000 lines;
- normal same-process `Execute` still uses a deliberately non-persisted
  fresh-launch capability until its manifest handoff is durable. Atomic
  manifest replacement and crash-time orphan inspection remain explicit
  NNC3.8 reconciliation and NNC6 workload/network-saga inputs; pass 22 did not
  claim or fork those later authorities;
- pass-23 actual-Sol/xhigh/fast review threads
  `019f97f3-c49c-7541-8c87-a8d6067dbfff`,
  `019f97f9-2128-7fa2-88d9-b2f46f6708bc`, and
  `019f97fe-e40d-79b1-b224-16b87e531f53` reported two P1 and one P2
  findings at overall confidence `0.97`, then clean chunks at `0.78` and
  `0.88`. The exact fail-before proofs
  `generic_withdraw_cannot_bypass_reservation_compensation_claim`,
  `losing_port_coordinator_cannot_release_winning_segment_authority`, and
  `post_launch_compensation_failure_is_returned_with_primary_error` each
  exited `100` against the reviewed candidate;
- generic withdrawal now rejects a claim-owned `Reserved` record without
  mutation. One attempt-scoped `NetworkReservationClaim` is minted before
  placement and reserves the stable attachment before IPAM and the complete
  publication-plus-PEP port batch. `SegmentAttachmentState` explicitly
  distinguishes `Reserved`, `Held`, and `CleanupPending`; exact adoption is
  idempotent and converts only the claimed attachment, while reverse
  compensation releases ports, then IPAM, then the exact claimed attachment.
  Generic acquire, release, quarantine, and orphan reconciliation cannot
  consume `Reserved` authority. Durable segment-state validation runs before
  and after every transaction and on reads;
- cluster lease expiry fences late adoption while preserving restricted
  exact-claim compensation, so a failed coordinator cannot remove a sibling
  attachment or strand authority after a fenced super-net lease expires.
  Container and krun launch paths share the same ordering and claim. Their
  failure paths return both the primary launch error and any secondary cleanup
  error instead of discarding rollback failure;
- owner follow-up captured
  `plan_only_service_workload_prepares_runner_manifest_pointer_and_proxy_env`
  exiting `100` because exact runner cancellation left an unactivated trust
  anchor behind. `release_plan_only_execution_artifacts` now performs exact,
  idempotent port/IPAM/attachment compensation and removes the unactivated
  trust anchor and rootfs. Both the explicit runner-cancellation entry point
  and generic plan-only `stop_sync` have exact behavioral proofs;
- the post-pass-23 corrected complete affected rerun
  `timeout 900 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 674 tests: 674 passed,
  14 expected skips. Four-crate all-target check, strict Clippy, rustdoc,
  format, and diff gates are green. The port-manager launch seam uses the
  typed `SandboxLaunchPortPlan` rather than suppressing the argument-count
  lint;
- pass-24 actual-Sol/xhigh/fast review threads
  `019f9834-7ab3-7d80-874d-115a20a79378`,
  `019f983d-1443-7962-919f-47f8612fffae`, and
  `019f9840-bd50-7b01-b438-66167312c4f1` reported four P1 and two P2
  findings at overall confidence `0.97`, then a clean chunk at `0.84`.
  `adopted_container_attachment_cleanup_releases_never_bound_launch_authority`
  and
  `adopted_krun_attachment_cleanup_releases_never_bound_launch_authority`
  both exited `100` against the reviewed candidate because generic
  provider-owned cleanup could not compensate a segment attachment adopted
  before any port/PEP provider effect.
  `reserved_segment_cleanup_retry_reconstructs_pending_cleanup_after_finalization_failure`
  exited `100` with `AlreadyReleased`, and
  `stop_for_restart_rebinds_exact_active_lease` exited `100` at the
  missing-provider-evidence guard. The two runner findings were source-traced
  directly: the durable handoff remained `PlanOnly`, and the runner bypassed
  `execute_start` compensation by calling `launch_manifest` directly;
- the correction persists `Execute` ownership before the first runner effect,
  routes launch failure through the ordinary execute compensation path, and
  makes the effect-owning runner perform final cleanup before persisting
  `Stopped`. Container and krun classify complete launch-owned port groups as
  exact never-bound or provider-owned and fail closed on mixed/foreign
  evidence; confirmed detach releases exact never-bound requests, deallocates
  IPAM, and only then finalizes the adopted segment. Exact pre-effect container
  rejection now also removes the unactivated trust anchor and retains any
  artifact-cleanup error. The segment allocator durably retains the exact
  cleanup claim until identity/epoch finalization, and restart PEP teardown
  accepts only the exact clean `Reserved` completion state after a lost
  process-local tombstone;
- owner closeout moved the ordered container teardown state machine into the
  216-line `runtime/execution_cleanup.rs` concept owner, reducing the
  production composition root to 1,339 lines. The complete post-pass-24
  affected rerun is 680/680 with 14 expected skips; the eight focused
  correction proofs are green;
- pass-25 actual-Sol/xhigh/fast threads
  `019f9869-bcb0-7d83-81fe-82548c6b7875`,
  `019f986f-9bab-7121-bac1-9f4b4fcfe751`, and
  `019f9874-9438-78a3-b8c4-583253707501` reported eight findings at overall
  confidence `0.97`. Six are accepted and corrected in NNC3.4. Exact durable
  `NetworkReservationClaim` capability now authenticates every mutation while
  a launch-owned lease is `Reserved`; runner configuration persists its
  Buildah cleanup context and every handoff, launch, wait, pointer, manifest,
  network, and artifact failure converges to durable terminal evidence or
  retained retry evidence; failed fresh PEP registration releases exact
  fresh-launch authority while retained restart remains fenced; and preview
  ranges reject zero before rendering an unexecutable binding. The two P1
  crash-convergence findings are valid, current authority remains safely
  fenced, and serialized NNC3.8 owns their recovery after all listener
  producers migrate: process-local PEP incarnation loss and pre-manifest launch
  reservation abandonment must use durable inspect-before-retry evidence, not
  PID/TTL/bindability guesses or manufactured provider absence;
- pass-25 correction proofs pass 3/3 for coordinator claim/adoption/failure,
  8/8 for runner/PEP/range lifecycle behavior, 3/3 for the complete-suite
  fixture trust boundary, and 2/2 after extracting the registration-failure
  group. The first complete run truthfully failed 679/682 because three
  lifecycle fixtures attempted fresh PEP adoption without their exact launch
  claim; the corrected rerun passes 682/682 with 14 expected skips;
- pass-26 actual-Sol/xhigh/fast threads
  `019f99af-57a3-7741-b527-ccc5fdf1c2be`,
  `019f99b5-88c3-7873-86aa-32d313bccb2c`, and
  `019f99bb-1cf1-7b53-93c8-2e9b1c4a5b0e` reported seven findings at overall
  confidence `0.97`. Four NNC3.4 findings are accepted and corrected. Generic
  reservation replay now compares the complete optional coordinator claim,
  including `None` versus `Some`; failed-registration tombstone retention
  clears registry poison only after repairing the invariant; the runner uses
  an exclusive durable tenant/sandbox/fingerprint execution token plus
  persisted-`PlanOnly` revalidation before any provider effect; and this proof
  status now matches the controlling ledger. The launch-cleanup module-path
  finding is rejected with direct compiler evidence: Rust resolves the
  `#[path = "lifecycle.rs"]` child at the sibling path, and all four extracted
  tests compile and pass. The repeated lost-PEP-incarnation and abandoned
  `Reserved`-attachment findings remain accepted NNC3.8 obligations with the
  current records safely fenced. A crash after runner-token creation is
  likewise fail-closed: the token blocks a second execution owner, and NNC3.8
  must reconcile the exact token/fingerprint against durable manifest and
  provider evidence before clearing or resuming it;
- pass-26 correction proofs pass 1/1 for complete optional coordinator-claim
  matching, 1/1 for poison repair followed by normal stop completion, 1/1 for
  concurrent exclusive runner ownership, and 4/4 for the disputed extracted
  module. The first runner proof truthfully failed with exit `101` because its
  fixture was `Execute` rather than the required prepared `PlanOnly`; the
  corrected fixture passes and exercises the intended ownership race.
  The pre-pass-27 complete affected set passed 685/685 with 14 expected skips;
- pass-27 actual-Sol/xhigh/fast review threads
  `019f99c3-c736-7050-acf2-bfc852a7eea0`,
  `019f99c7-afd3-7f82-8b21-0a1139def907`, and
  `019f99cd-367f-7002-8aed-2fec30f394fa` reported six findings at overall
  confidence `0.98`. Five NNC3.4 findings are accepted and corrected.
  Container and krun now acquire the complete exact durable Netavark bind
  claim before namespace setup can call the provider, then atomically adopt
  and activate the complete batch after acknowledgement; no sequential
  transition can leave one claimed sibling beside an unclaimed sibling.
  Runner Execute and Cancel contenders now publish one
  tenant/sandbox/manifest-fingerprint decision by fsynced unique staging plus
  an atomic hard link, so a winner is durable before effects and a loser can
  never parse a partially published canonical record. Exact replay is
  idempotent while conflicting, stale, or corrupt evidence fails closed;
- acknowledged krun restart teardown reconstructs the exact expected provider
  bindings, then atomically returns the complete `Active` batch to clean
  `Reserved` state before relaunch. Replay accepts an already clean exact
  batch; teardown failure retains exact `Active` evidence. A process crash
  after provider acknowledgement but before that durable rebind still leaves
  authority fenced rather than reusable. NNC3.8 must inspect the durable
  provider/netns/IPAM evidence before completing or retrying that ambiguous
  edge; NNC3.4 deliberately does not infer absence;
- the legacy empty-allocation release fallback is deleted. Empty segment
  attachment state cannot invent ownership from a tenant or IP address and
  remains byte-identical on rejection; exact held or exact claimed attachment
  authority is required. The exact fail-before proof exited `100`, then the
  three ownership cases passed. PlanOnly cleanup paths in container and krun
  were correspondingly corrected to release only their claim-authenticated
  launch reservations and local artifacts, never fabricate an adopted
  attachment for provider teardown;
- the repeated launch-cleanup path finding is rejected with stronger compiler
  evidence: the production `#[path = "runtime/lifecycle.rs"]` context resolves
  its child to `runtime/launch_cleanup.rs`, and all eight child tests compile
  and pass, including exclusive Execute/Cancel, replay, and corruption cases;
- the first complete post-pass-27 affected run executed 693 tests: 688 passed,
  five failed, and 14 were skipped. Those failures exposed real load-only
  behavior rather than being waived: the runner loser observed a partial
  decision record before atomic publication; one container and three krun
  PlanOnly cleanup cases invoked provider/attachment cleanup without
  authority. The implementation fixed the publication primitive and split
  PlanOnly from Execute cleanup at the lifecycle owners. All five formerly
  failing cases plus the renamed no-invented-authority proof passed in the
  focused rerun. A stale quarantine-order fixture was then corrected to use an
  Execute manifest—the provider-backed lifecycle it claims to verify—and
  passed 1/1;
- pass-28 actual-Sol/xhigh/fast review threads
  `019f99e7-e327-72b3-9da2-eff5f80621a1`,
  `019f99ee-54fa-7201-a3bc-f042a71d4ab7`, and
  `019f99f4-ea3d-7152-8aed-2fec30f394fa` reported seven findings at overall
  confidence `0.96`; all seven are accepted and corrected. A pointer
  publication error can compensate only after winning a durable Cancel
  decision, so a concurrent Execute winner retains its manifest and network
  authority. Provider-assigned batches prospectively reject sibling
  collisions before mutation, authenticate every member before atomic
  activation, and require concrete binding evidence to name the same provider
  as its durable claim. Segment cleanup authenticates the exact reservation
  claim before deleting IPAM or changing any authority. One bounded advisory
  OS lock serializes runner Execute, Cancel, status, and inspect across
  processes; an existing Execute decision is replayable after owner death,
  while a losing PlanOnly observer cannot mutate its fingerprinted manifest;
- successful container and krun PlanOnly previews now persist
  `network_config: None`, allocate no segment or IPAM state, and fail closed
  before provider effects if an Execute manifest lacks attachment
  configuration. `Some(network_config)` means an execute-shaped manifest owns
  a reserved attachment; no compatibility default or migration shim was
  introduced. The three provider-batch/provider-authentication proofs, exact
  segment/IPAM ordering proof, pointer-winner proof, and both PlanOnly
  allocation proofs failed before correction with exit `101`, then passed.
  The owner-loss replay and observer-fencing proofs also pass;
- the first post-pass-28 complete affected run truthfully reported 703/704:
  the sole failure was a krun restart fixture that still modeled PlanOnly as
  owning a segment. The corrected execute-shaped fixture proves restart makes
  zero allocator calls and final teardown releases the exact retained
  attachment; its focused rerun passes 1/1 without weakening production
  behavior;
- the final post-pass-28 complete affected rerun
  `timeout 1200 cargo nextest run -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing` executed 704 tests: 704 passed,
  14 expected skips. Four-crate all-target/all-feature check, strict Clippy,
  warning-denied rustdoc, format, and diff checks pass. Verifier self-test is
  16/16; the live verifier is 14/1 solely at expected later NNCV005; docs are
  108 pages link-clean and 17/17 site conditions;
- pass-29 actual-Sol/xhigh/fast review threads
  `019f9a1f-4993-7672-a3f8-925ba7bbf48b`,
  `019f9a23-d4c9-7b42-b9c7-efd336412c64`, and
  `019f9a2a-ce3c-7751-ac1a-0be32e4fbe16` reported four findings at overall
  confidence `0.98`; all four are accepted and corrected. Ambiguous Netavark
  setup retains exact bind claims, namespace evidence, and IPAM until
  confirmed detach, then abandons only the authenticated claim batch.
  Owner-lost runner Execute replays the exact durable manifest while still
  `ClaimedBeforeEffects`, but an `EffectsStarted` record fails closed for
  inspect-before-retry. Exact confirmed-stop binding receipts distinguish a
  restart-retained `Reserved` PEP from a fresh reservation and authorize
  terminal release only after acknowledged provider stop. Never-realized
  attachment compensation records `ReservationCleanupPending`, deallocates
  IPAM, and only then performs exact finalization, including when a sibling
  attachment keeps the segment live;
- targeted fail-before nextest runs exited `100` at all four reviewed
  boundaries: Netavark setup eagerly removed IPAM/stranded claims; durable
  Execute replay rejected before the effect boundary; final PEP teardown
  rejected its exact acknowledged-stop state; and sibling-retained
  compensation removed attachment authority before retryable IPAM cleanup.
  The corrected Netavark unit/integration set, runner owner-loss/effect-boundary
  set, confirmed-stop port/PEP set, and segment reservation/cluster/reaper set
  all pass;
- the first complete post-pass-29 correction run truthfully reported 711/712:
  the sole failure was stale runner cancellation being classified as a
  changed manifest before the durable Cancel winner was read. The ordering
  was corrected without weakening the fence, its focused rerun passes 1/1,
  and the complete affected rerun
  `timeout 900 cargo nextest run -p nimbus-network -p nimbus-sandbox
  -p nimbus-proxy -p nimbus-testing --no-fail-fast` executes 712 tests:
  712 passed, 14 expected skips;
- the final post-Pass-29 quality gates pass: affected
  all-target/all-feature check; strict Clippy after replacing two obsolete
  error-only switchboards with `?`; warning-denied rustdoc; format and both
  staged/unstaged diff checks; verifier shell syntax/lint plus 16/16
  fail-closed self-tests; expected live verifier 14/1 solely at later-owned
  NNCV005; docs 108 pages link-clean; and docs site 17/17. Pass 30 remains
  pending;
- Pass 30 found three P1 lifecycle defects and one accurate procedural
  closeout observation. The direct Execute entry point now publishes the exact
  manifest and a durable `ClaimedBeforeEffects -> EffectsStarted` decision
  under the same bounded OS lock before any provider effect, and removes that
  fence only after a durable live or clean terminal manifest. The decision
  authenticates both the full prepared bytes and an immutable normalized
  execution identity so legitimate lifecycle progress cannot make recovery
  unverifiable. Exact stop, inspect, cancellation, and PlanOnly status
  mutation share that lock and fail closed at or beyond the effect boundary;
- Krun natural exit now persists `Stopping` with withdrawn endpoints, performs
  final exact provider/attachment/port/IPAM/segment convergence, and only then
  publishes `Stopped` or `Failed`. Cleanup failure leaves durable `Stopping`,
  the exit code, empty endpoints, and exact retry capabilities. Failed
  activation or egress pinning preserves the primary plus detach error, retains
  the netns and claims when Netavark absence is unconfirmed, and permits
  namespace removal only after confirmed detach and all required exact
  cleanup flags;
- focused Pass-30 correction proofs pass 8/8:
  `direct_execute_effect_fence_precedes_every_provider_probe`,
  `effects_started_phase_remains_verifiable_after_manifest_progress`,
  `plan_only_status_update_waits_for_execute_manifest_owner`,
  `direct_stop_after_predecision_owner_death_releases_only_unstarted_authority`,
  `failed_krun_activation_teardown_retains_retry_evidence_until_confirmed_detach`,
  `failed_restart_teardown_retains_exact_active_netavark_evidence`,
  `natural_execute_exit_releases_exact_network_authority_before_terminal_status`,
  and
  `natural_execute_exit_cleanup_failure_remains_stopping_with_exact_fence`.
  `terminal_manifest_write_failure_retains_direct_execute_fence` separately
  passes 1/1. The post-correction complete affected set passes 720/720 with 14
  expected skips. The direct-execution and natural-exit test groups then moved
  intact to 120- and 176-line concept children; the sandbox all-target/all-
  feature check, format, and diff checks pass after that mechanical extraction.
  The exact frozen-candidate rerun, remaining quality/static/docs gates, and
  Pass 31 are still required before closeout;
- all seven Pass-31 code findings are corrected. PEP shutdown releases
  authority only after explicit acknowledgement; the node registry now holds
  one per-workload lifecycle cell with a transferable RAII cleanup executor,
  and invokes attachment callbacks outside the node-global map lock;
  registration atomically waits/rechecks running and stopping state instead
  of observing a vacancy. The segment coordinator first reserves an unplaced
  attachment, then persists the exact IPAM-selected stable segment identity
  before adoption; same-segment replay survives reopen, remap is rejected,
  and secondary adoption cannot fall back to the primary. Runner ownership
  now publishes `LifecyclePublished` and releases the cross-process handoff
  lock before workload-lifetime wait. A joined machine proxy records confirmed
  absence even when its one-shot diagnostic is a join error or panic;
- deterministic correction proofs cover disconnected PEP acknowledgement,
  global-lock re-entry, exclusive cleanup transfer, atomic
  reserve-or-inspect, exact secondary bind/reopen/remap/adoption, published
  runner lock release/replay fencing, and joined machine error/panic
  convergence. The cross-listener race passes 10/10 stress iterations and
  permits only its two valid rejection linearization points. The corrected
  three-crate suite passes 655/655 with 9 expected skips. All-target/all-
  feature check, strict affected Clippy, warning-denied rustdoc, format, and
  staged/unstaged diff checks pass;
- modularity cleanup moved the intact private PEP-engine matrix into
  `engine/tests.rs`; the production composition root is 1,063 lines and the
  private matrix is 611. The exact cfg(test)-only child is classified by both
  the in-crate and external EE1 reachability proofs without broadening the
  production allowlist. The minimized proptest seed that exposed secondary
  placement divergence is retained as a durable regression case. The
  pre-ledger 67-path staged tree is
  `207c740592d30c80a546eddffc854b3d63b62078`;
- Pass 32 ran the required structured reviewer with
  `--engine codex --model gpt-5.6-sol --thinking xhigh --codex-speed fast`.
  Review threads `019f9acd-a5e7-7e93-81c2-9fe0db758557`,
  `019f9ad3-c587-7573-895f-81973b0010d9`,
  `019f9ad9-7b48-76d0-8a01-116d5366a3bd`, and
  `019f9add-04fa-70e2-87f0-0ffb9f7d94ef` inspected the normalized candidate
  in four chunks and reported six actionable findings at overall confidence
  `0.97`. The runtime output confirmed the actual reviewer was GPT-5.6 Sol at
  `xhigh` reasoning with service tier `fast`; no Opus review was accepted;
- all six Pass-32 findings are accepted and corrected. Direct Execute and the
  effect-owning runner persist the exact `NetworkReservationClaim` in an
  authority-free manifest shell before the first attachment, IPAM, or port
  reservation. Post-claim planning failure performs exact compensation and
  persists `Failed`, retaining the claim and remaining authority if cleanup
  itself fails. Fresh-backend restart tests prove both direct and runner paths
  later retire that exact authority without inventing provider evidence;
- runner finalization now converges under one owner: persist `Stopping`, retry
  exact provider/network cleanup while persisting partial `Stopping` evidence,
  and publish `Stopped` or `Failed` only after cleanup succeeds. Its
  deterministic ordered trace injects one failure at initial `Stopping`
  persistence, provider cleanup, and terminal persistence. Each retry waits at
  the named state-machine stage and the terminal manifest contains the
  successfully cleaned state;
- pre-effect IPAM authority now stores the exact reservation claim and
  compare-deletes only that claim in one transaction. A `/30` regression proof
  deletes generation C1, reallocates the identical address to C2, and proves a
  stale C1 retry cannot delete or mutate C2: IP address is never generation
  identity. Netavark setup consumes only pre-reserved, stable-segment IPAM and
  cannot allocate as a provider-side surprise. Normal post-effect release
  remains restricted to confirmed provider/netns detach;
- `OciNetworkConfig.segment_id` no longer has a zero/default serde escape:
  production deserialization requires the stable identity, while
  cfg(test)-only defaults generate distinct valid IDs. The machine publication
  race now synchronizes at the exact registry-lock boundary and proves
  `TryLockError::WouldBlock` rather than treating a 100 ms scheduling window as
  evidence;
- the exact Pass-32 correction matrix passes 14/14. The complete sandbox suite
  passes 413/413 with 9 expected skips. The final affected four-crate command
  executes 738 tests: 738 passed, 14 expected skips. All-target/all-feature
  check, strict Clippy, warning-denied rustdoc, format, cached/uncached diff
  checks, verifier 16/16 self-tests plus the expected 14/1 live state solely at
  later-owned NNCV005, docs 108 pages, and docs site 17/17 all pass. Current
  Pass-32 production roots were `port_lease.rs` 1,891 under its explicit coherent
  deep-module exception, `port_manager.rs` 1,307 after intact private-test
  extraction, container `runtime.rs` 1,411, runner 1,018, IPAM 696, and proxy
  engine 1,063. The resulting 67-path pre-review ledger tree is
  `c15d041c8ce19f72127dded9013675a7fd63c17f`;
- the complete Pass-33/34 correction matrix passes. Focused krun/runner
  lifecycle is 100/100; the new explicit-stop/lifecycle-lock set is 5/5;
  container provider cleanup is 3/3; the extracted machine-proxy matrix is
  17/17. One first proxy-matrix run reported nextest's transient leaky-test
  diagnostic; immediate complete and leak-status reruns both passed 17/17
  without a leak. The complete sandbox suite passes 429/429 with 9 expected
  skips, and the affected four-crate suite passes 754/754 with 14 expected
  skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc,
  format, and unstaged diff checks pass. The verifier self-test is 16/16 and
  its live scan is the expected 14/1 solely at later-owned NNCV005; NNCV006
  production-bind classification and NNCV008 recovery-ledger validation pass;
- Pass 35 ran the required structured reviewer with
  `--engine codex --model gpt-5.6-sol --thinking xhigh --codex-speed fast`.
  Review threads `019f9b65-99fa-7962-ad65-7da6055d3ef9`,
  `019f9b6b-9e72-75b0-a2f2-b07baf29bb1f`,
  `019f9b70-173d-7f23-ad21-b513e379f8fe`, and
  `019f9b75-7356-77b1-85b7-b3a9aafe69ce` inspected byte-identical 72-path
  tree `ddb16adb1fb5d6439220337099f21b74b829bedb` in four chunks under actual
  GPT-5.6 Sol at `xhigh` reasoning with service tier `fast`; no Opus result was
  used. Seven findings are accepted. The lifecycle-lock finding's literal
  fresh-start `ENOENT` evidence is amended: broad conmon directory creation
  made that exact failure unreachable, but OCI image, network, conmon, and
  reservation effects did precede the lock and violated the serialization
  invariant;
- runtime absence now requires the exact expected runtime identity and the
  entire pinned crun 1.27.1 `container \`<id>\` does not exist` diagnostic.
  Generic, foreign-ID, suffixed, and multiline failures remain unknown and
  retain cleanup authority. Prepared runner cleanup failure persists both
  manifest and handle as `Stopping`, so a fresh backend can retry to
  `Stopped`. Krun bootstraps only its private lifecycle-lock parent and
  acquires that lock before image materialization, network placement, conmon
  layout, manifest, or reservation effects. Explicit provider absence without
  an exit receipt and without durable shutdown intent withdraws endpoints but
  retains nonterminal `Adopted`/`ProviderOwned` authority for NNC3.8
  reconciliation;
- unstarted krun launch compensation now cleans the exact launch artifact
  before releasing its network claim and cannot publish `Released` while
  artifact cleanup is retryable. Netavark teardown loads IPAM through the
  expected stable segment and fails before provider or durable-state mutation
  on same-address cross-segment substitution. Initial attachment reservation
  acknowledgement loss now runs through the same exact reverse-compensation
  path as every later placement failure; its trace proves
  reserve, release-reserved, finalize-reserved, and finalize-release with zero
  placement/config construction;
- the exact Pass-35 correction matrix passes 9/9. The complete sandbox suite
  passes 435/435 with 9 expected skips, and the affected four-crate suite
  passes 760/760 with 14 expected skips. Strict affected Clippy with
  `-D warnings`, warning-denied rustdoc, and format pass. Verifier shell syntax
  and ShellCheck pass; its fail-closed self-test is 16/16 and the live scan is
  the expected 14/1 solely at later-owned NNCV005. The legacy egress verifier
  remains independently red on 23 absent archived-plan/proof conditions and
  is not used as NNC3.4 evidence;
- Pass 36 ran the required structured reviewer on byte-identical 72-path tree
  `25f4f70e4ca95ea2e491c3a18ddc77f728c6825b` under actual GPT-5.6 Sol at
  `xhigh` reasoning with fast service mode. Threads
  `019f9b96-1eed-7a01-9d34-289d9cd29060`,
  `019f9b9a-e517-70d2-9100-19f53a5a2b58a`,
  `019f9ba2-9768-7b71-a78a-cb749aebb39a`, and
  `019f9ba6-d083-7990-92e2-db9a7fb9ece5` reported four actionable findings;
  no Opus result was used. All four are accepted and corrected. Runner exit
  and wait-error finalization reacquire the exact lifecycle lock, reread
  current durable state, and authenticate the immutable execution identity
  before mutation. Handoff decision staging uses bounded fixed names and
  cleans plus parent-syncs every write/sync failure. A failed PEP registration
  collision retains exact provider evidence in an engine-owned quarantine
  tombstone whose fail-closed readiness and exact independent cleanup preserve
  the primary lifecycle. Machine-port abandonment authenticates manager mode,
  cardinality, and every claim provider before any mutation;
- fail-before evidence captured the stale runner finalizer racing an explicit
  terminal stop and a provider/manager mismatch being accepted. The complete
  Pass-36 correction set passes 10/10, full sandbox passes 443/443 with 9
  expected skips, and the four affected crates pass 769/769 with 14 expected
  skips. Strict affected Clippy with `-D warnings`, warning-denied rustdoc, and
  format pass. The corrected candidate still requires the repeat frozen-tree
  review and final static/docs closeout before item completion;
- Pass 37 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9bcb-3732-7943-8fd3-eaa58fd4bbdc`,
  `019f9bd1-c79d-7da3-8e39-f3adc123e047`,
  `019f9bd7-183f-7062-9817-107ab6744043`, and
  `019f9bda-2de3-79f3-b62c-1cf639ee285d` against byte-identical staged and
  review-mirror tree `8ece305a14aaecae672dab3967548926cc25c452`.
  Seven findings were dispositioned. The claimless public `adopt` wrapper and
  no-effect-failure compatibility wrapper had zero production callers and
  22 plus 6 test callers; both were deleted, every caller now takes an exact
  durable `PortBindClaim`, `Binding` without that claim is semantic corruption,
  and source scans find zero remaining symbols. Pinned crun 1.27.1 source
  proves the raw non-TTY C-locale absence diagnostic is exactly
  `container \`ID\` does not exist: open \`STATE_ROOT/ID/status\`: No such file
  or directory\n`; the parser fixes `LC_ALL=C`, requires empty stdout, exact
  safe ID/path/status/errno/newline evidence, and rejects prefixes,
  traversal, foreign IDs, extra lines, invalid UTF-8, and nonempty stdout.
  Lifecycle-lock tests now wait at a hook fired only by the real
  `try_lock_exclusive` `WouldBlock` branch and reread a durably changed
  manifest after acquisition. The repeated `launch_cleanup` module-path claim
  is rejected because the canonical target compiles and executes the child
  tests. Durable krun `Adopting`/missing-receipt convergence remains the
  serialized NNC3.8 consumer while exact nonterminal authority stays fenced.
  Side-effecting inspect/restart remains the later NNC5.6/NNC6.4a correction;
  NNC3.4 repaired its fixture so the NNC0.6a expected-red exits `101` at the
  NNCF20 terminal assertion with `left: 1`, rather than failing setup;
- the semantic lifecycle-lock tests failed before the production hook with
  their bounded “must reach actual contended lifecycle-lock boundary”
  assertions and then pass 2/2. The claim-only correction matrix passes 6/6,
  `nimbus-network` passes 110/110, full sandbox passes 444/444 with 9 expected
  skips, and all four affected crates pass 771/771 with 14 expected skips.
  Strict affected Clippy with `-D warnings`, warning-denied rustdoc, format,
  and worktree/index diff checks pass. The corrected candidate still requires
  the repeat frozen-tree review and final static/docs closeout before item
  completion;
- Pass 38 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9bf9-d0a8-7f20-b404-d8bc2b2e0a2f`,
  `019f9c02-36f5-78c1-a8bf-02cf0761a00e`,
  `019f9c07-a379-7bd3-88f2-6cf69c243753`, and
  `019f9c0d-2ac3-7483-96fa-21197762ebab` against byte-identical staged and
  review-mirror tree `78f75df17567fb30f05d41fa18872a8570f97f47`.
  The first and fourth chunks were clean at `0.74` and `0.84`. Three P1
  findings are accepted as exact serialized NNC3.8 convergence obligations:
  a fresh process loses process-local machine-port provider ownership; krun
  restart may persist `Reserved` without a fresh-process convergence
  consumer; and durable krun `Adopting` needs that same consumer. NNC3.4
  retains every nonterminal fence and refuses unsafe terminal publication or
  reuse; moving their consumer early would violate producer-first migration
  ordering;
- the fourth P1 finding is accepted and corrected in NNC3.4. Before the fix,
  `timeout 300 cargo test -p nimbus-sandbox
  stale_claim_cannot_load_or_delete_reallocated_same_attachment_ipam --
  --nocapture` exited `101`: the same attachment's replacement reused
  `10.89.0.2`, and the stale claim loaded that replacement. Network plans now
  persist their exact reservation claim; IPAM live records and terminal
  tombstones authenticate attachment, segment, and claim; allocation
  atomically supersedes the prior tombstone; setup, teardown, and high-level
  container/krun cleanup authenticate before any provider, namespace,
  projection, port, PEP, segment, or IPAM mutation. Cleanup authentication
  returns exact live addresses or terminal evidence in one read, avoiding a
  split authenticate/reload window;
- focused stale-generation coverage passes 12/12, including same-address ABA,
  setup/teardown provider-effect fencing, container cleanup fencing, and krun
  cleanup fencing. Full sandbox passes 448/448 with 9 expected skips; the four
  affected crates pass 775/775 with 14 expected skips. All-target/all-feature
  check, strict affected Clippy with `-D warnings`, warning-denied rustdoc,
  format, and diff checks pass. Static syntax/ShellCheck and fail-closed
  self-tests pass; the live verifier is the expected 14/1 solely at
  later-owned NNCV005. Docs remain 108 link-clean pages and site verification
  remains 17/17. The corrected candidate still requires the repeat
  frozen-tree review before item completion;
- Pass 39 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9c37-fa3d-7fd0-b0c0-0ea2f90c1e24`,
  `019f9c3c-6af1-7f71-99a7-0e5acacecb5f`,
  `019f9c41-edfc-79c1-a84e-7dc732603f38`,
  `019f9c46-587c-76b0-82af-e4261ae68bd3`, and
  `019f9c48-ba28-7200-9d3a-29271e9f2f19` against byte-identical staged and
  review-mirror tree `747b242c1424af04a3abef6fa7193f16757b5301`.
  Two chunks were clean at `0.84` and `0.95`. The repeated
  `runtime/launch_cleanup.rs` module-path claim is rejected: the canonical
  target compiles and executes the exact extracted tests under their real
  `#[path]` context. The process-local machine cleanup restart gap is accepted
  as the serialized NNC3.8 consumer, and side-effecting inspect/restart remains
  the already expected-red NNC5.6/NNC6.4a correction. Both current paths retain
  exact nonterminal authority and cannot publish unsafe reuse;
- the proposed unforgeable provider-stop capability inside `nimbus-network`
  is rejected at the ownership boundary, not deferred. The transport-free
  control plane can authenticate durable request, selected port, binding,
  generation, and transition, but it cannot mint or validate proof that a
  sandbox-owned socket, proxy worker, Netavark effect, or external publication
  actually stopped. A nominal lower-crate token would be minted on the same
  caller assertion and add no provider evidence while duplicating effect
  authority. The exact source call graph has no direct production caller:
  raw single/batch transitions occur only in `nimbus-network` tests and the
  private OCI lease adapter; its production consumers are PEP cleanup after
  `shutdown_provider` acknowledgement, machine cleanup after every proxy join
  and publication-unexpose acknowledgement, Netavark restart after exact
  teardown success, and activation-ack-loss compensation after dropping the
  exact prepared sockets. Ambiguous/fresh-process states never take this path
  and remain fenced for inspect-before-retry in NNC3.8;
- the remaining P2 status-projection finding is accepted and corrected.
  Against the frozen Pass-39 candidate,
  `timeout 300 cargo test -p nimbus-sandbox
  status_removal_failure_cannot_confirm_netavark_detach -- --nocapture`
  exited `101` because a directory at the Netavark status path made removal
  fail while teardown returned success. Status removal now propagates every
  error except exact absence before provider detach may be confirmed. A
  terminal IPAM replay does not delete a potentially newer projection and
  instead requires exact observed absence. The corrected behavioral test
  proves removal failure retains the exact live IPAM allocation, retry
  converges after removal is possible, absent terminal replay is idempotent,
  and a conflicting replacement projection fails closed;
- the corrected status test passes 1/1. Full sandbox passes 449/449 with 9
  expected skips; the affected four-crate command executes 776 tests: 776
  passed, 14 expected skips. Affected all-target/all-feature check, strict
  Clippy with `-D warnings`, warning-denied rustdoc, and format pass. Verifier
  syntax/ShellCheck and 16/16 fail-closed self-tests pass; its live result is
  the expected 14/1 solely at later-owned NNCV005. The 75-path corrected
  candidate still requires staging, byte mirroring, and repeat Sol review;
- Pass 40 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9c59-c555-7970-a77a-eafdfebe13bb`,
  `019f9c60-c344-7c61-8432-089f7155346a`,
  `019f9c68-f6cb-7f23-bb96-19c3e5e3c86f`,
  `019f9c6e-cab6-70b0-8f10-f04895bbc971`, and
  `019f9c72-e50e-7633-bb96-19c3e5e3c86f` against staged tree
  `e8a28c38bd9d3cc49741223c6e8c778545e5738c`; one chunk was clean at
  `0.84`. Seven findings were dispositioned. Six code/test findings were
  accepted and corrected. Fresh-process loss of the process-local machine
  registry is accepted as the already serialized NNC3.8 convergence consumer;
  current NNC3.4 behavior remains safely fenced. The final procedural finding
  required the proof/recovery ledger and repeat review before completion; this
  checkpoint satisfies its ledger half without claiming the repeat review;
- container setup no longer escapes through `?` after a durable attachment
  claim. The concept-owned `complete_network_setup` seam routes setup failure
  through exact detach compensation, and its portable test persists the exact
  netns retry handle plus claims before forcing failure. A literal old-code
  container fail-before could not execute on macOS because the production
  Linux netns precondition fails earlier; the new seam makes the relevant
  post-claim boundary deterministic without weakening production checks;
- restart cleanup no longer derives port state from coarse
  `ProviderOwned` workload authority. `PortManager` validates every exact
  request and durable record, then classifies initial `NeverBound`, exact
  Netavark claims, active/provider-owned, clean restart-retained, terminal
  provider-owned, and released batches. Against the frozen pre-correction
  production tree,
  `restart_launch_failure_after_netavark_claim_releases_only_after_confirmed_detach`
  exited `101` with `different in-flight provider bind attempt`. After
  correction, ambiguous detach retains the exact claim; confirmed detach
  atomically abandons it with no initial-launch claim and releases the exact
  restart-retained batch. `release_batch_after_confirmed_stop` accepts a
  legacy partial replay only when every sibling is either already `Released`
  or owns the exact confirmed-stop receipt; one invalid sibling leaves the
  entire batch unchanged. A two-listener proof explicitly converges one
  legacy `Released` member plus one current `RestartRetained` member;
- runner lock tests now wait on a bounded condition fired by the real
  `try_lock_exclusive` `WouldBlock` branch. They no longer interpret a
  100-millisecond negative scheduling window as contention evidence.
  Inspection rereads only after actual lock acquisition, so the proof binds to
  the production concurrency boundary;
- terminal IPAM evidence is now a retry witness bounded by unresolved durable
  lifecycle authority rather than an append-only history. Exact
  compare-delete retirement rejects foreign claims without rewriting bytes;
  terminal container/krun manifest publication retires its own witness; and
  startup reconciliation before launch admission removes only witnesses named
  by exact terminal manifests. A newer authenticated never-realized claim may
  supersede an older terminal generation only when no live allocation exists;
  any foreign live generation fails closed. The focused matrix proves
  same-attachment replacement, byte-stable stale rejection, terminal-manifest
  startup recovery, and 256 unique completed attachment lifecycles leaving
  both live and terminal IPAM maps empty;
- Pass 42 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9ce6-20ac-7a30-8b2b-836d31e084b3`,
  `019f9cea-18c4-7860-ae23-b91356292b19`,
  `019f9cef-c43f-7602-aee5-6c4c705c5a24`,
  `019f9cf4-c3f7-70d0-8dad-e69fc96aceff`, and
  `019f9cf7-3d29-79c0-8342-a4d8781bcb9d` against byte-identical 76-path tree
  `98a90d439ebf3cb74356645fff32f042a60227d3`. Seven findings were
  dispositioned. The direct-call persistence loop, nonzero-exit
  classification, and exact-address machine bind findings are accepted and
  corrected. The direct path now returns after four failed persistence
  attempts while retaining `ClaimedBeforeEffects`, performs no provider
  effect, and lets a successor reacquire and advance the same decision.
  Natural nonzero exit durably records `Failed` with the exact exit code while
  an explicit stop that already won remains terminal. Machine proxies bind and
  evidence the exact requested IPv4 or IPv6 address instead of widening it to
  `0.0.0.0`;
- the repeated launch-cleanup module-path claim is rejected using canonical
  target compilation plus executed-child evidence. The 1,874-line PEP test
  parent remains the already documented coherent private-state-machine
  exception, with no production logic or generic fixture growth. The
  fresh-process Active PEP finding is accepted as the dependency-serialized
  NNC3.8 convergence consumer, where current durable authority remains fenced.
  The pending Final row is accepted as the required review-time procedural
  state and cannot be marked complete before the repeat exact-tree review;
- the intact runner finalization/reliability group now lives in a 359-line
  concept-owned child, leaving `runtime/launch_cleanup.rs` at 1,697 lines and
  `runtime/runner.rs` at 1,471. Bounded persistence, durable replay, nonzero
  exit, explicit-stop winner, wait-failure, and identity-fencing proofs remain
  colocated with that lifecycle seam rather than inflating a production
  switchboard;
- corrected focused proofs pass 8/8. One pure route assertion received a
  transient nextest leak attribution only in the concurrent focused run; its
  isolated rerun passes 1/1 with no leak. Full `nimbus-network` executes
  112/112; full `nimbus-sandbox` executes 464/464 with 9 expected future-band
  skips;
  `nimbus-proxy` executes 146/146; and `nimbus-testing` executes 71/71 with 5
  expected child-role skips, for 793 passed and 14 expected skips across the
  four affected crates. All-target/all-feature check, strict affected Clippy
  with `-D warnings`, warning-denied rustdoc, format, worktree/index diff
  checks, verifier syntax/ShellCheck, 16/16 fail-closed self-tests, and the
  expected live verifier result of 14/1 solely at later-owned NNCV005 pass.
  Docs gates are now green at 108 pages and 17/17. The repeat frozen-tree Sol
  review and final ledger/gate transition remain required before completion;
- Pass 43 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9d14-884a-7a70-849a-291e68762e3d`,
  `019f9d18-596d-7373-8ac8-18143def38c0`,
  `019f9d1d-7b50-7f51-9163-5cc680a0ac75`,
  `019f9d22-7df1-7472-aebd-1428a27d2a22`, and
  `019f9d26-0f9f-7e23-a53c-2a4868b3bf9a` against byte-identical 78-path tree
  `0910f4d6de6b02e67d20c3734f8e740cda664174`. Seven findings were
  dispositioned; the last two chunks were clean at `0.82` and `0.91`.
  Accepted the direct pre-effect owner-loss finding: inspection remains
  read-only and status mutation remains fenced, while explicit stop now owns
  only the authenticated `ClaimedBeforeEffects` no-effect compensation,
  persists a complete terminal manifest, then publishes
  `LifecyclePublished`. `EffectsStarted` and every ambiguous later phase
  remain fenced. The new reopen proof also covers a crash after terminal
  manifest publication but before handoff-phase publication;
- rejected the repeated launch-cleanup module-path claim using the canonical
  target's successful compile and execution. Accepted persisted krun
  `Reserved`/`Adopting` convergence as the dependency-serialized NNC3.8
  consumer. Rejected the broad krun double-rebind claim: the exact
  `Active -> Reserved` transition is idempotent and its existing replay proof
  passes. Accepted its narrower adjacent defect: restart reset now uses exact
  inspect-after-ambiguous runtime deletion and removes stale pidfiles before
  deleting the exit receipt, so a failed cleanup retry cannot erase the
  durable restart witness. The correction proof forces ambiguous deletion and
  sequential pidfile failures while proving network authority remains
  byte-identical until the third successful retry;
- accepted PEP post-activation acknowledgement-loss plus anchor-cleanup
  failure as an NNC3.4 defect. Activation commit is now distinct from
  post-activation observation. Once activation is durable, every observation
  or commit failure installs an engine-owned non-ready `Stopping` tombstone
  containing the exact started proxy, attachment, provider binding, anchor,
  and cleanup disposition before any fallible cleanup. Both `Retain` and
  `FreshLaunch` proofs show a fresh registry cannot infer provider absence or
  mutate the lease, while the original executor removes the blocker, resumes
  exact cleanup, and converges idempotently. Fresh-process Active PEP
  reconstruction remains the NNC3.8 consumer; no false absence was added;
- accepted side-effecting container/krun inspection as the preserved
  NNC5.6/NNC6.4a expected-red, not an authority to cross into NNC3.4. The
  current inspected states remain fenced. The four new correction proofs pass
  4/4, and the complete runner-reliability, PEP registration/post-activation,
  and krun launch-compensation filter passes 30/30. The first full sandbox
  correction run exposed a claimless test fixture and a legitimate
  never-started `None` exit receipt at terminal publication; the fixture now
  uses the production claim-bearing runner preparation seam and validation
  accepts only the two no-effect terminal receipts (`None` for initial-launch
  failure and `Some(0)` for explicit stop). Both reproductions pass 2/2;
- full sandbox passes 468/468 with 9 expected future-band skips. The complete
  affected command executes 797 tests: 797 passed with 14 expected skips.
  One `nimbus-network` test received a transient nextest leak annotation only
  in that concurrent run; its isolated rerun passes 1/1 with no leak.
  All-target/all-feature check, strict affected Clippy with `-D warnings`,
  warning-denied rustdoc, format/diff checks, verifier syntax/ShellCheck,
  16/16 fail-closed self-tests, and the expected live verifier result of 14/1
  solely at later-owned NNCV005 pass. The 79-path correction candidate still
  requires final staging, byte mirroring, repeat Sol review, and post-ledger
  docs gates before completion;
- Pass 44 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9d49-4330-7f53-85c5-7baccfeff870`,
  `019f9d4f-a388-7443-b550-73b6afb2a7e5`,
  `019f9d54-b470-73c2-8f51-2df3c277dd4a`,
  `019f9d5a-22e8-7620-8251-d41358fc6d8f`, and
  `019f9d5d-3930-7450-ab6f-4ab81d60eacb` against byte-identical 79-path tree
  `4548035221851e56205a4e2a2a830f0e8dd57121`. Seven findings were
  dispositioned. Accepted and corrected the async creator race: a durable
  `Pending` handoff precedes spawn, failed handoff contains and reaps the
  controlled process group before `Quiesced`, and cleanup refuses every
  provider/network release while the creator may still materialize a runtime.
  A live conmon receipt remains ambiguous and is never killed as if owned;
- accepted and corrected unbounded runner effect-fence persistence. Runner and
  direct execution now share the four-attempt, phase-aware convergence helper;
  exhaustion proves no provider sentinel ran and preserves exact inspect-before
  retry authority. Accepted process-local machine reconciliation as the
  dependency-serialized NNC3.8 consumer. Accepted the missing proof-ledger
  documentation exception and recorded it in the owner plan. Rejected the
  repeated launch-cleanup module-path claim using compiled/executed canonical
  target evidence; rejected the alleged production runtime binds because both
  cited binds are test-only; and rejected fixed generation/epoch aliasing for
  NNC3.4 because every public start mints a tenant-qualified ULID-backed
  sandbox incarnation and same-incarnation terminal resurrection is rejected;
- three independent read-only source audits confirmed those dispositions
  without edits. Focused creator/effect-fence proofs pass 10/10; full sandbox
  passes 476/476 with 9 expected skips; and strengthened controlled-wrapper
  descendant plus exact provider-sentinel proofs pass 2/2. The first complete
  805-test affected run reproduced a real post-`SIGKILL` descendant-reaping
  window: the wrapper was reaped but a killed descendant kept the process
  group observable long enough to deny quiescence. The correction polls the
  exact retained group for bounded semantic absence, treats every live,
  permission-denied, reused, or unknown observation as fenced, and never sends
  another signal after the owned cancellation. Its isolated proof passes 1/1;
  the repeat affected run passes 805/805 with 14 expected skips;
- all-target/all-feature check, strict affected Clippy with `-D warnings`,
  warning-denied rustdoc, format/diff checks, verifier syntax/ShellCheck,
  16/16 fail-closed self-tests, and the expected live verifier result of 14/1
  solely at later-owned NNCV005 pass. Docs are green at 108 pages and 17/17
  conditions. Staging, byte mirroring, and the repeat frozen-tree Sol review
  remain before final closeout;
- Pass 45 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9d80-deb1-73d3-9c3b-b564df5fc982`,
  `019f9d8a-83ad-72c2-ac64-86f8f00af51d`,
  `019f9d8f-5a45-7722-b3f9-3c5a5d012aa5`,
  `019f9d95-8ce2-7823-aaa5-f839accda7be`, and
  `019f9d9c-6125-74b0-baa5-8e60df524d50` against byte-identical 82-path tree
  `0245701477291f20a64137d0e1abad24f2df479f`. Six findings were
  dispositioned. Accept and correct Active batch replay that authenticated
  only the provider registration: the authority now retains the exact adopted
  attempt, claim-authenticates single activation and replay, and rejects a
  foreign same-provider attempt atomically without rewriting durable bytes.
  Accept and correct idle machine-worker observation and the unbounded
  test-thread unwind: completed workers are reaped every accept poll, exited
  retained providers cannot publish, and the unwind proof has a supervised
  driver plus independent bounded cleanup;
- accept fresh-process Active PEP reconciliation as the dependency-serialized
  NNC3.8 consumer. Reject the repeated launch-cleanup path claim using the
  canonical compiled/executed target. Reject krun execute-mode projection
  because `PublishedEndpoint` is observed reachability: Execute intentionally
  exposes none until `Ready`, while the updated desired spec and durable
  leases drive exact readiness reconstruction. Three independent read-only
  audits traced these seams without edits. Correction proofs pass 2/2 network,
  3/3 focused sandbox, and 7/7 real-process binding with 2 helper skips; full
  sandbox passes 477/477 with 9 expected skips. The complete affected command
  then passes 808/808 with 14 expected skips. All-target/all-feature affected
  check, strict Clippy with `-D warnings`, warning-denied rustdoc, format/diff,
  verifier syntax/ShellCheck, 16/16 fail-closed self-tests, and the expected
  live verifier result of 14/1 solely at later-owned NNCV005 pass. Docs remain
  green at 108 pages and 17/17 site conditions. Staging/mirroring and the
  repeat-review gate remain. The exact Pass-45 tree
  reproduced both accepted defects with the new
  behavioral assertions: network run
  `a5f8de89-dd06-429c-912a-930ba9682a0e` exited `100` because the foreign
  attempt was accepted, and sandbox run
  `fd948b61-2f7d-4adb-8332-ca45a6722fa8` exited `100` because the idle accept
  loop did not observe its failed worker;
- Pass 46 ran actual GPT-5.6 Sol/xhigh/fast in threads
  `019f9dc4-6654-7bd2-8a2c-81097ce718e5`,
  `019f9dca-1c52-7c22-b91f-9ef5403906da`,
  `019f9dce-455f-7280-97a1-0cd900e8b9dd`,
  `019f9dd0-a7db-7552-84de-93830b0a0c2f`, and
  `019f9dd6-717a-70f2-a577-6f551b86976b` against byte-identical 83-path tree
  `35f441855766cb169a45f536cc6201bfc89c98b4`; the final chunk was clean.
  Accept and correct absent/unreadable creator receipt as ambiguous authority,
  bounded successful creator reaping, and listener liveness that previously
  outlived the exact socket while connection workers drained. Accept
  fresh-process machine-port ownership as dependency-serialized NNC3.8.
  Reject the alleged krun restart-retained PEP leak: launch classification
  reaches `ProviderOwned`, final cleanup recognizes confirmed-stop evidence,
  and the exact restart-then-final release/rebind proof passes. Three read-only
  audits confirmed the dispositions;
- fail-before run `cfa3a60e-86ec-4cb7-b261-0af4a3685335` failed 0/2 because a
  missing receipt returned `Ok(())` and a closed listener stayed falsely live.
  The review's nice-to-have real session-escape proof independently failed
  0/1 as run `a087238d-ab5b-43a3-a7bf-3d754eb6ae29`: a controlled `setsid`
  child remained alive, but the absent receipt still authorized quiescence.
  Corrected focused run `1de08ac0-fada-41ea-9f56-3aaffe29c79f` passes 6/6
  with bounded escaped-child cleanup. Full sandbox leak-level rerun
  `a3e146d6-eb79-491b-8058-fa56a1af0921` passes 482/482 with 9 expected
  skips and no leak annotation. After the added escape proof, the complete
  affected suite passes 814/814 with 14 expected skips. All-target/all-feature
  affected check, strict Clippy, warning-denied rustdoc, format/diff, verifier
  syntax/ShellCheck, 16/16 self-tests, and expected live 14/1 solely at
  later-owned NNCV005 pass. The new creator file is 554 lines; machine proxy
  production/tests are 1,036/1,420. Docs are green at 108 pages and 17/17
  site conditions. Staging/mirroring and the repeat frozen-tree Sol review
  remain;
- source-derived current modularity counts are: network store/port authority/
  port tests 1,954/1,918/1,977; OCI port manager/tests 1,434/1,875 plus a
  138-line batch-classification child; container runtime/runner/lifecycle/
  launch-cleanup 1,460/1,963/1,943/1,937, with 1,343-line runner-reliability,
  356-line status-callback, and 1,429-line provider-cleanup children; krun
  lifecycle/tests/launch-compensation/startup-fencing
  1,755/1,974/1,395/217; segment/reservation 1,268/891; IPAM/Netavark/tests
  1,743/443/369; PEP production/tests/cleanup 1,464/1,938/432; machine proxy
  production/tests 1,038/1,456; and proxy engine/tests 1,505/1,427. No source
  file reaches 2,000 lines. Every 1,500–1,999 owner is explicitly justified
  above; the 1,977-line port tests, 1,974-line krun tests, 1,963-line runner,
  and 1,954-line network store permit no further inline growth before an
  intact concept-owned extraction;
- strict Clippy first rejected the over-wide registration-compensation
  function. `FailedPepPostAdoption` now carries that exact failure seam, and
  the green rerun proves no lint suppression. The 2,024-line port authority
  became a 1,931-line lifecycle parent plus a 135-line operation-diagnostic
  child. Pass 29 growth moved the complete confirmed-stop receipt state
  machine to a `port_lease/rebind.rs` child; Pass 41 leaves that coherent
  child at 213 lines after deleting mixed-state compatibility while preserving
  homogeneous atomic release and all-terminal replay. Pass 37 removed
  claimless provider-evidence compatibility and extracted the exact-claim and
  tenant-quota test groups, leaving the lifecycle parent below 2,000; Pass 45
  added the adopted-replay child while preserving that bound; the final
  mixed-phase rejection and invalid-sibling proofs leave its private matrix at
  1,977 lines. The
  2,007-line PEP
  matrix became a 1,864-line parent plus a 210-line
  intact registration-failure child. No production authority, store, lock, or
  transition was duplicated;
- `timeout 900 cargo check -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing --all-targets --all-features`: exit 0;
- `timeout 1200 cargo clippy -p nimbus-network -p nimbus-proxy
  -p nimbus-sandbox -p nimbus-testing
  --all-targets --all-features -- -D warnings`: exit 0. The pass-16 run found two
  over-wide constructors and a collapsible transition branch; the
  implementation fixed them at the source with a typed `PortLeaseFence`, an
  adapter-level `OciPortLeaseIntent` that cannot express invalid
  publication/accounting combinations, and the collapsed branch before the
  green rerun;
- `RUSTDOCFLAGS='-D warnings' timeout 900 cargo doc -p nimbus-network
  -p nimbus-proxy -p nimbus-sandbox --all-features --no-deps`: exit 0;
- `cargo fmt --all --check`, `git diff --check`, and
  `git diff --cached --check`: exit 0;
- dependency metadata reports one workspace edge,
  `nimbus-network -> nimbus-core`, and source/effect scans find no socket or
  provider effect in `nimbus-network`;
- the legacy sandbox scan/allocation symbols
  `allocate_missing_bindings_for_tenant`, `allocate_internal_host_port`, and
  `read_used_host_ports` have zero matches in the scoped production owners;
- `bash -n` and `shellcheck -s bash` pass for
  `scripts/verify-nimbus-network-control-plane.sh`;
- verifier self-test: 16 passed, 0 failed. Aggregate verifier: 14 passed,
  1 expected transitional failure, solely NNCV005 for later CLI dev/machine
  SSH allocation migration; NNCV006 production-bind classification passes;
- `bash scripts/check-docs.sh`: 108 pages link-clean, source map resolves,
  private fence intact, titles unique; and
- `bash scripts/verify-nimbus-docs-site.sh`: 17/17 conditions green.

The pre-existing vendored Brotli `unexpected_cfgs`, dead-code, and lifetime
warnings remain visible in ordinary check/test output; strict Clippy for the
four affected crates is green and NNC3.4 did not modify that vendored code.

## Worktree Isolation

Implementation occurred only in the dedicated owner worktree and branch. The
original checkout retained exactly its four pre-existing user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No original-checkout file was modified, staged, discarded, or overwritten. No
push or pull request was performed.

## Independent Review

Every pass used the actual `gpt-5.6-sol` reviewer with `xhigh` reasoning and
fast service mode. The command is:

```text
AUTOREVIEW_ALLOW_NESTED_CODEX=1 \
  autoreview --mode local --engine codex --model gpt-5.6-sol \
  --thinking xhigh --codex-speed fast --stream-engine-output
```

The helper's secret scanner treated deletion of an old inline test fixture as
new secret material. Passes 41 and 42 therefore used
`/Users/jack/src/github.com/nimbus/nimbus-nnc34-review-wt-pass41`, a detached
review-only mirror based at
`6e6655dca71a8f9091c287459a0fc0044ed94b11`. That base omitted only the legacy
inline copy of the already-extracted synthetic redaction test; every candidate
tree was copied byte-for-byte from the canonical index and verified with
`git write-tree`. This changed only review framing; implementation and
verification always ran in the canonical owner worktree.

| Pass | Finding disposition |
| --- | --- |
| 1 | Accepted the lost PEP teardown retry handle and fixed teardown to retain durable assignment evidence. A planning-reservation finding was initially handed to NNC3.8, then narrowed and closed in pass 4 for synchronous proven-no-effect failures. |
| 2 | Accepted and fixed four tenant-boundary defects: published teardown substitution, PEP assignment substitution, `MachinePortProxy` registry aliasing, and `EgressEngine` aliasing. Stable runtime keys and authority checks are tenant-qualified. |
| 3 | Accepted and fixed mapped-IPv6 target panic/scope normalization and plan-only cleanup contacting the machine forwarder. |
| 4 | Accepted and fixed three lifecycle defects: published leases now authenticate exact target/exposure, plan previews remain range authority at runner handoff, and all publication-plus-PEP requests reserve and deterministically compensate as one batch. Ambiguous crash reconciliation remains NNC3.8-owned. |
| 5 | Accepted all three findings at overall confidence `0.96`: confirmed no-effect `Failed` members no longer veto reserved-sibling compensation; krun pre-provider rejection uses never-bound compensation and preserves errors; and VM-config materialization failure compensates both ports and the unrealized segment hold. |
| 6 | Accepted all three findings at overall confidence `0.96`: every post-placement container/krun planning exit now compensates its unrealized segment hold; a later `MachinePortProxy` collision drops then releases earlier siblings; and pre-adoption PEP preparation failures were made compensatable while ambiguous phases remained fenced. Five focused proofs and the 299-test sandbox suite covered the corrections. Pass 17 later replaced phase-derived PEP release with explicit fresh-launch authority. |
| 7 | Accepted both findings at overall confidence `0.96`: PEP trust-anchor publication now follows actual listener ownership, with an independent-registry collision proof that the losing restart cannot delete the live anchor; and serialized manifests now require automatic-listener provenance instead of defaulting a missing field into exact port requests. The focused collision, manifest, and unwritable-root proofs plus the 300-test sandbox suite cover the corrections. |
| 8 | Accepted all four findings at overall confidence `0.98`: persisted PEP and machine requests no longer turn an empty overlapping registry into false provider-stop evidence; PEP release requires atomic exact-attachment removal plus acknowledged shutdown; machine listeners prepare inertly, adopt/activate before serving and exposure, retain exact registry evidence, and drain tracked connections; and the sibling-collision proof keeps both ephemeral selectors held until distinctness is kernel-enforced. The PEP expected-red exited `101` when the old code deleted/released another live registry, then passed after the fix; focused inert-start and overlapping-machine proofs pass; network/proxy are 225/0/0 and sandbox is 302/0/9. |
| 9 | Accepted all five findings at overall confidence `0.98` from actual Sol/xhigh/fast review thread `019f9620-d8c9-7612-84e6-544392b12fda`: machine leases now reserve and evidence the real wildcard socket while retaining external desired exposure separately; matching local registries revalidate exact durable `Active` authority; absent PEP and machine registries no-op only for exact terminal `Failed`/`Released` evidence; and the inertness proof uses a bounded invariant-specific observation loop instead of a fixed sleep. The corrections also extracted the complete machine-listener lifecycle into `runtime/machine_ports.rs`, keeping the container composition root at 1,366 lines. Focused proofs and the full 302/0/9 sandbox suite pass after the changes. |
| 10 | Accepted both P1 findings at overall confidence `0.98` from actual Sol/xhigh/fast review thread `019f963a-2368-70c2-a7a0-efba3879bab1`: the machine-proxy stream copier now retains its acknowledged offset across retryable write backpressure, and the accept loop owns a bounded connection set that reaps completed join handles instead of retaining one per historical connection. The implementation also caps concurrent connection workers at 128 and drops excess accepted sockets without spawning. Deterministic partial-write and capacity-reuse proofs pass, as do the full 304/0/9 sandbox library suite and strict three-crate Clippy. |
| 11 | Accepted both P1 findings at overall confidence `0.95` from actual Sol/xhigh/fast review thread `019f9647-ff2a-7c83-b861-78bb0dfaaf52`: machine-proxy Active validation/start and durable withdrawal now share the registry lifecycle lock, so teardown cannot make already-validated startup effects escape a withdrawn fence; and runner handoff no longer trusts a mutable list of automatic listener names. It requires canonical requested bindings and recomputes the complete image-derived suffix before re-reserving previews as ranges. The three focused fail-before runs exited `101`, then pass after correction; lifecycle is 14/0/2, planning is 21/0/0, the full sandbox library is 307/0/9, and the all-target check, strict Clippy, rustdoc, format, and diff gates pass. |
| 12 | Accepted all three findings at overall confidence `0.96` from actual Sol/xhigh/fast review thread `019f965d-de63-76c0-9421-414adcf4948c`: restart teardown now retains exact segment and port authority behind an explicit restart/final mode; bound PEP pre-adoption cleanup keeps the prepared socket exclusive through trust-anchor removal; and trust-anchor removal failure keeps the `Reserved` lease fenced. All three exact fail-before proofs exited `101`, then pass after correction; egress is 30/0/0, krun VM is 52/0/2, the full sandbox library is 310/0/9 plus binary 2/0/0, and the all-target check, strict Clippy, rustdoc, format, and diff gates pass. |
| 13 | Accepted all three findings at overall confidence `0.97` from actual Sol/xhigh/fast review thread `019f9670-1778-7b83-b273-016ca47a2fac`: bound PEP cleanup now drops its prepared listener after owned-anchor removal but before exact lease release; plan-only pure preview admission now precedes durable segment resolution rather than compensating a nonexistent attachment hold; and every machine-proxy polling timeout is checked before copy-pump admission with operation-specific cause propagated through bounded provider shutdown. The three exact behavioral fail-before proofs exited `101`, then pass after correction; planning is 22/0/0, machine-proxy behavior is 8/0/0, the full sandbox library is 314/0/9 plus binary 2/0/0, network/proxy are 225/0/0, and the all-target check, strict Clippy, rustdoc, format, and diff gates pass. |
| 14 | Accepted all three findings at overall confidence `0.97` from actual Sol/xhigh/fast review thread `019f9683-55ad-7043-b172-d799a77d91a4`: foreign machine teardown now proves exact process-local provider registration before durable withdrawal; PEP and machine socket effects acquire attempt-unique durable `PortBindClaim`s before bind so exact-request replays cannot terminalize another attempt; and krun pure plan-only preview now precedes durable segment resolution. The foreign-withdraw and krun exact fail-before proofs exited `101`; the replay correction is recorded honestly as review/source-trace evidence without a captured pre-fix behavioral exit. New same-request PEP, machine, and real-process claim proofs pass. Owner follow-up also captured and fixed the unlocked gvproxy publication window with an exact `101` fail-before. |
| 15 | Accepted all five P1 findings at overall confidence `0.97` from actual Sol/xhigh/fast review threads `019f96b1-f87f-7360-b21f-41f65ab98c61` and `019f96b7-bfe1-7ff3-b7fc-838aa007b6f1`: acknowledged restart now performs an exact fenced `Active -> Reserved` rebind transition; PEP teardown retains an engine-owned `Stopping` tombstone and retryable shutdown/cleanup evidence; tenant-published quota is checked in the same durable transaction as reservation instead of scanning manifests; machine caller identity must equal manifest handle identity; and live machine reuse compares the complete normalized forwarding plan. Six behavioral expected-red runs exited `101` and now pass. Owner follow-up clarified that plan-only manifests create no durable quota usage, made post-release tombstone replay monotonic, and removed registry-to-tombstone lock inversion; bounded `stopping_attachment_callback_can_reenter_registry_without_lock_inversion` proves a tombstone callback can reenter the registry without deadlock. |
| 16 | Accepted all three P1 findings at overall confidence `0.98` from actual Sol/xhigh/fast review threads `019f96e2-abaa-7b91-a0b1-0b368663fac0` and `019f96e7-59dd-7512-8613-ab0f9d8d7200`: machine restart now preserves an engine-owned `Stopping` tombstone until provider stop, every external unexpose, and atomic durable rebind acknowledge; final cleanup retains the same provider/binding evidence across retry and releases only after confirmed absence; and immutable `PortPublicationIntent` authenticates the exact external host address separately from the real wildcard conflict/bind target. Five behavioral expected-red runs exited `101` and now pass. Owner follow-up made batch rebind atomic, retained partial-start handles for compensation, and fixed the two complete-suite edge cases without weakening coverage. |
| 17 | Accepted all four P1 findings at overall confidence `0.97` from actual Sol/xhigh/fast review threads `019f971a-0bd2-7e83-9dfd-e03b558185f8` and `019f971e-9fbc-7793-835e-c83a03c9c914`: generic withdrawal no longer erases an in-flight durable bind claim; PEP release requires an explicit non-persisted fresh-launch capability instead of inferring authority from `Reserved`; machine activation failure drops every inert listener before exact claim compensation and inspects exact `Active` evidence after acknowledgement loss; and connection-local setup/forwarding failure no longer terminates a still-`Running`/`Active` listener. Four exact behavioral expected-red runs exited `101` and now pass. Owner follow-up added the committed-activation acknowledgement-loss proof, a live second-connection availability proof, and the positive fresh-launch release proof. |
| 18 | Actual Sol/xhigh/fast threads `019f974a-29ef-75a0-b486-96f2c81ed4bc` and `019f974f-703a-7c91-ab95-e07a4ce1874d` reported four findings at overall confidence `0.97`. The two P1 lost-process-registry findings are valid, explicitly fenced NNC3.8 inputs rather than authority for NNC3.4 to infer provider absence; NNC3.8 already owns restart, abandoned-claim, cleanup-pending, and effect-gap reconciliation after every producer migration. Accepted and fixed both P2 findings: PEP preparation now uses an exact per-workload marker instead of retaining the node-global registry lock, and the listener availability test no longer requires a racy shutdown wake connection to succeed. The lock proof failed behaviorally with nextest exit `100`, then the focused engine/listener set passed 13/13, PEP passed 35/35, the complete affected set passed 649/649 with 14 expected skips, and strict affected gates passed. |
| 19 | Actual Sol/xhigh/fast threads `019f975c-e865-7921-892b-44cb1e27cccb` and `019f9765-6f66-7441-9aca-3bcc5fdcf37e` reported five findings at overall confidence `0.94`. Accepted and fixed three P1 lifecycle gaps: restart teardown now passes and authenticates persisted PEP assignment instead of treating an empty registry as absence; PEP activation acknowledgement loss drops the unpublished socket then inspects exact `Active` evidence and prepares same-request rebind; and restart machine preparation retains exact `Reserved` authority through route or bind preparation failures while an explicit fresh-launch capability preserves proven-no-effect release. Accepted the P2 test-proof finding: same-key engine tests synchronize on an observable Condvar-wait boundary, and unrelated-key progress synchronizes on a barrier, so scheduler timeout is no longer evidence. Rejected the P3 indentation claim because exact pre- and post-correction `cargo fmt --all --check` runs are green. Three behavioral fail-before runs exited `100`; the focused correction set is 6/6 and the complete affected set is 651/651 with 14 expected skips. |
| 20 | Actual Sol/xhigh/fast threads `019f977c-828d-7ee0-9144-7c8728d7c770` and `019f9783-f626-7361-be1d-5773b8805513` reported two P2 findings at overall confidence `0.96`; the second chunk was clean at `0.85`. Both are accepted and fixed. Registration commit failure returns the activated PEP and attachment instead of destroying the only cleanup evidence, and sandbox compensation explicitly shuts down, withdraws the trust anchor, authenticates the exact durable binding, and prepares same-request rebind while retaining the original error. Durable port-authority reload now validates publication/accounting consistency rather than trusting a valid checksum alone. Both behavioral fail-before runs exited `100`; the focused correction set is 3/3 and the complete affected set is 653/653 with 14 expected skips. |
| 21 | Actual Sol/xhigh/fast threads `019f9797-1f9e-7743-9c57-4c2ae4aec408` and `019f979b-be46-7bb3-91ec-2486483031fa` produced one clean chunk at `0.76`, then two actionable findings at overall confidence `0.95`. Both are accepted and fixed. Failed registration now installs an engine-owned `Stopping` tombstone before any fallible PEP compensation, so provider/publication evidence survives secondary cleanup failure. Machine provider stop now records a sticky failed state instead of treating a consumed join handle as acknowledged stop, and accept-worker unwind signals and joins all tracked connection workers. Three behavioral fail-before runs exited `100`; the focused correction set is 5/5 and the complete affected set is 657/657 with 14 expected skips. |
| 22 | Actual Sol/xhigh/fast threads `019f97b5-7806-7a40-bbd1-5af765649142` and `019f97bc-82a4-7380-93ff-1f1970457644` reported two P2 findings at overall confidence `0.96`; the second chunk was clean at `0.82`. Both are accepted and fixed. Exact-request replay no longer inherits another coordinator's no-effect release capability: durable `NetworkReservationClaim` authenticates reservation replay, atomic compensation, and crash/reopen recovery, then clears on provider adoption. Retained PEP and machine restart bind failure now abandons only the exact bind attempt and returns to retryable `Reserved`, while fresh-launch no-effect failure remains durably terminal. Two behavioral fail-before runs exited `100`; owner audit added the analogous machine proof, complete-batch pre-effect claim authentication for container and krun, exact runner-root handoff, crossed-claim and semantic-corruption proofs, and projection-error compensation. The focused correction set is 9/9, network is 103/103, sandbox is 351/351 with 9 expected skips, and the complete affected set is 665/665 with 14 expected skips. Strict Clippy's over-wide preparation seam was fixed with a private typed request, and the krun parent test matrix is back below 2,000 lines. Normal-launch crash orphaning and atomic manifest recovery stay with NNC3.8/NNC6. |
| 23 | Actual Sol/xhigh/fast threads `019f97f3-c49c-7541-8c87-a8d6067dbfff`, `019f97f9-2128-7fa2-88d9-b2f46f6708bc`, and `019f97fe-e40d-79b1-b224-16b87e531f53` reported two P1 and one P2 findings at overall confidence `0.97`, then clean chunks at `0.78` and `0.88`. All three are accepted and fixed. Generic withdrawal cannot bypass a claim-owned `Reserved` port lease. The same attempt-scoped `NetworkReservationClaim` now reserves the attachment before IPAM and the complete port batch, then authenticates exact adoption or reverse compensation through explicit `Reserved`, `Held`, and `CleanupPending` segment states; cluster expiry fences adoption but preserves restricted cleanup. Container and krun preserve secondary compensation failure beside the primary launch error. All three exact fail-before tests exited `100`. Owner follow-up also caught an exit-`100` plan-only trust-anchor leak and unified both runner cancellation entry points on exact idempotent cleanup. The complete affected set is 674/674 with 14 expected skips, and strict affected gates pass. |
| 24 | Actual Sol/xhigh/fast threads `019f9834-7ab3-7d80-874d-115a20a79378`, `019f983d-1443-7962-919f-47f8612fffae`, and `019f9840-bd50-7b01-b438-66167312c4f1` reported four P1 and two P2 findings at overall confidence `0.97`, then a clean chunk at `0.84`. All six are accepted and fixed. Runner execution ownership is durable before effects and its effect-owning process performs ordered final cleanup; runner launch failures use execute compensation. Container and krun compensate adopted attachments whose publication/PEP batches remain proven never bound. Exact claimed segment finalization is retryable after acknowledgement/finalization failure without granting generic cleanup, and restart PEP teardown replays only from an exact clean `Reserved` completion state. Four behavioral fail-before runs exited `100`; the runner defects were directly source-traced in the reviewed candidate. Owner audit added preflight artifact cleanup, cleanup-evidence retention, an execution-cleanup concept child, and durable stopped-runner proof. The focused correction set is 8/8 and the complete affected set is 680/680 with 14 expected skips. |
| 25 | Actual Sol/xhigh/fast threads `019f9869-bcb0-7d83-81fe-82548c6b7875`, `019f986f-9bab-7121-bac1-9f4b4fcfe751`, and `019f9874-9438-78a3-b8c4-583253707501` reported eight findings at overall confidence `0.97`. Six are accepted and fixed in NNC3.4: exact coordinator capability authenticates every launch-owned `Reserved` mutation; runner Buildah cleanup configuration and terminal failure evidence are durable; handoff failure compensates manifest, pointer, network, and launch artifacts; fresh PEP registration failure releases only fresh-launch authority; and zero-based previews fail before port-zero rendering. The two P1 crash-convergence findings are accepted as explicit serialized NNC3.8 inputs because current PEP and pre-manifest reservation authority remains safely fenced and NNC3.8 already owns restart, abandoned-claim, cleanup-pending, and effect-gap reconciliation after every producer migration. Owner audit decomposed the hard-threshold port and PEP test modules without duplicating authority. Focused proofs pass 3/3 coordinator, 8/8 lifecycle, 3/3 fixture, and 2/2 extracted registration failure; the complete affected rerun is 682/682 with 14 expected skips; all-target check, strict Clippy, rustdoc, format, and diff are green. |
| 26 | Actual Sol/xhigh/fast threads `019f99af-57a3-7741-b527-ccc5fdf1c2be`, `019f99b5-88c3-7873-86aa-32d313bccb2c`, and `019f99bb-1cf1-7b53-93c8-2e9b1c4a5b0e` reported seven findings at overall confidence `0.97`. Accepted and corrected generic replay across the optional coordinator-claim boundary, poisoned-registry recovery that otherwise made its tombstone unretryable, non-atomic runner execution ownership, and this stale proof status. Rejected the launch-cleanup path claim because the actual `#[path]` module context compiles and all four child tests pass. The repeated lost PEP incarnation and abandoned pre-manifest segment findings remain safely fenced NNC3.8 obligations. New focused proofs pass 1/1 claim matching, 1/1 poison repair, 1/1 exclusive runner claim, and 4/4 extracted cleanup tests. |
| 27 | Actual Sol/xhigh/fast threads `019f99c3-c736-7050-acf2-bfc852a7eea0`, `019f99c7-afd3-7f82-8b21-0a1139def907`, and `019f99cd-367f-7002-8aed-2fec30f394fa` reported six findings at overall confidence `0.98`. Accepted and corrected five NNC3.4 defects: Netavark effects now follow a complete exact durable batch claim and feed one atomic adoption/activation transition; runner Execute/Cancel arbitration is one fingerprinted durable decision published without a partial-read window; acknowledged krun restart teardown atomically returns exact active leases to clean reserved state while failure retains exact active evidence; and empty legacy allocation state no longer fabricates attachment ownership. The krun acknowledgement-to-rebind crash cut remains safely fenced and is recorded as an exact NNC3.8 inspect-before-retry obligation. Rejected the repeated launch-cleanup path claim because the actual `#[path]` context compiles and all eight child tests pass. The first complete correction run exposed five load-only lifecycle failures at 688/693; all were fixed at the atomic-publication or PlanOnly/Execute authority seam. The final affected rerun is 694/694 with 14 expected skips, and all-target/all-feature check, strict Clippy, warning-denied rustdoc, format, and diff gates pass. |
| 28 | Actual Sol/xhigh/fast threads `019f99e7-e327-72b3-9da2-eff5f80621a1`, `019f99ee-54fa-7201-a3bc-f042a71d4ab7`, and `019f99f4-ea3d-7152-8aed-2fec30f394fa` reported seven findings at overall confidence `0.96`; all seven are accepted and corrected. Post-publication pointer failure must win durable Cancel before compensation. Provider-assigned batches prospectively reject sibling conflicts, authenticate every provider before atomic activation, and require binding evidence to match its claim provider. Segment-claim authorization precedes IPAM deletion. A bounded cross-process handoff lock serializes Execute/Cancel/status/inspect, replays an existing Execute after owner loss, and fences PlanOnly observer mutation. Successful PlanOnly previews allocate no attachment and Execute manifests without attachment config fail before effects. The proof and modularity counters are now source-derived. Behavioral fail-before proofs at the three provider-batch/provider-authentication seams, segment/IPAM ordering, pointer arbitration, and both PlanOnly allocation paths exited `101`, then pass; owner-loss and observer-fencing proofs pass. The final affected rerun is 704/704 with 14 expected skips, and all affected quality, verifier, and docs gates pass at the exact counts recorded above. |
| 29 | Actual Sol/xhigh/fast threads `019f9a1f-4993-7672-a3f8-925ba7bbf48b`, `019f9a23-d4c9-7b42-b9c7-efd336412c64`, and `019f9a2a-ce3c-7751-ac1a-0be32e4fbe16` reported four findings at overall confidence `0.98`; all four are accepted and corrected. Ambiguous Netavark setup retains exact claims/IPAM until confirmed detach. A runner may replay an exact durable Execute manifest after owner death only while its decision remains `ClaimedBeforeEffects`; `EffectsStarted` fails closed. A durable exact stopped-binding receipt authorizes final PEP release without allowing a fresh `Reserved` lease to manufacture provider absence. Never-realized segment compensation records an attachment cleanup fence, removes IPAM, and only then finalizes even while siblings keep the segment live. Targeted fail-before runs exited `100` at all four boundaries and now pass. The first complete correction run exposed a stale Cancel classification defect at 711/712; after reading the durable decision before classifying manifest drift, the focused proof passes 1/1 and the complete affected rerun passes 712/712 with 14 expected skips. Modularity cleanup extracted coherent manifest durability and confirmed-stop state-machine children, leaving production roots below 1,500 and explicit deep modules below 2,000. |
| 30 | Actual Sol/xhigh/fast threads `019f9a5b-e8c8-7553-860c-e438ea99f0a2`, `019f9a60-aeac-7b51-b5c8-08b045c0b594`, `019f9a65-8372-7492-b516-5bac87886b83`, and `019f9a69-16cd-74a3-9337-ffe6de447d8a` reviewed the byte-identical 59-path index tree `3d0ed840d1dadc92683d62515a0ce989ce7882d4` under actual Sol/xhigh/fast and reported four findings at overall confidence `0.99`. All three P1 code findings are accepted and corrected: direct Execute durably publishes its exact manifest and ownership fence before effects; Krun natural exit completes exact final network teardown before terminal publication; and failed Krun activation retains the netns and exact retry evidence after unconfirmed Netavark detach. Owner audit additionally serialized PlanOnly status mutation with Execute, split mutable manifest authentication from immutable execution identity, and retained the direct fence across terminal-manifest persistence failure. The fourth P1 correctly observed that the final closeout row was pending in the reviewed bytes and is recorded as the expected review-time procedural state. Focused corrections pass 8/8 plus terminal persistence 1/1; the complete affected rerun passes 720/720 with 14 expected skips; final exact-candidate gates and Pass 31 remain. |
| 31 | Actual Sol/xhigh/fast threads `019f9a8e-f5a3-70c2-a3df-4e1ac24b9e31`, `019f9a94-830d-7d42-90d9-ae3c06c4944f`, `019f9a9a-e25e-7880-9bc4-b456dc6cceab`, and `019f9a9e-dfef-72a1-99fc-cd0f5567f812` reviewed the byte-identical 61-path index tree `aa9d62a42e178cecd4ad5b510b57c78d74eb308a` under actual Sol/xhigh/fast and reported nine findings at overall confidence `0.99`. Seven code findings are accepted, with two scope amendments: only explicit PEP acknowledgement grants absence; one RAII stop executor must own a tombstone and attachment matching must not run under the node-global lock; registration must atomically wait/recheck against stop; each attachment phase must retain the exact selected segment before IPAM; runner start must durably publish a post-effect handoff phase and drop its lock before the workload-lifetime wait (inspect itself was not lock-blocked); and a joined machine accept worker proves absence even when its diagnostic is an error or panic. The port-incarnation finding is rejected for NNC3.4 and recorded for NNC6: public container/krun starts mint fresh ULID-backed sandbox IDs, the current plan explicitly uses that ID as incarnation fence, and terminal same-incarnation resurrection must remain rejected. The final-ledger finding is the expected review-time state. Seven corrections and Pass 32 remain. |
| 32 | Actual Sol/xhigh/fast threads `019f9acd-a5e7-7e93-81c2-9fe0db758557`, `019f9ad3-c587-7573-895f-81973b0010d9`, `019f9ad9-7b48-76d0-8a01-116d5366a3bd`, and `019f9add-04fa-70e2-87f0-0ffb9f7d94ef` reviewed the normalized candidate and reported six findings at overall confidence `0.97`; all six are accepted and corrected. Exact claims are durable before direct or runner attachment/IPAM/port effects, and both paths retain retry authority across failed compensation plus fresh-backend recovery. Runner cleanup persists `Stopping`, retries exact cleanup, and publishes terminal state only after convergence. Machine publication synchronization is observable instead of time-based. Pre-effect IPAM compare-delete is fenced by the exact reservation claim and proves same-address ABA safety. Production network config deserialization requires a stable segment ID. The correction set passes 14/14, sandbox passes 413/413 with 9 expected skips, and the final affected set passes 738/738 with 14 expected skips; every affected quality/static/docs gate is green at the exact results recorded above. |
| 33 | Actual Sol/xhigh/fast threads `019f9afc-81d1-7731-affd-77544dd3e614`, `019f9b01-4835-7e71-bde6-b1faa2a6b894`, `019f9b07-3189-7063-9389-5c898a4f6def`, and `019f9b0f-8203-7883-bc9b-342ae001aaaf` reviewed the exact byte-identical 67-path tree `674fa3ab7225c472b673fa0b2c45ee9b1d0b8aaa` and reported eight findings at overall confidence `0.98`; the fourth chunk was clean at `0.86`. Reject the repeated `launch_cleanup` module-path claim: the canonical sandbox target compiles the actual sibling under its `#[path = "runtime/lifecycle.rs"]` context and executed all 413 tests, including the named cleanup tests. All seven actionable findings are accepted and corrected: execute-start and terminal-publication failures retain exact cleanup authority; krun durably publishes its claim before placement and separates claim-scoped cleanup from adopted broad teardown; fallible two-pump startup joins the first worker; the compatibility reservation adapter is deleted; and fixed accept polling is replaced by a semantic bounded helper. Deterministic correction proofs are included in the 100/100 focused lifecycle, 5/5 explicit stop/locking, 3/3 provider cleanup, and 17/17 machine-proxy sets. |
| 34 | Two independent read-only source audits inspected the corrected runner/machine-proxy and krun seams without editing files or running competing Cargo jobs. The runner audit accepted two findings: cleanup-pending launch failure must remain `Stopping`, and generic runtime command failure must not be classified as absence; it retracted its initial double-cleanup and partial-replay claims after tracing the complete call graph. The krun audit accepted lifecycle-lock coverage, durable `Adopting`, intent-before-cleanup effects, shutdown-intent-before-signal ordering, and exact acknowledgement recovery for ambiguous claim-manifest publication. All are corrected. Its persisted nonterminal recovery-consumer finding is accepted as a serialized NNC3.8 obligation: NNC3.4 now preserves the exact `Reserved`/`Adopting`/`Adopted`/`ProviderOwned` fence and refuses unsafe restart or terminal inference, while NNC3.8 owns fresh-process convergence after every producer migration. Focused 100/100, sandbox 429/429 with 9 expected skips, affected 754/754 with 14 expected skips, all-target check, strict Clippy, warning-denied rustdoc, format, and static-verifier results pass. |
| 35 | Actual Sol/xhigh/fast threads `019f9b65-99fa-7962-ad65-7da6055d3ef9`, `019f9b6b-9e72-75b0-a2f2-b07baf29bb1f`, `019f9b70-173d-7f23-ad21-b513e379f8fe`, and `019f9b75-7356-77b1-85b7-b3a9aafe69ce` reviewed byte-identical 72-path tree `ddb16adb1fb5d6439220337099f21b74b829bedb` and reported seven findings. All are accepted, with the literal fresh-lock failure amended after source tracing while preserving its valid root invariant. Corrections make crun absence exact-ID and exact-whole-diagnostic; retain cleanup-pending runner `Stopping`; acquire the krun lifecycle lock before every launch effect; fence unexpected runtime absence without an exit receipt; order launch-artifact cleanup before network release; authenticate stable-segment IPAM on Netavark teardown; and compensate ambiguous initial attachment reservation. Exact proofs pass 9/9, sandbox 435/435 with 9 expected skips, affected crates 760/760 with 14 expected skips, and strict Clippy, warning-denied rustdoc, format, verifier syntax/lint, 16/16 self-tests, and expected live 14/1 all pass. |
| 36 | Actual Sol/xhigh/fast threads `019f9b96-1eed-7a01-9d34-289d9cd29060`, `019f9b9a-e517-70d2-9100-19f53a5a2b58a`, `019f9ba2-9768-7b71-a78a-cb749aebb39a`, and `019f9ba6-d083-7990-92e2-db9a7fb9ece5` reviewed byte-identical 72-path tree `25f4f70e4ca95ea2e491c3a18ddc77f728c6825b` and reported four actionable findings. All are accepted and corrected. Post-wait runner finalization reacquires and authenticates current durable lifecycle state; handoff staging has bounded names plus write/sync failure cleanup; registration collisions retain exact PEP evidence in an engine-owned quarantine tombstone with fail-closed readiness and independent exact cleanup; and machine-port abandonment validates manager mode and every claim provider before mutation. Fail-before evidence reproduced the stale-finalizer and manager-mismatch behaviors. The exact correction set passes 10/10, sandbox passes 443/443 with 9 expected skips, affected crates pass 769/769 with 14 expected skips, and strict Clippy, warning-denied rustdoc, and format pass. Three independent read-only audits confirmed the finding dispositions and test designs. |
| 37 | Actual Sol/xhigh/fast threads `019f9bcb-3732-7943-8fd3-eaa58fd4bbdc`, `019f9bd1-c79d-7da3-8e39-f3adc123e047`, `019f9bd7-183f-7062-9817-107ab6744043`, and `019f9bda-2de3-79f3-b62c-1cf639ee285d` reviewed byte-identical tree `8ece305a14aaecae672dab3967548926cc25c452` and reported seven findings. Accepted and corrected claimless provider-evidence APIs, the noncanonical crun absence matcher, timing-based lifecycle-lock tests, and the stale NNC0.6a fixture. Rejected the repeated module-path claim using compiled/executed evidence. Accepted durable krun nonterminal convergence as the serialized NNC3.8 obligation and side-effecting inspect/restart as the preserved NNC5.6/NNC6.4a expected-red; both remain safely fenced and were not pulled across their dependency gates. Exact claim/reopen/corruption/process proofs, a strict raw crun diagnostic matrix, and the semantic lock hook now cover the corrections. The expected-red exits `101` at NNCF20 with `left: 1`; the correction matrix passes 6/6, network 110/110, sandbox 444/444 with 9 expected skips, affected crates 771/771 with 14 expected skips, and strict Clippy, warning-denied rustdoc, format, and diff checks pass. Modularity cleanup leaves the production port authority at 1,870 lines and its private matrix at 1,852 with two concept-owned 40/150-line children. |
| 38 | Actual Sol/xhigh/fast threads `019f9bf9-d0a8-7f20-b404-d8bc2b2e0a2f`, `019f9c02-36f5-78c1-a8bf-02cf0761a00e`, `019f9c07-a379-7bd3-88f2-6cf69c243753`, and `019f9c0d-2ac3-7483-96fa-21197762ebab` reviewed byte-identical tree `78f75df17567fb30f05d41fa18872a8570f97f47`. Two chunks were clean. Three P1 findings are accepted as the serialized NNC3.8 fresh-process convergence consumer for process-local machine ports and durable krun `Reserved`/`Adopting` states; NNC3.4 safely retains their exact nonterminal fences. The fourth P1 is accepted and corrected in this item: immutable reservation claims now cross the OCI network plan into live and terminal IPAM evidence, and every setup/teardown/high-level cleanup authenticates the exact attachment generation before effects. The same-address ABA fail-before exited `101` with replacement address `10.89.0.2`; focused stale-generation proofs pass 12/12, sandbox 448/448 with 9 expected skips, affected crates 775/775 with 14 expected skips, and all affected quality/static/docs gates pass. The krun generation proof moved intact to an 87-line concept child, leaving its parent at 1,959 lines. |
| 39 | Actual Sol/xhigh/fast threads `019f9c37-fa3d-7fd0-b0c0-0ea2f90c1e24`, `019f9c3c-6af1-7f71-99a7-0e5acacecb5f`, `019f9c41-edfc-79c1-a84e-7dc732603f38`, `019f9c46-587c-76b0-82af-e4261ae68bd3`, and `019f9c48-ba28-7200-9d3a-29271e9f2f19` reviewed byte-identical tree `747b242c1424af04a3abef6fa7193f16757b5301`; two chunks were clean. Accepted and corrected fail-open Netavark status removal: its fail-before exited `101`, and the corrected proof retains live IPAM on removal failure plus rejects a terminal replay with a conflicting replacement projection. Rejected the repeated module-path claim using compiled/executed canonical-target evidence. Accepted machine fresh-process convergence as NNC3.8 and side-effecting inspect/restart as NNC5.6/NNC6.4a while preserving current fences. Rejected a lower-crate provider-stop token after exact call-graph review: network cannot certify an upper provider effect, and every production transition is reached only through private effect-owner adapters after acknowledged stop/detach/unexpose; a nominal caller-minted token would add no evidence and duplicate authority. The corrected affected set passes 776/776 with 14 expected skips and all affected quality/static gates pass. |
| 40 | Actual Sol/xhigh/fast threads `019f9c59-c555-7970-a77a-eafdfebe13bb`, `019f9c60-c344-7c61-8432-089f7155346a`, `019f9c68-f6cb-7f23-bb96-19c3e5e3c86f`, `019f9c6e-cab6-70b0-8f10-f04895bbc971`, and `019f9c72-e50e-7633-bb96-19c3e5e3c86f` reviewed staged tree `e8a28c38bd9d3cc49741223c6e8c778545e5738c`; one chunk was clean at `0.84`. Six code/test findings are accepted and corrected: container setup enters exact detach compensation; krun restart cleanup classifies exact durable port records rather than coarse workload authority; confirmed-stop release is an atomic batch that recovers mixed legacy `Released` and current `RestartRetained` siblings; runner contention proof waits at the actual `WouldBlock` branch; terminal IPAM retry evidence is exact, bounded, compare-deleted, and reconciled from terminal manifests at startup; and an authenticated newer never-realized generation cannot be blocked by an older terminal generation when no live IPAM exists. Krun fail-before exited `101`; focused correction proofs and 256-generation churn pass. Fresh-process machine-registry convergence remains NNC3.8. The procedural ledger finding is satisfied by this checkpoint but the repeat review remains required. Affected suites pass 783/783 with 14 expected skips; all-target check, strict Clippy, warning-denied rustdoc, format/diff, and static verifier gates pass. |
| Owner audit after 40 | Four additional load-bearing findings are accepted and corrected before freezing the repeat candidate. First, terminal container inspection propagated natural exit while discarding failed ordered network cleanup, allowing terminal IPAM retirement before the segment/provider effect was final; the manifest now carries required `network_cleanup_complete` finality, every cleanup path sets it only after complete success, inspection propagates cleanup failure, and terminal retirement requires the marker. Second, constructors observed startup-reconciliation failures and discarded them; container and krun backends now retain the aggregate error and fail closed before any new planning or provider effect while leaving cleanup/inspection available. Third, the reconciler trusted manifest-embedded roots and tenant paths; it now requires a regular file at the trusted canonical state-root/tenant/sandbox path and authenticates exact handle, tenant, and derived layout before mutation, so a copied foreign-root manifest cannot mutate foreign authority. Fourth, confirmed-stop release lacked an explicit all-or-nothing invalid-sibling proof; the new behavioral test rejects the batch without mutation. The natural-exit proof exposed a real runner crash-cut hang because mutable cleanup finality entered immutable execution identity; a process sample identified the fingerprint mismatch, and normalizing the finality marker out of immutable identity fixed the root cause. Atomic batch proofs pass 2/2, startup reconciliation 3/3, startup admission 2/2, natural exit 1/1, and the exact terminal-manifest crash-cut 1/1. The complete affected set passes 789/789 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, and the expected live 14/1 result pass. |
| 41 | Actual Sol/xhigh/fast threads `019f9cc5-a073-7d20-8742-eeac3f85c1c4`, `019f9cca-17f5-78e0-aa41-505ab197bb4a`, `019f9cce-9b7d-75a3-af08-454d0e1dc52d`, `019f9cd4-dc5a-7472-9d8c-50b459ca543d`, and `019f9cda-af1a-7803-972f-915df798a88e` reviewed byte-identical 76-path tree `4f99449c3d822b11df7cf3e4f6995732d2670922`; chunk five was clean at `0.91`. The helper first refused two secret-like hunks around a moved synthetic PEP redaction test; no model ran on those attempts. The test now uses explicit synthetic fixture values and three independently diagnosed assertions, passes 1/1, and a detached review-only base `6e6655dca71a8f9091c287459a0fc0044ed94b11` omitted only the legacy copy while the reviewed candidate tree remained exact and its concept-owned child was fully included. Six findings were reported. Accept both mixed-release findings as one bug class: the portable authority and sandbox classifier now reject heterogeneous terminal/retained batches before mutation, retain homogeneous atomic release, and allow only all-terminal idempotent replay. Accept the namespace-observation finding: `symlink_metadata` now distinguishes explicit `NotFound` from every metadata error, which preserves provider status and live IPAM. Reject the repeated launch-cleanup path claim because the canonical target compiled and executed its child tests. Accept the inspection resurrection and fresh-process Active PEP findings as the already-fenced dependency-serialized NNC5.6/NNC6.4a and NNC3.8 consumers; this item neither infers provider absence nor crosses those gates. Focused corrections pass 4/4; the affected set passes 790/790 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format, and diff pass. A repeat review is required because accepted findings changed code. |
| 42 | Actual Sol/xhigh/fast threads `019f9ce6-20ac-7a30-8b2b-836d31e084b3`, `019f9cea-18c4-7860-ae23-b91356292b19`, `019f9cef-c43f-7602-aee5-6c4c705c5a24`, `019f9cf4-c3f7-70d0-8dad-e69fc96aceff`, and `019f9cf7-3d29-79c0-8342-a4d8781bcb9d` reviewed byte-identical 76-path tree `98a90d439ebf3cb74356645fff32f042a60227d3`. Seven findings were reported. Accept and correct three code classes: direct effect-fence persistence is bounded at four attempts while preserving durable `ClaimedBeforeEffects` replay authority and performing no provider effect on exhaustion; natural nonzero runtime exit durably records `Failed` with its exit code while an explicit-stop winner remains terminal; and machine proxies bind and evidence the exact requested IPv4 or IPv6 address instead of broadening it to a wildcard. Reject the repeated launch-cleanup module-path claim using canonical compile and executed-child evidence. Retain the documented 1,874-line PEP private-state-machine test exception: it gained no production logic or generic fixtures. Accept fresh-process Active PEP convergence as the dependency-serialized NNC3.8 consumer and the pending Final row as the required review-time process state. The intact runner reliability group moved to a 359-line concept child. Correction proofs pass 8/8 plus an isolated pure route rerun 1/1; the affected set passes 793/793 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format, and diff pass. A repeat review is required because accepted findings changed code. |
| 43 | Actual Sol/xhigh/fast threads `019f9d14-884a-7a70-849a-291e68762e3d`, `019f9d18-596d-7373-8ac8-18143def38c0`, `019f9d1d-7b50-7f51-9163-5cc680a0ac75`, `019f9d22-7df1-7472-aebd-1428a27d2a22`, and `019f9d26-0f9f-7e23-a53c-2a4868b3bf9a` reviewed byte-identical 78-path tree `0910f4d6de6b02e67d20c3734f8e740cda664174`; the final two chunks were clean at `0.82` and `0.91`. Seven findings were dispositioned. Accept and correct direct `ClaimedBeforeEffects` owner-loss convergence and PEP post-activation acknowledgement/cleanup evidence retention. Reject the repeated launch-cleanup module-path claim and the broad krun double-rebind claim using compiled/executed idempotence evidence, while accepting and correcting the narrower ambiguous runtime-delete/exit-receipt ordering defect. Accept durable krun nonterminal and fresh-process PEP convergence as dependency-serialized NNC3.8 consumers, and side-effecting inspect as the preserved NNC5.6/NNC6.4a expected-red. The four new proofs pass 4/4, complete correction groups pass 30/30, sandbox passes 468/468 with 9 expected skips, and affected crates pass 797/797 with 14 expected skips; the one concurrent-run leak annotation does not reproduce in an isolated 1/1 rerun. All affected quality/static gates pass. A repeat review is required because accepted findings changed code. |
| 44 | Actual Sol/xhigh/fast threads `019f9d49-4330-7f53-85c5-7baccfeff870`, `019f9d4f-a388-7443-b550-73b6afb2a7e5`, `019f9d54-b470-73c2-8f51-2df3c277dd4a`, `019f9d5a-22e8-7620-8251-d41358fc6d8f`, and `019f9d5d-3930-7450-ab6f-4ab81d60eacb` reviewed byte-identical 79-path tree `4548035221851e56205a4e2a2a830f0e8dd57121` and reported seven findings. Accept and correct asynchronous creator owner loss with durable `Pending` handoff, exact process-group containment, and a cleanup fence; accept and correct unbounded runner effect-fence publication with one four-attempt, phase-aware helper shared by runner/direct paths; accept machine fresh-process reconciliation as NNC3.8 and the proof-ledger modularity exception as this plan correction. Reject the repeated module-path claim, test-only runtime-bind citations, and fixed generation/epoch aliasing for NNC3.4 with compiled/source/incarnation-fence evidence. Three read-only audits confirmed the dispositions. Focused corrections pass 10/10 and sandbox passes 476/476. The first affected run exposed a post-kill descendant-reaping window; bounded exact-group absence corrected it, its isolated proof passes 1/1, and the repeat affected run passes 805/805 with 14 expected skips. All affected quality/static/docs gates pass. The repeat review remains required because accepted findings changed code. |
| 45 | Actual Sol/xhigh/fast threads `019f9d80-deb1-73d3-9c3b-b564df5fc982`, `019f9d8a-83ad-72c2-ac64-86f8f00af51d`, `019f9d8f-5a45-7722-b3f9-3c5a5d012aa5`, `019f9d95-8ce2-7823-aaa5-f839accda7be`, and `019f9d9c-6125-74b0-baa5-8e60df524d50` reviewed byte-identical 82-path tree `0245701477291f20a64137d0e1abad24f2df479f` and reported six findings. Accept and correct provider-registration-only Active replay, idle connection-worker failure observation, and the potentially hanging unwind proof. The authority retains exact adopted-attempt evidence and claim-authenticates activation/replay; the accept poll reports a failed worker without future traffic; retained exited providers fail closed before publication; and the unwind proof is independently bounded. Accept fresh-process Active PEP convergence as NNC3.8. Reject the repeated module-path claim with compile/execution evidence and reject krun Starting endpoint projection because its explicit Ready-only observed-endpoint contract is backed by durable desired spec/lease reconstruction. Three read-only audits confirm the source dispositions. Correction proofs pass 2/2 network, 3/3 focused sandbox, 7/7 real-process binding with 2 helper skips, and full sandbox 477/477 with 9 expected skips. The complete affected suite passes 808/808 with 14 expected skips; all affected quality/static/docs gates pass. The repeat frozen-tree review remains because accepted findings changed code. |
| 46 | Actual Sol/xhigh/fast threads `019f9dc4-6654-7bd2-8a2c-81097ce718e5`, `019f9dca-1c52-7c22-b91f-9ef5403906da`, `019f9dce-455f-7280-97a1-0cd900e8b9dd`, `019f9dd0-a7db-7552-84de-93830b0a0c2f`, and `019f9dd6-717a-70f2-a577-6f551b86976b` reviewed byte-identical 83-path tree `35f441855766cb169a45f536cc6201bfc89c98b4`; its final chunk was clean. Accept and correct absent creator-receipt quiescence, unreaped successful creator children, and listener liveness that outlived exact socket ownership. Accept fresh-process machine-port convergence as NNC3.8. Reject the alleged krun final PEP leak by exact classifier/release trace and passing restart/final proof. Two expected-red runs reproduce all corrected defects, including a real `setsid` escape; focused proofs pass 6/6 and affected suites pass 814/814 with 14 expected skips. All affected quality/static/docs gates pass. Staging/mirroring and the repeat review remain because accepted findings changed code. |
| 47 | Actual Sol/xhigh/fast threads `019f9dfc-2169-7bb2-99af-dbe90732e1f6`, `019f9e03-3d6e-75d2-a31a-76bc1676109b`, `019f9e07-cdfc-71d3-9515-83a7b11adf40`, `019f9e0e-02a1-72a3-a69b-9196804b692f`, and `019f9e12-29fc-7830-baa4-3a5a49e9de81` reviewed byte-identical 83-path tree `c0266c9f69c46cd2b7c617ad32e2d561112bfa21` and reported six findings. Accept and correct plan-only callbacks that could mutate direct Execute manifests, unowned krun creator processes, ambiguous terminal publication, and machine-provider listener scope; amend the krun terminal claim to retain a replayable `Stopping` checkpoint and the PEP claim to a safe NNC3.8 reconciliation fence; reject the repeated launch-cleanup path claim with canonical compilation. Expected-red runs `c5d1bc98-7dd9-49b5-be22-55fe901a647f`, `329f95f9-89e5-4316-893a-5458f20e5be7`, and `ec6c649c-b97d-4a41-a712-dec9cf6a1445` each failed 0/1 on the frozen bytes. One shared 448-line conmon creator owner now retains, cancels, validates, and reaps exact attempts for both backends; container coordinator identity is durable and authenticated before locks/effects; krun terminal writes use inspect-after-ambiguous barriers; a claimed PEP cannot be released from absent process evidence; and machine guest listener, external publication, and durable evidence are distinct. The first sandbox run exposed seven stale loopback/coordinator assertions at 485/492; focused run `84e0d2ef-fb0f-4b6f-a60e-20c9ec2fad4d` passes 7/7, isolated run `8ce71617-fe3d-4dd9-8ec4-416d5c29aaa9` passes 1/1 without a leak, sandbox run `68466b0d-4aca-4c23-a780-edcbb8fd42ff` passes 492/492 with 9 expected skips, and affected run `257e4bf6-220f-41ec-be51-cbdd652c6a26` passes 823/823 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, expected live 14/1 solely at NNCV005, docs 108 pages, and site 17/17 pass. The krun wire tests moved intact to a 54-line concept child; all production and test owners remain below 2,000 lines with explicit 1,500–1,999 justifications. Final staging, byte mirror, and repeat review remain. |
| 48 | Actual Sol/xhigh/fast threads `019f9e41-858a-7723-b23a-41ede72b382d`, `019f9e46-63ed-79e2-aac9-59f402fb47f3`, `019f9e4a-f84b-7841-b525-e765d09918ba`, `019f9e52-60ec-77f0-9064-601d3d6dd3c6`, and `019f9e58-044a-7e63-b460-27dee8b054e7` reviewed byte-identical 86-path tree `2b853246f8327118f693b4591b7b4eee8a4a4826`; the final two chunks were clean. Accept and correct creator receipt loss after transient containment-acknowledgement failure, restart-retained MachinePortProxy receipts without process-local registry state, and terminal IPAM failure hidden by manifest readback. Retain fresh-process Active/Withdrawing PEP convergence as dependency-serialized NNC3.8. Reject the krun Starting-endpoint claim because Starting deliberately withholds observed endpoints while desired spec and durable leases retain the exact authority; a new concept-owned proof covers Starting, Ready publication, and NotReady withdrawal. Expected-red runs `fb396049-2629-4689-9a9f-4a2c2f7efae2`, `7acf962d-6338-4f41-8659-7bd4ed284cc5`, and `7636a32a-a96c-4766-a5c7-22def5534421` each failed 0/1. Corrections retain creator evidence for bounded retry, add a provider-specific atomic machine-batch classifier/release seam, and separate manifest acknowledgement from retryable terminal IPAM retirement with a real post-rename crash cut. Focused corrections pass 7/7; sandbox run `40a75026-d9c9-458a-b02e-e5a6a6385354` passes 499/499 with 9 expected skips; affected run `daecb705-a499-419c-b1a0-c3bed49d9e76` passes 830/830 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, expected live 14/1 solely at NNCV005, docs 108 pages, and site 17/17 pass; the first live scan caught and then re-passed an omitted literal `Last green` recovery field. Modularity cleanup leaves the port-manager root at 1,292 lines with 293-line batch-state and 164-line machine-test children; creator is 510 lines; krun lifecycle is an explicitly justified 1,689-line deep lifecycle owner, and its 1,968-line test parent gained only one child declaration while the 91-line endpoint proof and 519-line explicit-stop group remain concept-owned. Exact staging/mirroring, frozen affected rerun, and repeat review remain. |
| 49 | Actual Sol/xhigh/fast threads `019f9e7b-905e-74f2-b1a6-cd84b3b7a948`, `019f9e7d-3f80-7831-92dc-94104c70e2d6`, `019f9e81-c293-7533-88ac-34ed959fad87`, `019f9e87-0bc3-73d1-93e8-16c01adabe42`, and `019f9e8d-6cca-7581-bba1-e780a5803fc6` reviewed byte-identical 89-path tree `956316360b00a099bfa76d6f546a14291bd62793`; the final chunk was clean. Accept and correct two retry-evidence defects: failed restart teardown retains exit/PID/conmon receipts until every teardown and machine cleanup acknowledgement succeeds, and partial unstarted-artifact cleanup retains its exact launch coordinator claim so same-claim network release can replay idempotently. Frozen fail-before tests `failed_restart_teardown_retains_runtime_receipts_for_retry` and `unstarted_artifact_cleanup_failure_retains_claim_for_idempotent_retry` each exited `101`; corrected focused proofs pass 2/2. Reject the repeated `launch_cleanup` module-path claim using compiled/executed canonical-target evidence. Retain fresh-process Active/Withdrawing PEP convergence as dependency-serialized NNC3.8. Reject fixed generation/epoch as an NNC3.4 aliasing defect: public starts mint fresh ULID sandbox incarnations, same-ID fresh reservation conflicts, and restart deliberately reuses exact identity; durable generation promotion remains NNC6. Amend/defer the Netavark preflight concern: ordinary current choreography is incarnation- and lifecycle-lock-fenced, while NNC5.2a/NNC5.4 must add an atomic durable attachment/provider detach-attempt claim and NNC5.6/NNC6.4a removes side-effecting inspect as a competing coordinator. A second read alone is not sufficient. Three independent read-only call-graph audits confirmed these dispositions and changed no paths. The two proofs remain in the 670-line concept-owned provider-cleanup child; production runtime is 1,492 lines, artifact cleanup 167, lifecycle 1,929, and launch cleanup 1,937. Sandbox run `ed092a3b-b45e-45c3-96b3-268b8018f633` passes 501/501 with 9 expected skips; affected run `d0a72c45-73c4-465e-990f-abe24ef915ae` passes 832/832 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, expected live 14/1 solely at NNCV005, docs 108 pages, and site 17/17 pass. Exact staging/mirroring and repeat Sol review remain because accepted findings changed code. |
| 50 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9ea2-a6fa-7f40-babc-a5ce6c519307`, `019f9ea8-bcc1-7ff2-a65b-c1d13dd13ce9`, `019f9eae-f942-7bf1-84bf-258de9d33c98`, `019f9eb5-0fce-70b1-8153-cf129c0e5461`, and `019f9ebb-210e-7dd1-bb6c-9dd3ecd00526` reviewed byte-identical 89-path tree `017845a49d9f7170008d53e160c59841bb2729c6`; two chunks were clean. Accept four correction bands: share one absolute deadline across every PEP preparation/lifecycle wake; use the manifest's required launch-time runner and machine-provider context for existing-workload terminal/restart/relaunch effects; use the persisted Buildah path/unshare context for mounted-rootfs cleanup; and replace unbounded ULID stage accumulation with a bounded fixed stage protected by an independent per-sandbox OS lock, exact legacy-stage reconciliation, startup discovery of stage-only first-write crashes, no read-side effects, and `manifest.json` as the only commit point. Defer the already-owned side-effecting krun inspect finding to NNC5.6/NNC6.4a. Three independent read-only audits confirmed the dispositions and changed no paths. Frozen expected-red proofs fail behaviorally as intended: PEP one-budget run `19b0c4b1-36da-4f6c-a161-ae4fd52db65e`, machine-forwarder drift `20800417-969d-4af3-995d-288f5370f38b`, Buildah drift `b0c76443-bc29-46fa-84b5-ce84fd9a14f0`, and orphan-stage accumulation `0dff643e-7d3c-44ff-90c0-55b9ff7b021b`, each with nextest exit `100`. The scratch mirror was restored exactly to `017845a49d9f7170008d53e160c59841bb2729c6` with 89 staged paths and no unstaged/untracked files; the canonical index stayed on that exact tree throughout. Correction implementation remains in progress. |
| 51 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9f00-900f-7a63-8161-feaf5304e86d`, `019f9f05-b2cb-7e60-87cf-812f5be45bec`, `019f9f0c-3c55-77f3-bf14-e8362e2cd16d`, `019f9f12-dedb-7dd2-8c49-9556a694f3dc`, and `019f9f17-980a-7a62-b4eb-6b009b22c8a5` reviewed byte-identical 93-path tree `6380e81d0bf792b1f2c3aa7bbcc34bedc5ee680b` and reported nine findings. Accept and correct the definitely-unspawned creator false-`Pending` cut through exact `Quiesced` publication/readback, fresh-versus-restart partial machine-start disposition, retired unique-stage compatibility grammar, and exact immutable terminal provider classification. Accept active machine owner death plus true spawned-creator `Pending`, krun `Adopting`, and retained-authority terminal convergence as dependency-serialized NNC3.8 inputs; three read-only audits confirm current behavior fails closed and supply the exact lifetime-lock/effect-receipt, creator identity, allocator observation, and crash-cut proof contracts. Reject both 1,500–1,999 modularity findings as already satisfied by explicit owning-plan exceptions; neither file gained production logic or generic fixtures. The new creator proofs moved intact to a 167-line concept child, leaving provider cleanup at 1,350 lines. Focused run `45c6c837-5b6a-4ce9-8647-3614321a69e3` passes 7/7; sandbox passes 520/520 with 9 expected skips; affected passes 853/853 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, expected live verifier 14/1 solely at NNCV005, docs 108 pages, and site 17/17 pass. Exact staging/mirror, frozen rerun, and repeat Sol review remain because accepted findings changed code. |
| 52 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9f3e-34e5-7ed1-88ad-0dc7b33c0217`, `019f9f42-58e5-76c0-93e7-4abd85c90cb8`, `019f9f49-37df-7122-ae77-fec05cb06ff8`, `019f9f4d-9248-7440-86a7-5e2985cc9482`, `019f9f50-d42a-7c23-af08-17fd723339d4`, and clean final chunk `019f9f56-cea1-7ec3-890b-d3ccaad53360` reviewed byte-identical 94-path tree `f1418057b53ce025778caef640b9ab96266810e7` and reported eleven findings. Accept and correct quarantined registration selection, mutable egress in runner identity plus missing reload serialization, incomplete terminal projection short-circuiting, krun `NotSpawned` cleanup and non-resumable provider-failure compensation, a no-op reload test, exact same-coordinator failed-launch compensation, and mixed `Failed`/`Released` terminal classification. Reject the repeated egress-test modularity claim under the existing exception; reject machine wildcard replacement because guest wildcard listener evidence and exact external publication are deliberately separate; reject claimless failure compensation while correcting the valid same-claim case. Machine-forwarder acknowledgement loss is accepted only as a dependency-serialized NNC3.8 recovery contract: current generic gvproxy responses cannot authenticate provider identity/generation or exact absence, so NNC3.4 remains safely fenced. PEP reload acknowledgement-before-persistence is likewise recorded for exact generation/inspect reconciliation in NNC3.8. Proxy fail-before run `74f5a97f-62a1-4640-ac69-f7d4c9872348` exits `100`; the first grouped sandbox fail-before build exits `101` on its own `PolicyGeneration` assertion-type mismatch before behavior, which is corrected without weakening the test. Three bounded implementation lanes changed disjoint paths and returned exact evidence without staging. Focused run `aa0669a0-dc54-41cf-932f-7be5191bf62c` passes 99/99; the first full sandbox run exposed one load-sensitive one-second barrier plus natural-exit finality integration at 528/530, isolated correction run `3e001c0f-b744-4527-9c0a-9cee989c471e` passes 2/2, sandbox run `551d9030-cac3-47af-8594-1b4364c4bf10` passes 531/531 with 9 expected skips, and affected run `1593b89f-6c66-4bfa-b069-b262a4a03a4d` passes 866/866 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 fail-closed self-tests, expected live verifier 14/1 solely at later-owned NNCV005, docs 108 link-clean pages, and docs site 17/17 pass. Modularity leaves production port manager at 1,369 lines, its explicitly justified test child at 1,730, runner at an explicitly justified 1,619, and krun lifecycle at an explicitly justified 1,815 with provider-failure proofs in a 422-line child. Exact staging/mirror, frozen rerun, and mandatory repeat Sol review remain because accepted findings changed code. |
| 53 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9f84-72a1-7e32-abbf-08f3fd8954af`, `019f9f8a-37bb-7bb1-8a3e-6b6a671fb9ec`, `019f9f91-2fab-7f53-a69d-b54cf5a206c0`, `019f9f97-4c66-7432-bbf9-564bc7fa4722`, `019f9f9b-b154-75d3-bc04-bedfd5c0d02f`, and clean final chunk `019f9f9f-fd8e-71e0-9366-1b9790b54b15` reviewed byte-identical frozen 95-path tree `ef541e7c4ccc147a0b4b1378827098995c68f5f3` and reported twelve findings. Seven direct NNC3.4 defects are accepted and corrected: recycled creator process-group signaling, suppressed state-root metadata errors, matching `Retiring` PEP retry, first-error multi-binding withdrawal, leaked owned rootfs on first manifest-write failure, repeated withdrawal after durable PEP release, and launch-owned Netavark caller-identity bypass. Five findings are accepted only as NNC3.8 recovery contracts because current NNC3.4 behavior fails closed and lacks authenticated fresh-process provider evidence: active machine owner death, post-reap creator handoff durability, krun stopping without an exit receipt, krun `Adopting`, and persisted krun creator `Pending`. The seven behavioral fail-before proofs all fail at their intended assertions: creator and artifact-path exact cargo-test runs exit `101`; proxy `4f59435c-ca9d-45c9-b0b4-953be027e9bf`, PEP replay `9a224a41-8774-4b89-9980-42e6d540ac9c`, machine withdrawal `531b12ed-f978-4dbf-b502-f1af7494cc53`, krun rootfs `d56c6f79-9d44-478a-b2b6-f7882768a2cd`, and caller identity `ec8bc2b7-9246-461a-a59d-36b5a69b5468` each fail 0/1 with nextest exit `100`. The corrected combined matrix `581be679-ecf9-4595-9856-a791c3f72261` passes 7/7; sandbox `90b146d1-8f8d-4084-90f0-ca9ae0dfe714` passes 539/539 with 9 expected skips; affected run `a3216124-a89f-4cf1-b079-caeeb79e03dc` passes 873/873 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format and both diff checks, verifier syntax/ShellCheck, 16/16 fail-closed verifier self-tests, expected live verifier 14/1 solely at later-owned NNCV005, docs 108 link-clean pages, and docs site 17/17 pass. Modularity remains within the recorded contract: production roots stay below 1,500 except the already-justified 1,619-line runner and 1,815-line krun lifecycle; test exceptions remain below 2,000, with provider cleanup at 1,487, port-manager tests at 1,787, krun tests at 1,972, and egress tests at 1,913; this proof is 1,926 lines before the final reviewed-tree update. Exact staging/mirroring, a frozen affected rerun, and mandatory repeat actual Sol review remain because accepted findings changed code. |
| 54 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9fbd-d5f7-79c2-8633-b48bbf097ec1`, `019f9fc3-953a-7500-9a84-e49744a42c91`, `019f9fc8-ec23-7493-9999-ce58b62f0cf2`, `019f9fcd-428e-7351-8d78-d0d7b685f46e`, `019f9fd2-5d74-73b3-ac48-f50ebb5ddb84`, and clean final chunk `019f9fd6-0bf7-7fd3-a67b-f2c4e0c02679` reviewed exact staged 95-path tree `e67293e4ea6d0e8cc563072570ffc78ba6b65e35` and reported six findings. Four direct NNC3.4 findings are accepted and corrected: runtime-observed creator reap establishes same-attempt process-group quiescence before later cleanup may signal; confirmed Netavark detach moves exact published bindings from `Active` to restart-retained `Reserved` with confirmed-stop evidence before consuming runtime receipts; terminal `Failed`/`Released` PEP leases reject Restart while final Release remains idempotent; and generic listener withdrawal/release preauthenticates the entire batch, attempts every unresolved member, preserves successes, aggregates exact failures, and skips completed members on retry. Persisted krun creator `Pending` and attachment `Adopting` are accepted as dependency-serialized NNC3.8 recovery contracts because current NNC3.4 behavior fails closed and lacks authenticated fresh-process provider inspection authority. Behavioral fail-before evidence is exact: creator cargo-test red exits `101`; Netavark `bbfc4e79-5a4d-4931-abc2-4d37d3c322b7` fails 0/1 at `Active` versus `Reserved`; PEP `d594d941-eefd-44b7-8800-e472bf9f410f` fails 0/2 because restart falsely succeeds; teardown `7d3b4574-a6da-47be-8991-b3cb0d89a235` fails 0/2 because later progress is suppressed. Corrected focused proofs pass creator 2/2, Netavark 1/1 (`ee956fe9-bf1b-461e-aa72-e6dc22f2b777`), PEP 2/2 (`92e412cf-a960-403d-ab00-77db82a3eb19`), and teardown 3/3 (`79de6f41-5954-4c88-89d2-9348ed51dfec`); OCI egress passes 45/45 and port manager 38/38. Sandbox `4d0f740c-9727-437a-9be4-785845a397ca` passes 547/547 with 9 expected skips; the affected four-crate gate passes 881/881 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format and both diff checks, verifier Bash/ShellCheck, 16/16 fail-closed self-tests, expected live 14/1 solely at NNCV005, exact `nimbus-network -> nimbus-core` metadata, docs 108 pages, and site 17/17 pass. Direct modularity cleanup moves restart lifecycle ownership from the container composition root into its existing 169-line concept module and places Netavark/teardown proofs in 99-/230-line test children; runtime is 1,406 lines, port manager 1,416, provider-cleanup parent 1,490, and its existing explicitly justified port-manager test parent 1,789. Exact staging/mirroring, a frozen affected rerun, and mandatory repeat actual Sol review remain because accepted findings changed code. |
| 55 | Actual GPT-5.6 Sol/xhigh/fast threads `019f9fef-6fe3-7742-8493-f49709ecee4b`, `019f9ff4-b7f6-7df3-bdc7-247dbd1c7c42`, `019f9ff8-5d5b-7c51-91f4-d90d95b91d66`, `019f9ffd-213b-79a0-a70d-bd6a144c8679`, clean chunk `019fa002-7367-7102-90fc-c447f6fa592c`, and `019fa006-8bdf-75a1-8b2e-8eada4405204` reviewed byte-identical 98-path tree `5115139eb8805763e646e3f0d58adf7d7260b1ae` and reported five findings. Accept and correct three code defects: a resolved preparation marker left installed after registry poison recovery spun without honoring the lifecycle bound; repeated krun natural-exit inspection replayed provider teardown from `Released` and blocked post-publication IPAM witness retirement; and machine cleanup returned early on an empty lease slice before authenticating binding/lease cardinality. Reject the repeated launch-cleanup module-path claim because the exact frozen tree compiled and executed that concept child in the 881-test run. Accept the pending Final-row finding as the required process gate, not a code defect. Fail-before proofs are behavioral and bounded: proxy `a711e04e-558f-4792-a799-420ac99f8314` is terminated by the explicit 15-second command bound with the exact test still spinning; krun `4463c3d5-3e1c-47ac-aeca-49616b9cfd4f` fails 0/1 when a second terminal inspection rejects `Released`; machine `d0b537fe-954d-477c-8040-5ee3af59d4fc` fails 0/1 when truncated authority returns `ProviderOwned`. Corrections detect the impossible resolved marker while the map still contains it, make `Released` natural-exit finalization skip provider effects but republish terminal state through the effect barrier, and authenticate the whole machine set before only an empty/empty set becomes terminal no-effect. Focused runs `ac2d42cf-ea5c-4eb3-86a2-6b389a459490`, `7ca4e90e-521d-4b53-aefc-a9cea033998a`, and `0ea37f62-6850-40d4-b2fb-4147a629b922` pass 1/1 each. The corrected affected run `a039e4a8-ddb6-443b-99be-4cddb0539f83` passes 883/883 with 14 expected skips; all-target/all-feature check, strict Clippy, warning-denied rustdoc, format, and diff pass. Final static/docs gates, exact staging/mirroring, a frozen affected rerun, and mandatory repeat actual Sol review remain because accepted findings changed production code. |
| 56 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa018-0f76-7f21-bd20-60405bc82848`, `019fa01b-6445-7b51-9430-18c3c67b08c0`, `019fa023-9736-7a11-af7c-7413c3107d94`, `019fa027-aa90-7b70-b081-6590c92a157f`, clean `019fa02c-927c-7751-9845-fa4a69560967`, and `019fa030-78d7-7dc1-9ed1-f682403e1f6e` reviewed byte-identical 98-path tree `d622d3c7bdc9efd0db514edb16445316024fbb04`; frozen run `f0c11c99-56f1-47c6-968a-776ef7b3353b` passed 883/883 with 14 expected skips. Five direct findings are accepted and corrected: runner ownership/publication/provider-cleanup convergence is exactly four attempts/three waits with primary errors preserved and no terminal publication before cleanup; first manifest publication creates and fsyncs the trusted ancestor chain before staging; provider-cleanup forwarder observers block on an attempt-unique completion marker with timeouts instead of spinning; Netavark setup/teardown uses durable attempt-specific `Reserved/Provisioning/Ready/Deleting/DetachedProjectionPending/Detached` claims and exact generation/attempt capabilities across provider and projection effects; and assignment-less cleanup cannot borrow a live leased PEP. Persisted krun creator `Pending` and attachment `Adopting` are accepted as dependency-serialized NNC3.8 recovery obligations; current behavior fails closed. The Final-row finding remains procedural. Exact runner fail-before runs `a2e3a4e5-018d-4da5-92ab-81cacc4c6772`, `883bf620-a865-4787-a82b-cf90cc1e59b2`, and `95bc848d-9961-4340-a2a2-d90970c0dd24` each hit the fourth-wait sentinel and exited `100`; leased assignment-less PEP run `92deffcb-4ef5-43f2-962e-a82bb5781889` failed 2/3 while the absent case passed. Focused corrections pass manifest 12/12 (`ab58b387-94b1-4aeb-9b97-9e37003de047`), forwarder 3/3 (`f98fa02d-a515-487a-8c23-2dc5273a80cb`), provider cleanup 22/22 (`9d041d72-62a9-433e-aab6-2f364cc1a119`), egress 50/50 (`0c64e5e5-53dd-4709-86c0-7b71dd226a7e`), runner reliability 19/19, and launch cleanup 32/32. Full gates exposed and corrected one provider-assigned cleanup authentication defect and one stale krun expectation that tried to replay a durably pending `Deleting` effect; strict Clippy exposed and corrected one collapsible branch. Three independent read-only audits found the production runner/manifest/observer, Netavark, and PEP teardown state machines clean. They routed authenticated runner `EffectsStarted` recovery to NNC3.8 and found two direct proof gaps: the new counted projection-retry test runs Netavark exactly once, forces status removal failure after durable provider absence, proves the retry performs zero provider calls, then releases IPAM; and final/restart substituted-port PEP tests prove the durable record, readiness, socket, and trust anchor remain byte-for-byte/live before exact cleanup converges. The Netavark proofs pass 1/1 as runs `9ce3fc9e-13f0-41b6-a624-c8365fea86aa` and `4c5c3f08-2533-43d4-94c2-92624e9b6ec1`; substituted-port focus `d8ed528b-ec67-41e2-97fd-5b9e872343fb` passes 2/2; network/IPAM passes 125/125; and sandbox `8dd610df-72f5-4c77-8722-883e5152e2a8` passes 567/567 with 9 expected skips. The pre-audit affected run `b1f21a6b-e052-47bf-b87e-41bd307cdd2d` passed 899/899 with 14 expected skips; corrected 100-path tree `65b3b25dff7c75e4c82a7f82a8539c285f91be1e` passed 902/902 with 14 expected skips as frozen run `05f238d6-65e8-42b9-aed5-8c2c58c654da`. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format and both diff checks, verifier Bash/ShellCheck, 16/16 self-tests, expected live 14/1 solely NNCV005, exact core-only dependency, docs 108 pages, and site 17/17 pass. Netavark fresh-process `Provisioning`/`Deleting` adoption remains safely fenced for NNC3.8/NNC5.2a/NNC5.4. Exact staging/mirroring, frozen rerun, and mandatory repeat Sol review remain because accepted findings changed production code. |
| 57 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa070-a55a-76e3-94e9-8ca4275d97a9`, `019fa074-d2d2-73c0-a293-031e743262aa`, `019fa078-6f76-75f2-bcc7-03d79439dba1`, `019fa07d-d292-7762-947b-da5ab73dbf28`, `019fa081-b41a-7f71-afcb-77583ab2394c`, and `019fa085-ae4f-7151-baaf-97ff4e0e1a03` reviewed byte-identical 100-path tree `65b3b25dff7c75e4c82a7f82a8539c285f91be1e` and reported nine findings. Accept two direct code defects: conmon `Path::exists` suppressed exit-receipt/pidfile metadata failures into false absence, and krun's first no-replace manifest synced only its leaf before attachment reservation. Reject the repeated launch-cleanup module-path claim because the exact tree compiled/executed that child; reject the Execute stale-endpoint claim because final selected bindings are synchronized into the Starting handle, deliberately withheld until Ready, and now covered by a direct durable-port reselection proof; retain the already-justified port-manager test owner. Route safely fenced krun `Pending`/`Adopting`, Netavark `Provisioning`/`Deleting`, and machine bind-claim fresh-process convergence to NNC3.8/NNC5, which owns authenticated provider inspection. Conmon/krun behavioral red `b6a370bd-339b-45bb-857a-721fbd942388` failed 0/3; krun ancestor red `621b773e-239d-4519-beeb-14b103f21965` failed 0/2 against leaf-only sync. Corrections distinguish only exact `NotFound`, share a narrow provider-state directory-durability seam, and move krun first publication into a concept owner. Combined focus `607c2d1b-4275-4ad7-ba33-def0fa452a6d` passes 8/8; durability/endpoint focus `3c2f3421-fdb8-4b00-a7c1-fb1be61d54ee` passes 3/3; sandbox `f43fc4c2-54e9-4451-b7a0-283bf999c67c` passes 574/574 with 9 expected skips; affected `ac80613d-3b04-45a1-bec6-8a534bd58da2` passes 909/909 with 14 expected skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, format/both diff checks, verifier Bash/ShellCheck, 16/16 self-tests, expected live 14/1 solely NNCV005, and exact core-only dependency pass. The proof header is corrected from stale Pass 52. Exact staging/mirroring, frozen rerun, mandatory repeat Sol review, final docs gates, and item commit remain because accepted findings changed code. |
| 58 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa0a6-3199-7c62-b543-533f6c70c7c4`, `019fa0ab-76e7-7cf0-bf47-1b2598b31d29`, `019fa0b0-963a-7102-8f3f-bd407c344c07`, `019fa0b5-1956-7022-b092-c039b76ef449`, `019fa0bb-02cd-7cc0-bca5-5e41736c054f`, and clean final chunk `019fa0be-58ca-74b3-be4d-56013304df25` reviewed exact 103-path tree `8bd7a766fd888321b25048625b7972339f556a89`; its frozen affected run `1e4a89ec-02ca-4fc0-8acc-ef4557ba2d1b` passed 909/909 with 14 expected skips, docs passed 108 pages and 17/17, and reviewer identity was explicitly `gpt-5.6-sol`/`xhigh`/fast with no fallback. Nine findings were dispositioned. Accept and correct two direct NNC3.4 defects: a dead PID receipt predating the current conmon creator could falsely authorize quiescence, and concurrent creation of one shared provider-state directory component treated the winning `AlreadyExists` as fatal without revalidation. Reject the repeated launch-cleanup module-path claim because the exact candidate compiled and executed that child. Retain safely fenced krun creator `Pending`/attachment `Adopting`, Netavark `Provisioning`/`Deleting`, active/withdrawing PEP, exact never-bound PEP claim, and machine bind-claim fresh-process convergence as explicit NNC3.8/NNC5 obligations: current paths retain exact durable authority and cannot infer absence, while those later items own authenticated prior-process/provider observation. Behavioral fail-before run `74f9a9ff-49db-4730-8cb6-a5dfdd945478` failed 0/2 at the stale-receipt and concurrent-directory assertions. The correction removes only a proven-dead regular receipt before spawning, blocks live/non-regular/unknown receipts before effects, and accepts `AlreadyExists` only after the winner is revalidated as a real directory. Focused run `76454c5d-08b0-4b27-8d2b-a07c4166dc2c` passes 3/3; the complete conmon/durability/creator-persistence matrix `cf874a2e-01d4-474d-bf2e-0b3c97c374a8` passes 32/32; affected crates pass 914/914 with 14 expected skips. All-target/all-feature check, strict affected Clippy, warning-denied rustdoc, and format pass. Exact restaging/mirroring, repeat Sol review, final static/docs gates, and item commit remain because accepted findings changed production code. |
| 59 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa0cc-5f50-70c1-a6f2-559b57fa8c9b`, `019fa0d1-9623-7b40-a40e-1265104e5f21`, `019fa0d7-686f-7fd2-a9e2-e0a9b5b0c95b`, `019fa0dc-b51e-7c33-aff3-5918f274d054`, `019fa0e0-c322-7d93-992f-a10bf085095c`, and `019fa0e5-0796-7632-8813-cdc06f9e0845` reported eleven findings. Accept and correct three direct NNC3.4 defects: creator containment distinguishes leader-retained, leader-reaped, and quiesced phases so post-reap recovery is observation-only and cannot signal a recycled numeric PGID; failed PEP recovery consumes exact slot/commit-failure authority, preserves a foreign preparation, and carries exact quarantines across commit, drop, failure, and independent cleanup; and terminal PlanOnly cancellation inspection authenticates immutable identity plus exact terminal finality under the handoff lock and returns without publication. The recycled-PGID, stale-preparation, and terminal-inspection behavioral tests each exited `101` at the intended assertion before correction. Three independent read-only state audits found no remaining production defect and identified small proof gaps now covered by foreign-preparation cleanup-before-commit, earlier-quarantine carry-forward, and malformed-decision no-write tests. Repeated module-path is rejected by compiled/executed evidence; side-effecting Execute inspection remains NNC5.6/NNC6.4a; fresh-process krun, Netavark, PEP, and machine convergence remains safely fenced NNC3.8/NNC5 work. Focused PlanOnly inspection passes 4/4 and exact PEP audit proofs pass 2/2; full proxy passes 153/153; full sandbox passes 583 with 9 expected ignores; affected crates pass 922/922 with 14 expected skips. All-target/all-feature check, warning-denied rustdoc, format, and strict Clippy pass after Clippy exposed and the implementation corrected a 168-byte registration failure result by boxing only failure-path payload. Modularity moves the four PlanOnly inspection proofs intact to a 202-line concept child, leaving `launch_cleanup.rs` at 1,937 lines and every deep owner within its recorded exception. Exact restaging/mirroring, frozen rerun, mandatory repeat Sol review, final static/docs gates, and item commit remain because accepted findings changed production code. |
| 60 | The byte-identical Pass-59 tree `c542fad6bee4edc61bb74cb6b65c9068ef312003` first passed 922/922 with 14 expected skips from frozen synthetic commit `feda86aae7f12a6a2285be3f9e4afd7ce2319301`. The required repeat review then ran actual GPT-5.6 Sol/xhigh/fast with no fallback in threads `019fa253-febf-71a3-ba9a-12bdeee549bd`, `019fa254-894c-7513-b8a6-21da37aaa7f0`, `019fa25b-9c44-7eb0-8d67-4d50d26e84c4`, `019fa25f-adc3-7873-8989-e6d9df09f20c`, `019fa264-d380-7462-8285-e193039c7b66`, and `019fa26a-126c-7230-876a-fb4972cfc9fe`; the complete persistent result is `/tmp/nnc34-pass59-repeat-review.json`. Eleven findings are dispositioned: accept and correct three direct NNC3.4 defects plus the stale proof header; reject the repeated launch-cleanup module-path claim using the exact compiled/executed tree; and route six fresh-process recovery obligations to NNC3.8/NNC5—persisted krun creator `Pending`, explicitly absent krun runtime without an exit receipt, krun attachment `Adopting`, krun inspect restart after withdrawal, process-local PEP restart convergence, and Netavark `Provisioning`/`Deleting`. The direct corrections prevent PEP primary publication while quarantine exists, serialize Execute inspection on the shared lifecycle lock with a canonical reread, and fence plan-only status cleanup, explicit stop, and natural-exit effects after retained startup-reconciliation failure. Their behavioral proofs failed before correction at 0/1, 0/1, and 0/3, then passed 1/1, 1/1, and 3/3. Full proxy passes 154/154; full sandbox passes 588/588 with 9 expected skips. The first corrected affected run passed 926/927, with its only failure a fixed two-second test-harness completion timeout after a semantically proven krun lifecycle-lock release; widening that bounded completion budget to ten seconds preserves the assertion and passes 10/10 focused repetitions. Two complete affected reruns pass 927/927 with 14 expected skips; the second leak-only run reports no leaked process. All-target/all-feature check, strict Clippy with `-D warnings`, warning-denied rustdoc, format, and both diff checks pass. Verifier syntax/ShellCheck and 16/16 fail-closed self-tests pass; the live verifier is the expected 14/1 solely at later-owned NNCV005. Docs pass at 108 link-clean pages and 17/17 site conditions. Exact restage/frozen proof and mandatory repeat actual Sol review remain before completion. |
| 61 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa284-8b55-7ec2-9cdb-4088b451403a`, `019fa286-4109-7b00-abc3-555e3aec0b37`, `019fa28c-56d7-7b41-8897-c7353c40e761`, `019fa293-5afd-7c32-9980-15da9555c108`, `019fa298-c04e-7d90-8c72-bb60ee5e3e7d`, and `019fa29d-f30d-7542-9e78-28fde75b0e19` reviewed the then-current candidate with no fallback; `/tmp/nnc34-pass60-final-review.json` records all twelve findings. Accept and correct seven direct NNC3.4 issues: callback terminal finality, delayed Ready resurrection, runner handoff mode/pre-effect fingerprint authentication, Netavark Reserved no-effect classification, empty Netavark batch no-effect finality, and machine liveness-before-socket-drop; refresh stale modularity evidence. Reject an unforgeable-stop capability after private-caller/visibility audit and the repeated launch-cleanup module-path claim after exact compilation. Route safely fenced krun Pending/Adopting and Netavark Provisioning/Deleting fresh-process recovery to NNC3.8/NNC5, and side-effecting krun inspect/restart to NNC5.6/NNC6.4a. Three independent Pass-61 audits then found and corrected directly related cleanup-pending Ready resurrection, EffectsStarted desired-state fingerprinting, Failed callback cancellation/cleanup, nonzero stopped-outcome preservation, canonical machine API callback identity, absent-partition no-op authority, and stronger machine liveness proof. Their six-test fail-before matrix was 0/6 and correction is 6/6. Full gates exposed only stale test setup: three CLI tests hand-authored a now-incomplete private manifest and four store tests used a no-op solely to create authority. They now seed through the public machine API or a real mutation without weakening assertions. CLI passes 856/856 with 2 expected skips; affected crates pass 942/942 with 14 expected skips; affected and CLI check/strict Clippy, warning-denied rustdoc, format/diff, verifier syntax/ShellCheck, 16/16 self-tests, and expected live 14/1 solely at NNCV005 pass. Updated docs, exact staging/mirroring, frozen affected/CLI reruns, and mandatory repeat actual Sol review remain because the accepted corrections changed code. |
| 62 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa2df-63d1-7040-82ea-44491d5dc296`, `019fa2e4-5919-7d93-9882-e2002827426c`, `019fa2e8-17ae-7620-9e0e-611c24c5ee19`, `019fa2ec-f97d-7540-b195-c93b77c8c80b`, `019fa2f1-31e8-79f0-b67e-1b5febcecc51`, and `019fa2f5-1329-74c1-a3ef-20bd1b42ce8e` reviewed exact prior candidate tree `3447836e1e31a4b80f9eb1566d5a4ffb0930d534`; `/tmp/nnc34-pass61-final-review.json` records thirteen findings. Accept and correct four direct NNC3.4 defects: failed PEP-slot drop now repairs a poisoned registry only after removing its exact preparation and preserving quarantine; krun inspect/stop fence all lifecycle effects behind retained startup-reconciliation failure; segment quarantine/release atomically compare the exact adoption receipt before mutation; and MachinePortProxy bind failure authenticates provider mode before claim lookup or mutation. Prior-tree run `50633db6-3593-4991-954f-bde7de16b2a5` executed six cases: all five new regressions failed at their intended assertions while the existing container startup analogue passed. Corrected run `f598d4b2-ac13-4ccc-9418-d16f4bebf0e7` passes 6/6. Reject the repeated launch-cleanup path claim because the exact frozen tree compiled and executed that child; retain the explicitly justified 1,943-line PEP test owner; reject the constant-port-generation claim because each public start mints a tenant-qualified ULID incarnation, an existing `PortLeaseId` accepts only the identical immutable request, terminal records are never replaced, and released capacity is reused only under a different stable ID. Route authenticated fresh-process convergence for runner `EffectsStarted`, krun creator `Pending`, attachment `Adopting`, absent runtime without exit receipt, and Netavark `Provisioning`/`Deleting` to NNC3.8/NNC5; route inspect-triggered restart serialization to NNC5.6/NNC6.4a. Corrected affected run `bc29062f-38d7-45c7-a5fc-85756582161d` passes 946/946 with 14 expected skips; CLI run `8e77c7cb-5255-4cae-98c7-0f526d2e9640` passes 856/856 with 2 expected skips. Affected all-target/all-feature and CLI default-feature checks, strict Clippy for both surfaces, warning-denied rustdoc, format, and both diff checks pass. A combined CLI all-features attempt correctly tripped the V8 pointer-compression/shared-target archive guard; the cache was preserved and the plan-required feature surfaces pass separately. Verifier syntax/ShellCheck and 16/16 self-tests pass; the live verifier is the expected 14/1 solely at later-owned NNCV005. Docs pass at 108 link-clean pages and 17/17 site conditions. Exact restaging/mirroring, frozen reruns, and the mandatory repeat Sol review remain because accepted findings changed production code. |
| 63 | Actual GPT-5.6 Sol/xhigh/fast threads `019fa315-6220-7ae0-aa47-b3260383bcd3`, `019fa318-836c-75f0-b725-e9b67c038724`, `019fa31e-8aec-7232-bd57-b0f48db62be4`, `019fa323-73d0-7fa2-bfb1-56a627a16b69`, `019fa327-28d9-7132-aafa-9d88be02861e`, and `019fa32c-a5fb-74a2-92ed-02c8f6ef330d` reviewed exact scanner-safe Pass-62 candidate `954d8e217c382aa7d9cab7ce837beb3f2168bd2d`; `/tmp/nnc34-pass62-final-review.json` records twelve findings at `0.98` confidence with no fallback. Five direct findings are accepted and corrected. A failed PEP registration now retains its exact slot/provider fence until explicit shutdown and acknowledged stop; restart cleanup shares one delete-then-explicit-absence helper with final cleanup; every terminal container replay retries exact IPAM retirement after already-committed manifest publication; Netavark preview uses the portable protocol/realm/address overlap predicate while MachinePortProxy wildcard mode keeps numeric-global exclusion; and PEP tests request provider-assigned port zero without probe/drop. Reject the repeated `launch_cleanup` path claim because the exact candidate compiled and executed that child. Reject the generation/epoch ABA claim for current port authority: a terminal same-ID record cannot be replaced, same-ID fresh reservation conflicts unless byte-identical, and every public start mints a tenant-qualified ULID incarnation; durable rollover remains NNC6. Route container creator `RuntimeObserved`, runner `EffectsStarted`, krun missing-exit/Pending/Adopting fresh-process convergence to NNC3.8 and side-effecting inspection to NNC5.6/NNC6.4a. Fail-before PEP run `37133089-6272-4716-94e2-dcd23f9cb27f` failed 0/1; combined restart/preview run `174d1ea2-5136-42bb-9d38-d40a8ae018d8` failed the two regressions while the machine wildcard control passed. Corrected runs `167b366c-72d9-4e96-bb63-d14c5a45b8ed` and `3e7e01d0-c1c3-4278-b95b-bdadcdea83e7` pass 2/2 and 6/6. Full run `6d175b2f-c716-4d21-895a-20a9bede1aa6` exposed only a stale test fixture at 949/950; supplying canonical explicit-absence evidence passes both restart proofs 2/2 and the complete affected rerun passes 950/950 with 14 expected skips. CLI run `b3f0210b-c7b4-4c61-8296-f26bfd4239cf` passes 856/856 with 2 expected skips. Affected all-target/all-feature and CLI default-feature check/strict Clippy, warning-denied rustdoc, format/diff, verifier Bash/ShellCheck, 16/16 self-tests, expected live 14/1 solely NNCV005, docs 108 link-clean pages, and site 17/17 pass. Strict Clippy found and the item fixed one literal-bool assertion without changing behavior. Source-derived modularity counts remain below 2,000; every changed 1,500–1,999 owner is explicitly justified in the plan, including the 1,620-line planning matrix and 1,528-line machine client. Exact restaging/mirroring, frozen reruns, and mandatory repeat Sol review remain because accepted findings changed production code. |
| 64 | Exact Pass-63 tree `0fdd8f67c5cdfdf30f7c02074217b49dd55a2a42`, synthetic commit `085e6efde9b91a6cc8cb002c1d80691d9dd24d81`, and scanner-safe exact commit `f7b418f952d577754196f9bc5a5f90ac7f93c661` were byte-identical. Frozen affected run `5bb6848b-266d-4904-ba6e-27edd775671d` passed 950/950 with 14 expected skips. The first frozen CLI attempt correctly failed before tests because ignored generated UI and embedded-package payloads were absent; `make build-ui` and `make build-packages` supplied those documented build prerequisites without changing tracked bytes, and frozen CLI run `af52c7fb-d0df-452d-ad25-e7781af6d86c` then passed 856/856 with 2 expected skips. The mirror remained tracked-clean at the exact synthetic commit. Actual GPT-5.6 Sol/xhigh/fast threads `019fa34d-9c10-70d2-8eb3-22e633a72bae`, `019fa351-8180-7272-9250-d8de3b69a993`, `019fa358-be21-7e71-add5-88e756184fc5`, `019fa35d-ad40-75b1-83c6-f008e2a51d78`, `019fa361-446f-7a21-be63-bf47a41fac9b`, `019fa364-324c-7a60-9ebd-95f7b0a29dd2`, and `019fa367-0a06-73d2-8adb-17c4981bfc3e` produced nine findings with no fallback; `/tmp/nnc34-pass63-final-review.json` is the persistent result. Accept one direct NNC3.4 defect: dropping a failed PEP registration commit destroyed the provider/attachment and vacated the exact slot unless every caller explicitly invoked `retain()`. Its behavioral proof exited `101` at the intended attachment-retention assertion. The correction stores one atomic failure payload; explicit `retain()` consumes it, while `Drop` converts it through the same exact preparation capability into an engine-owned stopping tombstone and drops only the temporary stop executor. Explicit and implicit proofs pass 2/2. Reject the repeated launch-cleanup path claim because this exact tree compiled and ran that child. Reject constant generation/epoch ABA because terminal same-ID records are immutable, same-ID fresh requests must be identical, and public starts mint tenant-qualified ULID incarnations; NNC6 owns durable rollover. Route container creator `RuntimeObserved`, krun creator `Pending`, attachment `Adopting`, Netavark pending effects, fresh-process PEP convergence, and inspect-triggered restart to their existing NNC3.8/NNC5.6 owners. Corrected owner affected run `24e26c92-ede6-4ae9-9ba8-a7e27a1081a6` passes 951/951 with 14 expected skips. Affected and CLI checks/strict Clippy, warning-denied rustdoc, and format pass. The proxy engine/test owners are now 1,505/1,427 lines and have an explicit lifecycle-owner justification in the plan. Exact restaging/mirroring, frozen affected/CLI reruns, mandatory repeat Sol review, final static/docs closeout, and item commit remain because the accepted finding changed implementation bytes. |
| 65 | Exact Pass-64 tree `1a8ff5a170c3beaf330b4547aedd9c58975382ca`, synthetic commit `057d68db6fff07044a1a7f5fca27e1c8ac4d5f9b`, and scanner-safe exact commit `9f595b2f13c5fb2b8afdcf464f027512725d5477` were byte-identical. Frozen affected run `1513bc5c-47de-41ac-8135-da1a89525c2f` passed 951/951 with 14 expected skips and frozen CLI run `d8c27fa6-f712-4f25-8c6e-c27861b33189` passed 856/856 with 2 expected skips; the mirror remained tracked-clean. Actual GPT-5.6 Sol/xhigh/fast threads `019fa375-25d8-7183-9dfa-0eca48a04da0`, `019fa377-713d-7571-bac0-b3f1fb6d9634`, `019fa37d-101e-7810-901b-050a6b0fc1a7`, `019fa382-569b-7f71-a68a-e04e14d61d09`, `019fa385-ed18-7991-9c98-ffa53eaa47fa`, `019fa38b-3866-7073-8cbf-a1ef9bc56389`, and `019fa38f-2162-74d2-88f0-c11e664122e3` produced nine findings with no fallback; `/tmp/nnc34-pass64-final-review.json` is the persistent result. Accept and correct four direct classes. `OwnedConmonCreator` now owns its exact prepared receipt and exposes no arbitrary-path cancellation capability; the foreign-receipt expected-red exited `101`, the first module rerun exposed three stale mismatched-path fixtures, and the sharpened unforgeable API passes all 15 creator cases. Reservation-coordinator classification reads one store snapshot rather than synthesizing a generation across per-record reads; its deterministic concurrent expected-red exited `101` and all 3 classifier cases pass. Retained startup failure remains a new-admission/provider-launch fence but no longer vetoes authenticated exact stop or non-restarting terminal cleanup; restart-eligible inspection performs no provider or durable effect. The five rewritten expectations failed 0/5 before correction and the complete container/krun matrix passes 7/7. Proof wording now distinguishes MachinePortProxy's wildcard guest listener from exact gvproxy external publication intent. Route container creator `RuntimeObserved`, krun creator `Pending`, attachment `Adopting`/adopted-before-spawn to NNC3.8 and inspect-triggered restart to NNC5.6/NNC6.4a because current authority remains safely fenced. Owner affected run `6b871280-bb1d-4f24-9445-0ffe890ca61a` passes 955/955 with 14 expected skips; affected/CLI checks and strict Clippy, warning-denied rustdoc, format, and both diff checks pass. Exact Pass-65 staging/mirroring, frozen affected/CLI reruns, mandatory repeat Sol review, final static/docs closeout, and the item commit remain because accepted findings changed implementation bytes. |
| 66 | Exact Pass-65 tree `e9f327f0d9019c21584de097b3f69d6b45ec4367`, synthetic commit `4c8dd7d19f15aff968636f99f9e1bc62ea3c1e25`, and scanner-safe exact commit `4a45ac6fa08ad33fe2e83741d41174c8784e6ff9` were byte-identical. Frozen affected run `1952a43c-c9a3-49ae-a24e-aeab7c193c10` passed 955/955 with 14 expected skips; diagnostic rerun `e3148ced-d960-4824-8706-75c3711ff2cb` also passed 955/955 without a leak; frozen CLI run `44ab480e-70d0-4189-ab30-0dcc0af1c7dd` passed 856/856 with 2 expected skips. Actual GPT-5.6 Sol/xhigh/fast threads `019fa3aa-7f65-7020-8795-0f229c4484b2`, `019fa3ad-cb77-7383-92ae-b09a9f0be23e`, `019fa3b3-272d-7661-a3c0-42a3342d8e04`, `019fa3b8-49d8-7032-8eb5-8c3b442c9266`, `019fa3bb-667e-7852-983b-8ce4a996bac0`, `019fa3bf-ab9f-72c3-84b2-ff0adfc5170c`, and `019fa3c2-8442-7310-ba04-d8861822e486` produced ten findings and three clean chunks with no fallback; because the owner accepted and implemented one small clarification before the run ended, autoreview's source-change guard correctly refused to persist a stale aggregate JSON and requires Pass 66 to repeat against immutable bytes. The repeated nonexistent `launch_cleanup` module claim is rejected by this exact compiled/executed tree. The claimed stale status/stop overwrite was already prevented by `lock_execute_lifecycle` rereading and equality-authenticating the durable manifest after lock acquisition; the accepted clarity/testability improvement routes both production callers through `lock_current_execute_lifecycle_for_backend`, explicitly adopts its authenticated current value, and confines the old guard-only seam to tests. The new status-callback proof first failed before that clarification because the production path did not expose the semantic contention probe, then focused run `56964bf6-a2af-4b83-9407-5c9c479f7caa` passes both status/stop cases 2/2 and proves a concurrent newer manifest remains byte-exact. The claimed completed pre-effect IPAM gap is rejected: `release_reserved_network_launch_without_effect` retires the exact tombstone before returning success, and only that success permits `network_cleanup_complete`; a deterministic attempted reproduction remained green without the proposed production change. Route krun creator `Pending`, attachment `Adopting`/adopted-before-spawn, process-local PEP, Netavark `Provisioning`/`Deleting`, and inspect-triggered restart to their existing NNC3.8/NNC5.6/NNC6.4a owners because current paths retain exact authority and fail closed. Accept the final proof-closeout finding procedurally: it is satisfied only by the immutable Pass-66 review, final gates, and ledger transition. Full sandbox run `49180a78-a637-43dc-9080-cfa7914e91b7` passes 616/616 with 9 expected skips; affected all-target/all-feature check, strict Clippy with `-D warnings`, warning-denied rustdoc, format, and both diff checks pass. Exact Pass-66 staging/mirroring, frozen affected/CLI reruns, mandatory repeat Sol review, final static/docs closeout, and item commit remain because implementation bytes changed. |
| 67 | Exact Pass-66 tree `5515d21128b75c569df12548830c9df44856ba13`, synthetic commit `4871a8df10d67244588460ea55737c1efc411c1f`, and scanner-safe commit `f81de861f783815b3d64875d71654136be56eb72` were byte-identical. Frozen affected run `c7b96ed4-5a64-44ce-aab9-ab2d26045500` passed 957/957 with 14 expected skips and frozen CLI run `0a75f8d0-34b1-4de8-8737-01db478f6020` passed 856/856 with 2 expected skips; the mirror remained tracked-clean. Actual GPT-5.6 Sol/xhigh/fast threads `019fa3ce-4ec9-72e3-bd09-0ef712a43a59`, `019fa3d2-616c-73f1-9db2-99e74a77be71`, `019fa3d7-6b08-7163-947f-15a5ddf1bb62`, `019fa3dd-f53b-7b52-bf98-a536cf13a3c8`, `019fa3e0-c27c-7113-8090-bc5120de04cc`, `019fa3e6-5eec-7e20-933c-def613b98d56`, and `019fa3e9-760d-7481-97d1-f2a433d0c0ae` produced twelve findings with no fallback. Accept one P1: restart reset performed network/provider/listener teardown after runtime deletion failed or absence was not confirmed. Reject the repeated nonexistent-module claim using exact compiled/executed evidence; reject the generation/epoch ABA claim because terminal same-ID records are immutable, fresh same-ID requests must be identical, and every public start mints a tenant-qualified ULID incarnation; reject the real-process provider-attempt claim because adoption separately authenticates the exact durable bind claim and the realized provider resource handle. Route fresh-process krun creator/attachment, PEP, Netavark, machine bind-claim, and cleanup-pending convergence to NNC3.8/NNC5, and accept the final static/docs/ledger finding procedurally. The direct correction moves `delete_runtime_and_confirm_absent` before PEP, machine, Netavark, listener, namespace, or publication teardown while retaining later exact cleanup/retry behavior. Its deterministic expected-red exited `101` at the byte-exact network-authority assertion; corrected focused restart cases pass 5/5 and owner affected run `1449ed0c-6b9f-4cb2-8c31-7864adafbb19` passes 958/958 with 14 expected skips. Affected all-target/all-feature check, strict no-dependency Clippy with `-D warnings`, warning-denied rustdoc, format, and both diff checks pass; emitted warnings are confined to unchanged vendored Brotli dependencies. Exact Pass-67 restaging/mirroring, frozen affected/CLI reruns, mandatory repeat actual Sol review, final static/docs closeout, and item commit remain because production bytes changed. |
| 68 | Exact Pass-67 tree `90d66015483ef40803ba9706645d83a4d9521158`, synthetic commit `db8c99c5dbb8bd3855de2479c5f5b46c2e6e3c0e`, and scanner-safe commit `552556fee82eb58ddae84e6e7264420ebd317327` are byte-identical. Frozen affected run `a85e91a0-a941-4c02-81f3-ce8fdfd051a8` passes 958/958 with 14 expected skips and one diagnostic leak marker. The first frozen CLI attempt failed before tests because the independent target exhausted disk while archiving `nimbus-server`; removing only 42.7 GiB of reproducible Cargo targets from obsolete Pass-63 through Pass-66 mirrors restored capacity, and exact frozen CLI run `4cc93f99-a8b7-49db-8d16-14e7302ba8c5` passes 856/856 with 2 expected skips. Mirror source stayed tracked-clean/exact. Verifier Bash/ShellCheck and 16/16 self-tests pass; live result is the expected 14/1 solely at later-owned NNCV005. Docs pass at 108 link-clean pages and 17/17 site conditions after generating ignored mirror prerequisites. A supplementary read-only exact-tree audit found no direct defect: runtime absence gates every PEP/machine/Netavark/listener/netns/publication effect; runtime failure stays primary; post-absence effects remain exact/retryable; receipts survive until full success; and the regression constructs live PEP, Ready Netavark, Active leases, durable authority, and exact receipts before proving no mutation. Residual process-crash convergence and side-effect-free inspection remain NNC3.8/NNC5. Mandatory autoreview confirmed `gpt-5.6-sol`, `xhigh`, and fast service tier, opened thread `019fa400-a93e-7390-994a-3298d319e7`, then the service rejected pass 1 before analysis because usage is exhausted until 2026-08-01 14:18. No Terra/Opus fallback and no verdict are accepted. Retry that exact review when capacity returns; NNC3.4 remains in progress. |
| 69 | Actual GPT-5.6 Sol/xhigh/fast structured review completed against exact scanner-safe Pass-67 commit `552556fee82eb58ddae84e6e7264420ebd317327` with no fallback; `/tmp/nnc34-pass67-final-review-retry.json` records ten findings. Accept and correct three direct NNC3.4 defects: launch polling now retries ambiguous completed runtime observations while strict cleanup `runtime_state` still rejects them and timeout preserves the last diagnostic; explicit runtime absence maps false Starting/Ready/NotReady projections to `Stopping` without inventing exit evidence, releasing authority, or weakening an already-terminal projection; and krun restart confirms exact runtime absence before PEP, machine, Netavark, listener, namespace, publication, or other network-authority teardown. Route container creator `RuntimeObserved`, krun creator `Pending`, attachment `Adopting`/adopted-before-spawn, fresh-process Active PEP, and Netavark `Provisioning`/`Deleting` convergence to the explicit NNC3.8/NNC5 criteria because NNC3.4 retains/fences exact authority but does not claim authenticated fresh-process recovery. Reject the repeated `launch_cleanup` module-path claim because the exact reviewed tree compiled and executed that child. Accept final frozen/static/docs/ledger closeout procedurally. Exact-prior scratch `/Users/jack/src/github.com/nimbus/nimbus-nnc34-fail-before-pass68` proves the transient-observation regression exited `101`; owner fail-before runs prove the false-Ready case and both restart ordering cases exited `101` at their intended byte-preservation assertions. Corrections pass focused 1/1, 1/1, 1/1, and 2/2 proofs, full sandbox 622/622 with 9 expected skips, and current affected 963/963 with 14 expected skips. Affected all-target/all-feature check, strict no-dependency Clippy, warning-denied rustdoc, format, and both diff checks pass; warnings remain confined to unchanged vendored Brotli code. Verifier Bash/ShellCheck and 16/16 self-tests pass; its live result is the expected 14/1 solely at later-owned NNCV005. Docs pass at 108 link-clean pages and 17/17 site conditions. Exact Pass-68 staging/mirror, frozen affected/CLI reruns, mandatory repeat Sol review, criterion audit, and final item commit remain. |
| 70 | Final actual GPT-5.6 Sol/xhigh/fast review ran with no fallback against exact scanner-safe Pass-68 commit `b8b42ff922304f72a54d88bb7d9537188ab533ed`; `/tmp/nnc34-pass68-final-review.json` records fourteen findings. Accept and correct five direct NNC3.4 defects: direct predecision egress reload cannot cross a retained launch claim; successful runtime-state JSON authenticates the exact runtime ID; exit-receipt metadata errors cannot manufacture absence; failed trust-anchor removal cannot publish clean rebind evidence; and every placement config claim is authenticated before IPAM. Their five behavioral expected-red proofs failed at the intended assertions and now pass. Route runner `EffectsStarted`, krun creator `Pending`, restart-progress/exit-receipt, attachment `Adopting`, and process-local listener recovery to the existing NNC3.8/NNC5 obligations because current authority remains fenced. Reject test-only loopback port zero under the explicit fixture exemption and retain the plan's explicit 1,743-line IPAM deep-owner exception. Correct stale evidence here and satisfy final closeout procedurally; per owner direction, no further review campaign follows the accepted corrections. Full sandbox passes 627/627 with 9 expected skips; affected network/proxy/sandbox/testing crates pass 968/968 with 14 expected skips; the frozen CLI candidate passes 856/856 with 2 expected skips because the final corrections touch only sandbox-owned paths. The written NNC3.4 matrix passes 3/3: `two_real_allocator_processes_expose_sandbox_pep_port_collision`, `machine_port_proxy_rejects_bind_without_port_lease`, and `active_manifest_is_observation_not_host_port_authority`. Affected check, strict Clippy, warning-denied rustdoc, format, and staged/unstaged diff checks pass. Verifier Bash/ShellCheck and 16/16 fail-closed self-tests pass; live result is the expected 14/1 solely at later-owned NNCV005. Docs pass at 108 link-clean pages and 17/17 site conditions. |
| Final | NNC3.4 is complete. Its one 117-path reviewed item commit contains the implementation, fail-before and corrected behavioral proofs, final review dispositions, exact acceptance audit, routing-index update, and recovery-ledger transition. NNC3.5 is the sole active item; no push or PR is authorized. |
