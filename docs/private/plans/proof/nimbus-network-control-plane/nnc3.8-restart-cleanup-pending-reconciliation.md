# NNC3.8 Restart And Cleanup-Pending Reconciliation Proof

Date: 2026-07-28

Status: `complete`

Starting checkpoint:
`17f26c1e576dfc38ee6f435d2556b732ef4ee021`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Scope And Ownership

NNC3.8 closes the restart and ambiguous-cleanup obligations retained by every
NNC3 producer. It does not move sockets, provider commands, process
supervision, Netavark, gvproxy, egress forwarding, workload orchestration, or
provider-specific inspection into `nimbus-network`.

The ownership split is:

- `nimbus-network` owns portable durable request/binding identity, desired
  generation and lease epoch fencing, process-lifetime ownership evidence,
  cleanup-pending state, and exact transition authentication;
- each effect-owning adapter owns its socket/process/provider receipt and the
  inspection that classifies an exact effect as present, absent, or ambiguous;
- the sandbox lifecycle owners compose those two forms of evidence before
  cleanup, retry, promotion, or terminal workload projection; and
- `nimbus-system` remains an observed projection and never becomes recovery
  authority.

The initial workspace dependency invariant remains
`nimbus-network -> nimbus-core`. Filesystem locks and durable portable records
are permitted; socket APIs and provider semantics are not.

## Written Acceptance Decomposition

The NNC3.8 criterion is complete only when every row below has direct
behavioral evidence. Passing one crash-cut does not close the item.

| ID | Required behavior | Current state | Required proof |
| --- | --- | --- | --- |
| A1 | A genuinely fresh process reopens an active lease without inferring that durable `Active` means a live kernel/provider owner. | Reference process-bound path passes. | NNC0.1b crash child is killed while holding a real listener and the fresh recovery child converges through authenticated lifetime evidence. |
| A2 | A lifetime generation lock distinguishes a live process owner from owner death; sandbox-owned receipts still decide provider cleanup. | Passes: portable, server/KV, PEP, MachinePortProxy, Netavark, and machine-listener integrations reject a live foreign owner and grant one exact recovery capability only after owner death; provider effects remain fenced until their owner authenticates absence. | A live-owner recovery attempt fails closed; owner death yields one exclusive recovery capability; external/provider-managed effects remain fenced until exact inspection. |
| A3 | Ambiguous unbind moves to durable `CleanupPending` and prevents reuse. | Passes: process-bound and provider-managed paths retain the exact binding and lifetime evidence in `CleanupPending`, reject conflicting reuse, and release atomically only after authenticated absence. | Lost response/unknown effect tests preserve exact binding and reject conflicting reservation until authenticated absence. |
| A4 | Machine-forwarder withdrawal retains the exact lease/binding and provider-instance generation and accepts only typed `Withdrawn` or authenticated `ExactAlreadyAbsent`. | Passes: launch-time provider instance/generation is manifest-serializable; only a typed receipt authenticating the exact provider instance, generation, endpoint, protocol, and allowed outcome can authorize release. The unauthenticated global `/all` fallback is deleted, and every generic or malformed outcome retains the fence. | Status, EOF, timeout, refusal, and arbitrary text are all `Ambiguous`; only exact typed evidence authorizes release. |
| A5 | Egress reload persists desired and attempt generation; acknowledgement before manifest persistence reconciles by exact inspection. | Passes: desired and exact attempt identity publish before the PEP effect; same-process acknowledgement loss inspects before retry; a fresh process authenticates the dead PEP lifetime, rebinds once, completes the same attempt, and preserves byte-stable replay. | Crash after provider acknowledgement and before manifest persistence converges without rollback or inference. |
| A6 | Abandoned never-bound reservations follow one explicit fenced rule. | Passes: a non-cloneable claim-derived lifetime spans atomic port reservation through canonical container/krun manifest publication; live owners reject recovery, dead owners permit one exact no-effect compensation, and any bind/provider ambiguity stays fenced. | A fresh process releases only a reservation whose exact lifetime and no-effect evidence authenticate abandonment; ambiguity stays fenced. |
| A7 | Pending creator attempts authenticate exact process birth and containment before `Quiesced` or `RuntimeObserved`. | Passes: durable receipts carry the exact attempt, OS birth token, and fresh containment group; container and krun compose only exact runtime/conmon evidence; one killed owner plus two distinct recovery processes proves live, dead-contained, escaped, unknown-birth, missing-receipt, and runtime-observed outcomes. | Live, dead-contained, escaped, and unknown creator outcomes produce distinct fail-closed transitions. |
| A8 | Container runner `EffectsStarted` authenticates the exact handoff and effect receipts before promotion or cleanup. | Passes: the Execute manifest anchors the exact winning handoff generation; lifecycle publication requires its exact result receipt; same-process and killed-owner/fresh-process matrices prove present, absent, ambiguous, and substituted-generation behavior without launch replay. | Substituted generation/receipt is rejected; exact present, absent, and ambiguous outcomes converge idempotently. |
| A9 | Krun `Reserved`/`Adopting` observes the claim-fenced allocator result and releases or promotes idempotently. | Passes: the portable allocator exposes a claim-authenticated read-only reservation state; krun composes that exact state with durable `Adopting` intent; same-process and killed-owner/fresh-process cuts prove one compensation before allocator commit or one promotion followed by provider-aware cleanup after it. | Crash before/after allocator commit proves one promotion or one exact release, never both. |
| A10 | Netavark `Provisioning`/`Deleting` inspects exact provider-generation evidence before complete, compensate, or retry. | Passes: setup and teardown attempts retain exact durable generation evidence; a fresh process compensates a lost setup response without duplicate setup, observes an already-committed delete as exactly absent without replaying it, and permits reuse only after detach plus IPAM release. | Effect-created/response-lost and effect-deleted/response-lost cuts converge without duplicate create/delete or early reuse. |
| A11 | No provider terminal observation becomes workload `Stopped`/`Failed` until every retained launch authority is `Released`. | Passes: both OCI-family manifest writers require manifest-local finality and read-only authentication of every exact port, IPAM, attachment, creator, runner/VMM, artifact, and restart authority before publishing terminal status. | A matrix injects every retained authority and proves terminal projection remains nonterminal until exact release. |

Written acceptance checkpoint: `11/11` pass. The 14 accepted findings from
the corrected-candidate review and the five accepted findings from the final
full-candidate review now have direct behavioral coverage across A1-A4, A6,
A8, A10, and A11. The one container `RestartRetained` finding remains rejected
because the implementation already performs the exact release and
`restart_retained_machine_listener_releases_without_process_registry` passes.
NNC3.8 remains `in_progress` only for one correction-only review of the five
material executable fixes, final ledger staging, and the item commit.

## Restart/Recovery Producer Audit

| Producer/effect owner | Durable authority already present | Recovery gap owned here | Primary source owner |
| --- | --- | --- | --- |
| Server main and sibling listeners | Exact request, bind claim, adopted binding, Active/Withdrawing phase, process-incarnation listener identity. | A dead process releases its kernel descriptors but leaves Active authority; an external adoption cannot be inferred absent from local death. | `crates/nimbus-server/src/listener_lease.rs` |
| Standalone KV listener | Exact request/binding and deliberate ambiguous-drop Active fence. | Generation/epoch are incarnation-local and fresh-process cleanup has no authenticated lifetime handoff. | `crates/nimbus-kv/src/listener.rs` |
| CLI dev/start listeners | Exact prepared/adopted lease ownership follows server adapters. | Unconsumed or cancelled process ownership must converge under the same lifetime rule without restoring probe/drop authority. | `crates/nimbus-bin/src/commands/dev.rs`, `crates/nimbus-bin/src/commands/start.rs`, and server adapter owners |
| PEP listener and sandbox endpoints | Shared host-global leases, exact provider handles, manifest receipts, retained process-local worker ambiguity. | Fresh process loses the worker registry; exact durable receipt plus provider observation must precede release. | `crates/nimbus-sandbox/src/backends/oci/network/proxy.rs` and container/krun lifecycle owners |
| OCI `MachinePortProxy` and machine SSH listener | Exact lease/binding, gvproxy process receipt, confirmed-stop/restart path. | Ambiguous process loss and provider-generation withdrawal need typed inspection rather than status/text inference. | `crates/nimbus-sandbox/src/backends/oci/network/machine_ports.rs`, `crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs`, and `crates/nimbus-cli/src/machine/manager/ports.rs` |
| Egress reload | Desired manifest/provider context and fenced launch lifecycle. | Provider acknowledgement can precede durable manifest publication; restart must inspect the exact attempt. | `crates/nimbus-sandbox/src/backends/oci/egress/` and container/krun lifecycle owners |
| Creator process | Exact attempt receipt and `Pending`/`Quiesced`/`RuntimeObserved` handoff. | Fresh process must authenticate birth identity and containment rather than treat PID absence or text as proof. | `crates/nimbus-sandbox/src/backends/conmon/creator.rs` plus backend manifest adapters |
| Container runner | Durable `EffectsStarted` handoff and effect receipts. | Restart must authenticate the exact handoff before promotion or compensation. | `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs` and its concept-owned children |
| Krun attachment | Durable `Reserved`/`Adopting`/`Adopted` state plus allocator claim. | Restart must observe the exact claim-fenced allocation outcome and converge idempotently. | `crates/nimbus-sandbox/src/backends/krun/vm.rs` and `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs` |
| Netavark IPAM | Durable `Provisioning`/`Deleting` operation attempt and segment/IPAM authority. | Restart needs exact provider-generation observation before completing or retrying. | `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs` and `crates/nimbus-sandbox/src/backends/oci/network/netavark.rs` |
| Workload terminal projection | Exact retained launch/network authority in backend manifests. | All retained authority must be released before `Stopped`/`Failed` is durable or projected. | Container and krun lifecycle/status owners |

This audit preserves the routed seams. Cluster transport, logical service
names, tenant admission/PDP, proxy forwarding, certificate ownership, and
system projections are not NNC3.8 authorities.

## First Fail-Before: Dead Process-Owned Listener

The initial test uses the NNC0.1b subprocess harness, a real loopback
`TcpListener`, a shared durable state root, and a truly fresh recovery child:

```text
timeout 120 cargo test -p nimbus-testing --test network_port_lease \
  fresh_process_reconciles_a_dead_process_owned_listener_before_port_reuse \
  -- --ignored --exact --nocapture
```

Exit: `101` as required for fail-before.

The parent killed the owner with `SIGKILL` only after the child durably
activated the lease and reached
`network.port-lease.active-listener-owned`. The fresh child then proved the
kernel listener absent by binding the exact port, but replacement reservation
failed because the durable record remained `Active`:

```text
expected: recovered:kernel-free:lease-released:replacement-reserved
actual:   recovered:kernel-free:lease-active:replacement-conflict
```

This is the intended gap. Kernel bind success is test evidence only and will
not become a production availability probe or release authority. The
production correction must use an OS lifetime lock tied to the exact durable
lease generation. Provider-managed and externally owned bindings additionally
require their effect owner's exact receipt/inspection.

## Portable Lifetime And Recovery Evidence

The correction adds one stable lock file per lease, a monotonic durable
owner-lifetime generation, a non-cloneable live guard, and a nonblocking
recovery guard. Lock-file existence is never evidence; only exclusive OS lock
ownership plus the exact durable request/generation/epoch/lifetime tuple can
authorize a transition. One file per retained durable lease bounds filesystem
growth and avoids unlink/inode races.

```text
timeout 180 cargo test -p nimbus-network \
  port_lease::lifetime::tests -- --nocapture
```

Result: `13 passed; 0 failed`.

The thirteen focused cases prove:

- a concurrently held guard returns typed `LiveOwner` without changing
  durable bytes;
- dead process-bound ownership enters `CleanupPending`, continues to reject a
  conflicting reservation, releases exactly once, and then permits reuse;
- dead provider-managed ownership remains `CleanupPending`, rejects
  process-death release, and continues to fence the slot; and
- a divergent desired generation cannot substitute its request into
  recovery; and
- explicit reconciliation releases only dead process-bound owners while
  reporting live and provider-managed owners without weakening their fences;
- confirmed-stop rebind clears the prior active lifetime before a new bind
  generation; and
- an exact dead process-bound binding can become one restart-retained slot
  while no-effect compensation authenticates its live lifetime guard.

The full portable authority suite remains green:

```text
timeout 240 cargo test -p nimbus-network --all-features -- --nocapture
```

Result: `132 passed; 0 failed` (`126` unit and `6` integration).

The original ignored crash-cut now passes:

```text
timeout 120 cargo test -p nimbus-testing --test network_port_lease \
  fresh_process_reconciles_a_dead_process_owned_listener_before_port_reuse \
  -- --ignored --exact --nocapture
```

Result: `1 passed; 0 failed`.

The owner binds only after reserve, bind claim, and lifetime acquisition. The
parent kills it at the named boundary. The fresh child acquires the exact dead
generation, publishes `CleanupPending`, completes process-bound release, then
reserves and performs a real replacement bind. The replacement bind is a
post-authority behavioral assertion; it is not a production probe/drop
allocator or absence decision.

## Server And KV Listener Integration Evidence

The direct listener owners now acquire the portable lifetime atomically with
the bind claim before the socket effect, retain the non-cloneable guard for
the complete socket lifetime, and authenticate the same generation while
adopting and activating the binding. Before reserving a new direct listener,
they explicitly reconcile dead process-bound ownership. Externally supplied
descriptors are marked `ProviderManaged`, so local process death never becomes
absence evidence.

An external main-listener provider must additionally persist and replay one
exact `ExternalServerListenerContext` with the inherited descriptor. The
opaque provider incarnation identifies one socket incarnation, and its
resource generation advances on replacement. A rebound socket cannot inherit
authority from an earlier descriptor merely because it uses the same address;
addresses and file-descriptor numbers remain diagnostic rather than identity.

```text
timeout 240 cargo test -p nimbus-server listener_lease::tests --lib
```

Result: `9 passed; 0 failed` (`521` unrelated tests filtered).

```text
timeout 240 cargo test -p nimbus-kv --test network_listener
```

Result: `9 passed; 0 failed; 2 ignored` (both ignored cases are
subprocess-child entry points).

The KV fresh-process parent kills an active listener owner with `SIGKILL`.
The recovery child opens the same authority root, reconciles the exact dead
process-bound lifetime, reserves the same port, and performs a real
replacement bind. The server cases separately prove that provider-managed
external adoption remains fenced after local drop.

## A2/A3 Provider Lifetime And Cleanup-Pending Evidence

The remaining effect owners consume the same portable lifetime seam without
moving provider interpretation into `nimbus-network`.

- Process-bound terminal release clears the binding only after exact owner
  death and descriptor finality.
- Provider-managed terminal release is one atomic, lifetime-authenticated
  batch transition. It preserves the exact historical binding and adoption
  claim as audit evidence while changing every member to `Released`; a partial,
  mixed, stale, or foreign batch mutates nothing.
- A live foreign owner cannot inspect, prepare, release, or borrow another
  generation. Owner death grants one recovery capability, but it grants no
  provider effect authority.
- Present or ambiguous provider evidence enters or remains `CleanupPending`,
  rejects conflicting reservation, and reuses the same generation on retry.
  Only exact absence authorizes atomic release and subsequent reuse.

The Netavark integration proof exercises that complete sequence with one real
portable lifetime:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::port_manager::tests::netavark_lifetime_cleanup::live_owner_rejects_foreign_cleanup_and_dead_owner_stays_fenced_until_exact_absence \
  -- --exact --nocapture
```

Result: `1 passed; 0 failed`. It proves live-owner rejection without mutation,
dead-owner transition to `CleanupPending`, conflict rejection, ambiguous
inspection retention, same-generation retry, atomic exact-absence release,
preserved audit evidence, and reuse only after release.

The complete PortManager slice passes `47` cases with `2` declared
child/characterization ignores. MachinePortProxy lifetime/withdrawal coverage
passes `20/20`; container provider cleanup passes `30/30`; krun launch
compensation passes `22/22`; and CLI machine-port state passes `8` with `1`
declared characterization ignore. Together with the server/KV/PEP and portable
proofs above, these close A2 and A3 without using IP addresses as identity or
letting process death stand in for provider absence.

## A4 Machine-Forwarder Withdrawal Evidence

The pre-correction exact test exited `101`: a bare `HTTP/1.0 200 OK` with an
empty body returned `Ok(())` and could authorize lease release. The corrected
adapter gives every launch-time forwarder config an opaque provider-instance
handle plus resource generation, and that tuple survives manifest
serialization. A response authorizes withdrawal only when it is typed
`Withdrawn`/`ExactAlreadyAbsent` evidence matching provider instance,
generation, exact local binding, and protocol. There is no global-listing or
textual-status fallback. A bare status, malformed or substituted receipt,
wrong provider generation, missing response, exhausted shared deadline, or
unreachable provider stays ambiguous. The existing cleanup state therefore
keeps the exact `Withdrawing` lease/binding fence and cannot advance to
confirmed-stop/release.

```text
timeout 240 cargo test -p nimbus-sandbox \
  backends::oci::network::forwarding::tests --lib -- --nocapture
```

Result: `7 passed; 0 failed`. The matrix directly covers exact typed
withdrawal, exact-already-absent receipt, substituted generation, generic
success status, non-success status, EOF, timeout, connection refusal, and
arbitrary text.

```text
timeout 300 cargo test -p nimbus-sandbox machine_forwarder --lib -- --nocapture
```

Result: `7 passed; 0 failed`. Existing manifest-config drift, ambiguity fence,
multi-binding retry, and provider-shape behavior remains green.

```text
timeout 240 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::lifecycle::machine_proxy_restart_waits_for_external_unexpose_before_rebind \
  --lib -- --exact --nocapture
```

Result: `1 passed; 0 failed`. The exact restart binding remains active while
unexpose is in flight and advances to rebindable state only after exact
provider-state absence.

Affected `cargo check -p nimbus-sandbox -p nimbus-cli --all-targets`, strict
Clippy with `-D warnings`, format, and diff checks pass. The Clippy gate found
and removed one directly owned redundant `PathBuf` conversion in the portable
lifetime seam; no behavior changed.

## A5 Egress Reload Acknowledgement Recovery Evidence

The pre-correction exact test exited `101`: the live PEP acknowledged the new
policy while the injected completion-publication failure left the canonical
manifest at the old deny-all desired policy. The correction publishes the
desired policy, monotonic desired generation, and exact provider-attempt
generation before touching the PEP. `Applying` becomes `Stable` only after the
PEP exposes the same attempt and policy bytes.

The process-local PEP records caller-owned attempt identity separately from
its observed policy generation. Exact replay is idempotent; reuse of an attempt
for different policy bytes and stale generations both fail closed.

```text
timeout 180 cargo test -p nimbus-proxy policy_state::tests --lib -- --nocapture
```

Result: `4 passed; 0 failed`.

```text
timeout 300 cargo test -p nimbus-sandbox \
  backends::oci::egress::tests --lib -- --nocapture
```

Result: `51 passed; 0 failed`.

The same-process acknowledgement-loss test passes `1/1`: after the completion
write fails, the manifest retains desired generation `2`, attempt generation
`1`, and `Applying`; replay inspects the exact live attempt, publishes
`Stable`, and does not advance the PEP generation.

The fresh-process proof uses a distinct child test binary invocation and a
shared durable root:

```text
timeout 240 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::egress_reload_recovery::fresh_process_recovers_acknowledged_egress_reload_without_rollback_or_duplicate_attempt \
  --lib -- --exact --nocapture
```

Result: `1 passed; 0 failed`.

The crash child starts a real PEP, publishes `Applying`, receives the exact
provider acknowledgement, emits
`network.egress-reload.provider-acknowledged`, and parks inside the
post-acknowledgement/pre-completion boundary. The parent kills it there. The
fresh recovery child proves:

- the durable desired policy remains generation `2` with attempt `1`;
- the killed PEP's `ProcessBound` lifetime remains `Active` at generation `1`;
- exact owner-death recovery creates one restart-retained slot and one new PEP
  lifetime at generation `2`;
- the same durable attempt becomes `Stable` without minting attempt `2`; and
- replay preserves both provider policy generation and byte-identical manifest
  state.

PEP sockets, policy forwarding, and provider inspection remain in
`nimbus-sandbox`/`nimbus-proxy`; `nimbus-network` supplies only the portable
process-lifetime fence.

## A6 Abandoned Never-Bound Reservation Evidence

The pre-correction exact test exited `101`: the launch coordinator held an
exact claim-derived process lifetime after reserving a port, yet another
authority handle released that `Reserved` record before the coordinator could
publish its canonical request set. The correction introduces a second,
purpose-specific lifetime token rather than overloading the per-lease provider
lifetime:

- `NetworkReservationLifetimeGuard` is non-cloneable and keyed by the exact
  attempt-unique `NetworkReservationClaim`;
- reservation acquires that lifetime before the atomic batch transaction;
- container and krun retain it until the complete request set is durably
  published in their canonical manifest;
- a live coordinator may compensate its own projection failure only through
  the exact guard;
- a fresh process may compensate only after acquiring the dead claim lifetime
  and revalidating every record as `Reserved`, identical `Released`, or
  terminal no-effect `Failed`; and
- any bind claim, adopted binding, foreign claim, or otherwise ambiguous
  provider state rejects the complete batch without partial mutation.

Lock-file existence is not evidence. Exclusive OS lock ownership supplies
liveness, while the durable attempt-unique claim supplies generation/fencing.
The portable crate learns no manifest, socket, Netavark, PEP, container, or
krun semantics.

Focused portable behavior:

```text
timeout 180 cargo test -p nimbus-network \
  port_lease::reservation_lifetime::tests --lib -- --nocapture
```

Result: `4 passed; 0 failed`. These cases prove live-owner rejection, exclusive
reacquisition only after owner death, exact guarded and idempotent
compensation, foreign lifetime rejection, and bind/provider ambiguity
retention.

The same-process sandbox publication proof passes:

```text
timeout 180 cargo test -p nimbus-sandbox \
  backends::oci::port_manager::tests::launch_reservation_lifetime_spans_canonical_manifest_publication \
  --lib -- --exact --nocapture
```

Result: `1 passed; 0 failed`. Claim-only cleanup is rejected while the
coordinator retains the lifetime; after the canonical publication checkpoint,
the exact no-effect batch releases and preserves durable audit evidence.

The genuinely fresh-process crash proof passes:

```text
timeout 180 cargo test -p nimbus-testing --test network_port_lease \
  fresh_process_releases_only_an_abandoned_never_bound_reservation \
  -- --ignored --exact --nocapture
```

Result: `1 passed; 0 failed`. The parent kills the reservation owner only after
it durably reserves the exact port and reaches
`network.port-lease.reserved-before-manifest-publication` while still holding
the lifetime. A distinct recovery process authenticates owner death, proves
that no bind claim, binding, or failure evidence exists, releases exactly once,
replays byte-stably, and reserves the same slot for a different stable
listener.

Container and krun execute planning each pass their exact integration test
`1/1`: both now end the vulnerable lifetime only after the complete manifest
write succeeds. `cargo check -p nimbus-sandbox --all-targets` passes. The full
sandbox library suite passes `634 passed; 0 failed; 11 ignored`; the ignores
are the explicitly named child-only, future-plan, provider, and scale lanes.
Full `nimbus-network --all-features` passes `126/126` (`120` unit plus `6`
integration).

## A7 Creator Birth And Containment Evidence

Status: `pass`

The fail-before proves the durable `Pending` schema records only a logical
attempt ID and therefore cannot authenticate the OS process whose effects may
outlive the coordinator:

```text
timeout 180 cargo test -p nimbus-sandbox \
  pending_creator_manifest_persists_exact_birth_and_containment_receipt \
  --lib -- --nocapture
```

Result before correction: exit `101`; `0 passed; 1 failed`. The serialized
state was exactly
`{"phase":"pending","attempt_id":"creator-birth-receipt-attempt"}`, with no
process-birth or containment identity. A PID alone is not an acceptable
correction because the operating system may recycle it.

The target seam is intentionally split:

- the shared conmon creator module owns an attempt-scoped receipt containing
  the creator PID, an OS process-birth token, and its fresh process-group
  identity, plus deterministic `Live`, `DeadContained`, `Escaped`, and
  `Unknown` observation;
- container and krun remain the effect-owning adapters that combine exact
  creator containment with their own runtime-state command and conmon receipt;
- `RuntimeObserved` requires the exact runtime ID to be present after creator
  containment is proven absent;
- `Quiesced` requires exact creator containment plus explicit runtime absence
  and a dead attempt-scoped conmon receipt; and
- a pre-spawn intent whose post-spawn receipt was never durably acknowledged
  stays explicitly unknown and cannot authorize provider or network cleanup.

The implementation must retain the clean pre-spawn intent barrier, persist the
birth/containment receipt before waiting on or promoting the provider, and
prove the complete outcome matrix in a genuinely fresh process.

The correction keeps the shared seam transport- and provider-free. A durable
receipt pairs the logical attempt with the creator PID, the platform-native
process-birth token, and its fresh process group. Linux reads `/proc/<pid>/stat`
field 22 and has a parser regression for command names containing `)`; macOS
uses `PROC_PIDTBSDINFO`. Hosts without an implemented stable birth observation
return `Unknown` rather than accepting a numeric PID. The container and krun
adapters retain their own runtime-state commands and dead conmon receipt
authentication.

The same-process recovery matrix is:

```text
timeout 300 cargo test -p nimbus-sandbox creator_recovery \
  --lib -- --nocapture
```

Result: `8 passed; 0 failed; 3 ignored`. The three ignores are the child-only
crash, first-recovery, and drain-recovery entries. The eight executed tests
cover both container and krun composition: exact live authority remains
`Pending`; exact dead-contained plus explicit runtime absence becomes
`Quiesced`; exact dead-contained plus matching runtime identity becomes
`RuntimeObserved`; escaped containment, a substituted live process birth, and
a pre-spawn intent without a receipt remain distinct fences; terminal replay
keeps canonical manifest bytes stable.

The genuinely fresh-process proof is:

```text
timeout 300 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::creator_recovery::fresh_process::fresh_process_authenticates_creator_birth_and_containment_matrix \
  --lib -- --exact --nocapture
```

Result: `1 passed; 0 failed`. The crash child publishes five independent
creator states and is killed only after the receipts are durable. The first
fresh recovery process reports:

```text
network.creator-recovery.fresh:live=fenced:runtime=observed:escaped=fenced:unknown-birth=fenced:intent=fenced
```

The parent then uses semantic release files, never a PID-only signal. A second
fresh recovery process waits for authenticated group absence, publishes the
two exact absent attempts `Quiesced`, preserves the exact runtime-observed
attempt, and keeps the receipt-less intent fenced:

```text
network.creator-recovery.drained:live=quiesced:escaped=quiesced:unknown-birth=dead-contained:intent=fenced:runtime=observed
```

Supporting ownership and persistence gates pass:

```text
timeout 300 cargo test -p nimbus-sandbox \
  backends::conmon::creator --lib -- --nocapture
```

Result: `20 passed; 0 failed`.

```text
timeout 300 cargo test -p nimbus-sandbox \
  creator_persistence --lib -- --nocapture
```

Result: `4 passed; 0 failed`.

```text
timeout 600 cargo check -p nimbus-sandbox --all-targets
```

Result: pass. The deep creator ownership module remains coherent, while the
krun orchestration moved into its concept-owned `vm/creator.rs` child and the
fresh-process proof lives under the concept-owned creator-recovery test child.

## A8 Container Runner Recovery Evidence

The runner deep-module growth gate was satisfied before changing behavior:
its complete immutable execution-identity projection and hashing phase moved
intact from `runtime/runner.rs` to the concept-owned
`runtime/runner/identity.rs` child. The existing runner reliability suite
remained green at `24 passed; 0 failed`, proving that extraction did not change
the current handoff protocol.

The first A8 behavioral regression authenticates the exact gap rather than
merely checking an error message:

```text
timeout 180 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::lifecycle::runner_recovery::effects_started_rejects_substituted_handoff_generation \
  --lib -- --exact --nocapture
```

Exit: `101` as required for fail-before. The fixture publishes a real
`ClaimedBeforeEffects -> EffectsStarted` decision, replaces only its
syntactically valid ULID `decision_id`, and then asks the production phase
validator to authenticate it. The current implementation returns
`Some(EffectsStarted)`:

```text
an unanchored handoff generation must not authenticate EffectsStarted:
Some(EffectsStarted)
```

No provider effect is needed to demonstrate the authority substitution. The
correction anchors the decision generation in the exact Execute manifest and
requires a receipt containing the same handoff ID, a typed `Present` or
`Absent` outcome, and the exact resulting manifest SHA-256 before lifecycle
publication. A substituted decision generation or conflicting receipt fails
closed without changing the manifest or decision bytes.

Provider observation remains in the container/OCI owner. Recovery under the
exact Execute lifecycle lock:

- authenticates exact runtime, creator, attachment/IPAM, Netavark projection,
  port, machine-forwarder, and PEP evidence before promoting `Present`;
- classifies `Absent` only from explicit runtime absence and exact
  no-effect/provider-cleanup authority, then converges cleanup before recording
  the receipt;
- retains the exact `EffectsStarted` decision and all authority on an
  ambiguous provider observation; and
- never calls the initial launch effect again during recovery.

The same-process behavioral matrix passes:

```text
timeout 240 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::lifecycle::runner_recovery \
  --lib -- --nocapture
```

Result: `4 passed; 0 failed`. The cases prove substituted-generation rejection,
exact-present promotion without launch replay plus byte-stable publication
replay, exact-absence compensation plus byte-stable stop replay, and ambiguous
observation preserving the exact manifest and decision.

The fresh-process crash matrix uses a child that durably publishes
`EffectsStarted`, materializes one exact `Present`, `Absent`, or `Ambiguous`
provider state, and is killed while retaining three exact lifecycle locks. A
new recovery process and then a distinct replay process prove one promotion,
one compensation, or one retained ambiguity:

```text
timeout 300 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::lifecycle::runner_recovery::fresh_process::fresh_process_converges_exact_runner_effect_matrix \
  --lib -- --ignored --exact --nocapture
```

Result: `1 passed; 0 failed`. Present recovery reestablishes the PEP and
publishes lifecycle without provider launch. Absent recovery enters through
the production prepared-runner entry point, converges terminal cleanup, and
returns an explicit no-replay outcome. Ambiguous recovery preserves exact
bytes across both fresh processes.

The pre-existing runner reliability owner remains green:

```text
timeout 300 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::lifecycle::runner_reliability \
  --lib -- --nocapture
```

Result: `24 passed; 0 failed`.

The full sandbox library gate initially found two directly related
expectations. One equality assertion predated the required manifest-anchored
handoff generation. The other exposed a real pre-effect path that attempted
to publish an `Absent` provider-effect receipt even though the durable phase
was still `ClaimedBeforeEffects`; it now uses the terminal cleanup manifest as
the no-effect receipt and publishes lifecycle without falsely claiming effects
may have started. Both exact regressions pass `1/1`, and the complete rerun is:

```text
timeout 600 cargo test -p nimbus-sandbox --lib
```

Result: `653 passed; 0 failed; 17 ignored`. The ignores are declared
subprocess-child entry points, allocation characterizations, and later-owned
NNC0.7 fail-before proofs; none is an A8 pass claim.

## A9 Krun Attachment Adoption Recovery Evidence

The first A9 regression changed the pre-existing durable `Adopting` stop fence
into the exact required behavior test:

```text
timeout 180 cargo test -p nimbus-sandbox \
  backends::krun::vm::tests::explicit_stop::adopting_stop_releases_exactly_when_allocator_still_proves_reserved \
  --lib -- --exact --nocapture
```

Exit: `101` as required for fail-before. The durable krun manifest proved one
exact `Adopting` reservation while the allocator still proved `Reserved`; the
old implementation returned its deferred-reconciliation error and retained
the fence instead of performing claim-authenticated no-effect compensation.

The correction adds
`NetworkAttachmentReservationState` and the object-safe
`NetworkSegmentAllocator::inspect_attachment_reservation` capability. It is a
transport-free, read-only view of allocator authority only:

- the tenant, stable attachment ID, and exact reservation claim authenticate
  every observation;
- `Reserved`, `ReservationCleanupPending`, `Adopted`, and
  `ProviderCleanupPending` remain distinct;
- a foreign claim fails closed and does not learn another coordinator's
  state; and
- no namespace, VMM, Netavark, socket, or provider effect is inferred by the
  portable result.

The portable allocator proof passes:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::reservation::tests
```

Result: `7 passed; 0 failed`. The new phase-exact inspection test covers
absent, reserved, adopted, provider-cleanup-pending, foreign-claim rejection,
and byte-stable read-only observation.

Krun owns the composition in the concept-owned
`vm/attachment_recovery.rs` child. It first publishes durable stop intent and
then:

- compensates the exact reservation when the allocator reports `Absent`,
  `Reserved`, or `ReservationCleanupPending`; or
- promotes the same claim to `Adopted` and enters the existing checkpointed
  provider-failure cleanup saga when the allocator reports `Adopted` or
  `ProviderCleanupPending`.

It never retries adoption to discover state and never classifies error text.
The same-process owner suite passes:

```text
timeout 600 cargo test -p nimbus-sandbox --lib \
  backends::krun::vm::tests
```

Result: `109 passed; 0 failed; 5 ignored`. The two direct A9 cases prove exact
reserved compensation and exact adopted promotion followed by provider-aware
cleanup, including byte-stable terminal replay.

The fresh-process matrix kills a child after publishing two durable
`Adopting` manifests: one before allocator commit and one after it. A new
process observes the exact claim-fenced allocator state; startup may
conservatively quarantine the adopted orphan to `ProviderCleanupPending`, but
the same claim continues to prove the adopted side of the cut. A third process
then proves byte-stable terminal replay:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::krun::vm::tests::attachment_recovery::fresh_process_converges_exact_krun_adoption_matrix \
  -- --exact --nocapture
```

Result: `1 passed; 0 failed`. The reserved cut ends `Stopped/Released`; the
adopted cut promotes before the provider-aware saga and ends
`Failed/Released`. Neither path can perform both never-realized compensation
and adopted cleanup.

The affected full gates pass:

```text
timeout 900 cargo test -p nimbus-sandbox --lib
```

Result: `656 passed; 0 failed; 20 ignored`.

```text
timeout 600 cargo test -p nimbus-network --all-features
```

Result: `126 passed; 0 failed` (`120` unit plus `6` integration). The sandbox
ignores are declared subprocess-child entry points, allocation
characterizations, and later-owned NNC0.7 fail-before proofs; none is an A9
pass claim.

## A10 Netavark Provider-Generation Recovery Evidence

The two exact fail-before cases exercised the previously stranded durable
states:

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::netavark::recovery_tests::reopened_deleting_reuses_the_exact_attempt_instead_of_staying_pending \
  -- --exact --nocapture
```

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::netavark::recovery_tests::reopened_provisioning_compensates_the_exact_attempt_without_duplicate_setup \
  -- --exact --nocapture
```

Both exited `101` as required. The pre-correction implementation rejected
`Provisioning` and `Deleting` with an inspect-before-retry diagnostic even
after a fresh owner had authenticated the exact attachment, reservation, IPAM,
and durable provider-operation generation. It could neither compensate a
setup whose response was lost nor continue a delete whose response was lost.

The correction keeps provider semantics in `nimbus-sandbox`:

- `Provisioning` carries the exact setup attempt. Recovery never calls setup
  again; it starts one teardown compensation tied to that setup generation.
- `Deleting` carries both the exact setup generation and exact delete attempt.
  Recovery reuses that same pair rather than minting a replacement attempt.
- `DetachedProjectionPending` retains the same pair until observed status is
  removed, so projection failure cannot erase provider-generation evidence.
- Every transition authenticates the tenant-qualified attachment generation,
  reservation claim, setup attempt, and delete attempt before mutation.
- Provider absence is confirmed before status removal, IPAM release, segment
  release, or host-port reuse. An error leaves the complete authority
  unchanged.

Netavark itself exposes setup and teardown effects rather than a portable
provider-inspection interface. The adapter therefore composes its exact
durable operation generation with provider-owned namespace/effect evidence,
replays teardown only when that exact effect remains present, and completes an
already-committed delete without issuing a duplicate provider command. No
Netavark command, namespace, socket, or provider type enters `nimbus-network`.

The same-process recovery owner passes:

```text
timeout 240 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::netavark
```

Result: `9 passed; 0 failed; 3 ignored`. The three ignored tests are child
entry points for the fresh-process matrix. The focused IPAM state-machine
owner passes `11 passed; 0 failed`.

The fresh-process matrix persists two distinct response-loss cuts before
process exit:

- a `Provisioning` operation whose exact provider evidence is `Present`; and
- a `Deleting` operation whose exact provider evidence is `Absent`.

A new process proves setup compensation retains the lost setup generation,
delete recovery retains the same setup and delete attempts, duplicate setup is
never invoked, an exactly absent delete is not replayed, and replacement
remains fenced. A third process proves terminal bytes are stable and admits
replacement only after exact detach and IPAM release:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::netavark::recovery_tests::fresh_process_converges_netavark_response_loss_matrix \
  -- --exact --nocapture
```

Result: `1 passed; 0 failed`.

The cross-stack krun compensation proof uses a provider stub that fails two
consecutive teardown invocations. Between failures, it compares the complete
tenant-IPAM partition and proves the setup generation, delete attempt, IPAM,
and reservation authority remain byte-identical; host-port claims remain
`Reserved`. The third invocation succeeds, after which cleanup releases the
ports while retaining only the terminal no-effect replay claim:

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::krun::vm::tests::launch_compensation::failed_krun_activation_teardown_retains_retry_evidence_until_confirmed_detach \
  -- --exact --nocapture
```

Result: `1 passed; 0 failed`.

The first complete rerun also exposed two load-sensitive creator fixtures that
allowed their short-lived leader to exit before the exact birth receipt was
captured. Explicit filesystem release gates now make the intended pre-exit
boundary deterministic; the creator recovery owner passes `5 passed; 0
failed`. This is a proof-harness correction only and does not change creator
production behavior.

The complete affected gates pass:

```text
timeout 900 cargo test -p nimbus-sandbox --lib
```

Result: `659 passed; 0 failed; 23 ignored`. The ignores are declared
subprocess-child entry points, allocation characterizations, and later-owned
NNC0.6/NNC0.7 fail-before proofs; none is an A10 pass claim.

```text
timeout 600 cargo check -p nimbus-sandbox --all-targets
```

Result: pass. `cargo fmt --all --check` and `git diff --check` also pass.

## A11 Terminal Projection Finality Evidence

The fail-before was exact:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::container::runtime::tests::lifecycle::terminal_finality::terminal_manifest_publication_rejects_a_retained_port_lease \
  -- --exact
```

Exit: `101`. The pre-correction manifest writer accepted `Stopped` while the
canonical lease record remained `Reserved`, demonstrating that the
manifest-local predicate was not sufficient authority.

The correction adds one OCI-family, read-only composition seam. It authenticates
the exact tenant-qualified port request and lease fence, IPAM generation, and
attachment reservation claim before terminal manifest publication. It performs
no cleanup and cannot infer absence from an address, process exit, error, or
provider status. Provider-specific state remains in the sandbox adapters;
`nimbus-network` retains only portable durable authority.

Container and krun manifest writers now apply two independent terminal gates
before serialization and publication:

1. manifest-local finality rejects retained launch claims, artifacts, creator or
   runner/VMM handoffs, provider-failure cleanup, restart intent, incomplete
   cleanup, and mismatched workload/handle status; and
2. aggregate network finality requires every exact published/PEP lease to be
   terminal, IPAM to be released or absent, and the exact attachment reservation
   to be absent.

The aggregate authority matrix passes:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::finality::tests::
```

Result: `4 passed; 0 failed`. It rejects a `Reserved` port lease, every
reserved/adopting attachment phase, adopted and provider-cleanup-pending
attachments, and live IPAM. Each case accepts only the corresponding exact
released/absent evidence.

The container projection matrix passes:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::container::runtime::tests::lifecycle::terminal_finality::
```

Result: `2 passed; 0 failed`. It proves terminal publication preserves the
prior nonterminal bytes for incomplete cleanup, retained launch claim, retained
artifact, retained restart intent, missing shutdown intent, handle/status
mismatch, and retained canonical port authority. Fully released authority
publishes once.

The krun projection matrix passes:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::krun::vm::tests::explicit_stop::terminal_projection_rejects_every_retained_krun_launch_authority \
  -- --exact
```

Result: `1 passed; 0 failed`. It rejects `Reserved`, `Adopting`, `Adopted`, and
`ProviderOwned` launch authority, active provider-failure cleanup, pending
creator authority, and retained restart intent while preserving the prior
nonterminal bytes. Exact `Released` authority publishes.

The full-suite correction pass also verified five lifecycle regressions
individually: runner finalization remains nonterminal until network finality;
terminal IPAM retirement failure is not confused with manifest acknowledgement
loss; lifecycle-lock reread is ordered after lock acquisition; natural exit
releases exact authority before terminal status; and startup-recovery failure
still permits exact natural-exit cleanup. Result: `5 passed; 0 failed`.
Repeated terminal inspection performs only the required exact read-only
attachment-absence observation and does not replay quarantine, release,
finalization, or provider effects.

Complete A11 gates:

```text
timeout 900 cargo test -p nimbus-sandbox --lib
```

Result: `683 passed; 0 failed; 24 ignored`. The ignored cases remain declared
subprocess-child entry points, allocation characterizations, and later-owned
fail-before proofs; none is counted as A11 evidence.

```text
timeout 900 cargo check -p nimbus-sandbox --all-targets
```

Result: pass. The final affected all-feature/all-target check also passes for
network, sandbox, proxy, server, KV, CLI, and testing. Workspace `make clippy`
passes with `-D warnings`; warning-denied rustdoc passes for those seven
crates.

Final-review-corrected behavioral gates:

- portable lifetime owner: `23 passed; 0 failed`;
- full `nimbus-network`: `142 passed; 0 failed` (`136` unit plus `6`
  integration);
- full `nimbus-proxy`: `158 passed; 0 failed` (`157` unit plus `1`
  integration);
- full `nimbus-sandbox --lib`: `683 passed; 0 failed; 24 ignored`;
- full `nimbus-kv`: `23 passed; 0 failed; 3 ignored`;
- full `nimbus-cli`: `875 passed; 0 failed; 1 ignored`;
- `nimbus-testing --test network_port_lease`: `6 passed; 0 failed; 2 ignored`,
  where both ignores are subprocess child entry points rather than parent
  proofs; and
- the server broad lane reports `505 passed; 25 ignored; 3 filtered` after
  excluding the two independently reproduced main-branch baseline failures,
  plus two listener timeouts only under aggregate load. Each listener case
  passes exactly in isolation (`1/1` and `1/1`), so the broad lane is not
  misreported as a single green aggregate; and
- the two excluded server failures reproduce unchanged from
  `17f26c1e576dfc38ee6f435d2556b732ef4ee021`:
  `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` returns
  `400` instead of `200`, and
  `cloud_functions_passes_runtime_owner_lifecycle_conformance` returns `409`
  instead of `200`. They are not NNC3.8 listener or recovery regressions.

The one planned source-census refresh reconciles `67/67` production
bind/allocation/ownership occurrences and `35/35` classified non-authority
risks across the same 26 logical sites. The risk count falls by one because
the corrected forwarding adapter deletes a retired authority-shaped helper;
there is no unclassified production bind. The verifier self-test passes
`44/44`.
Live verification reports `14 passed; 1 failed`: NNCV006 now passes, and the
only red condition is NNCV005 for the legacy `PortManager` type whose deletion
is explicitly NNC3.9-owned. This is the intended band boundary, not an NNC3.8
exception.

The census classifies the creator launch gate's two Unix pipe endpoints as
creator-local control IPC, never TCP/UDP allocation or listener authority.
Docs pass at `108` link-clean pages and `17/17` site conditions. Final format
and staged/unstaged diff checks run after this evidence update and before the
review snapshot.

## Implemented Recovery Seam

The portable lifecycle enforces these constraints:

1. An effect owner acquires one exact lease-lifetime generation before its
   bind/provider effect and retains the non-cloneable guard for that effect's
   complete live lifetime.
2. The guard authenticates lease ID, desired generation, lease epoch, and a
   monotonic owner-lifetime generation. It grants no socket or provider
   operation.
3. Another process cannot acquire recovery authority while the lifetime lock
   is held. After owner death, exactly one process can hold the recovery guard.
4. Owner death alone never proves an external/provider-managed effect absent.
   It permits the adapter to inspect its exact durable provider handle.
5. `Present` retains/fences; `Absent` may complete exact cleanup; `Ambiguous`
   enters or remains `CleanupPending`. No generic error or textual response is
   absence evidence.
6. Direct process-owned listeners may use the stronger invariant that the
   effect and lifetime guard share one process owner, but each adapter must
   prove it never lets the guard die before every duplicate descriptor.
7. All transitions are idempotent, stale generation/epoch/lifetime tokens are
   rejected, and nonterminal records continue to fence conflict slots.

Small capability/state-machine types are preferred over a provider god
interface. `nimbus-network` will not learn gvproxy, Netavark, PEP, egress,
creator, runner, or VMM semantics.

## Dirty Checkpoint

Current NNC3.8-owned paths:

- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`
- `crates/nimbus-network/src/lib.rs`
- `crates/nimbus-network/src/segment.rs`
- `crates/nimbus-network/src/port_lease.rs`
- `crates/nimbus-network/src/port_lease/error.rs`
- `crates/nimbus-network/src/port_lease/lifetime.rs`
- `crates/nimbus-network/src/port_lease/lifetime/tests.rs`
- `crates/nimbus-network/src/port_lease/reservation_lifetime.rs`
- `crates/nimbus-network/src/port_lease/reservation_lifetime/tests.rs`
- `crates/nimbus-network/src/port_lease/operation.rs`
- `crates/nimbus-network/src/port_lease/rebind.rs`
- `crates/nimbus-network/src/port_lease/tests.rs`
- `crates/nimbus-network/src/state_store.rs`
- `crates/nimbus-server/src/listener_lease.rs`
- `crates/nimbus-server/src/construction.rs`
- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-kv/src/listener.rs`
- `crates/nimbus-kv/tests/network_listener.rs`
- `crates/nimbus-testing/tests/network_port_lease.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs`
- `crates/nimbus-sandbox/src/backends/oci/network.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/support.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/forwarder_observer.rs`
- `crates/nimbus-cli/src/machine/api.rs`
- `crates/nimbus-cli/src/machine/api/tests.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/egress_reload.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/planning.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/egress_reload_recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/manifest_durability.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_lease.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_manager.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_manager/netavark_lifetime.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_manager/tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_manager/tests/netavark_lifetime_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/proxy/tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/assignment.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/reload.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/tests/post_activation_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/tests/registration_failure.rs`
- `crates/nimbus-proxy/src/lib.rs`
- `crates/nimbus-proxy/src/policy_state.rs`
- `crates/nimbus-proxy/src/worker.rs`
- `crates/nimbus-proxy/src/worker/policy_reload.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/start.rs`
- `crates/nimbus-sandbox/src/backends/conmon/creator.rs`
- `crates/nimbus-sandbox/src/backends/conmon/creator/attempt_annotation.rs`
- `crates/nimbus-sandbox/src/backends/conmon/creator/recovery.rs`
- `crates/nimbus-sandbox/src/backends/conmon/creator/recovery/tests.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/creator.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/direct_execution.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/launch_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner/identity.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner/recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/creator_persistence.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/creator_recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/creator_recovery/fresh_process.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/runner_recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/runner_recovery/fresh_process.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/cluster.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/dto.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/finality.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/finality/tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/netavark.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/netavark/recovery_tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/segment.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/segment/cleanup.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/segment/reservation.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/test_support.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/attachment_recovery.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/creator.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/attachment_recovery.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/explicit_stop.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/launch_compensation.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/launch_compensation/restart_fencing.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/lifecycle_locking.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/provider_failure_recovery.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/support.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/creator_recovery.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/natural_exit.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/terminal_finality.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/runner_reliability.rs`
- `crates/nimbus-cli/src/machine/manager/tests/stop_cleanup.rs`
- `crates/nimbus-cli/src/start/tests/adapters.rs`
- `crates/nimbus-cli/src/machine/manager/ports.rs`
- `crates/nimbus-cli/src/machine/manager/stop.rs`
- `crates/nimbus-cli/src/machine/manager/tests/ports_state.rs`
- `crates/nimbus-proxy/src/tests/policy_lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/conmon/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/network_launch.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/absent_runtime_projection.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/execute_inspection.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/netavark_restart.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/status_callbacks.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/cleanup.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/proxy.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_manager/batch_state.rs`
- `docs/private/plans/README.md`
- `docs/private/plans/nimbus-network-control-plane-plan.md`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc3.8-restart-cleanup-pending-reconciliation.md`

The portable slice is joined by the first effect-owner integration:
`nimbus-server` and `nimbus-kv` direct listeners plus the KV process
integration proof. Their socket ownership remains local; they only acquire and
retain the portable guard, invoke explicit dead process-bound reconciliation
before a new reservation, and keep external adoption provider-managed.
The first sandbox effect-owner slice is admitted for A4's machine-forwarder
response-classification correction: the provider codec/config, the two
existing cleanup behavior owners, their response observer, and the machine
API constructors that must carry the generation-scoped config. The exact
fail-before command was:

```text
timeout 180 cargo test -p nimbus-sandbox \
  backends::oci::network::forwarding::tests::generic_http_success_is_not_machine_withdrawal_evidence \
  --lib -- --exact --nocapture
```

It exited `101` because the pre-correction implementation returned `Ok(())`
from a generic `HTTP/1.0 200 OK` with an empty body. No other sandbox lifecycle
path was editable in that slice. A5 now admits only the container reload
composition method and its manifest-durability test owner until a deterministic
post-acknowledgement/pre-publication fault captures the current loss of desired
state. The exact command is:

```text
timeout 240 cargo test -p nimbus-sandbox \
  backends::container::runtime::tests::manifest_durability::reload_acknowledgement_before_completion_persistence_retains_durable_desired_intent \
  --lib -- --exact --nocapture
```

It exits `101`: the live PEP acknowledges the new policy, the injected
completion publication fails, and the canonical manifest still contains the
old deny-all desired policy. The completed A5 correction paths include the PEP
policy-state interface, its concept-owned worker reload child, the OCI egress
adapter and lifetime bridge, and the container manifest/composition plus
fresh-process proof owners. They add only durable desired/attempt generations,
exact provider observation, process-lifetime fencing, and idempotent
reconciliation of this crash window. Each later producer slice added only its
exact effect-owning adapters to this proof and recovery ledger. A6 adds only
the portable claim-lifetime module, the OCI reservation adapter and
manager, container/krun publication composition, and their behavioral proof
owners. It does not move manifest or provider interpretation below the
sandbox.
NNC3.9 deletion and NNC7.1a compiler-resolved census work remain excluded.

## Initial Structured Review Disposition

The one complete staged-item review ran with GPT-5.6 Sol, xhigh reasoning, and
fast service tier. Its three bounded bundle passes are one review invocation,
not three reviews. All nine findings were accepted after inspecting the exact
implementation and callers, and all nine are corrected on this candidate:

| Finding | Corrected disposition and proof |
| --- | --- |
| No-lifetime confirmed-stop rebind can clear a live lifetime fence. | Portable no-lifetime entry points now reject live lifetime evidence; production producer paths authenticate the exact lifetime guard. Live-owner and dead-owner tests cover process- and provider-managed batches. |
| A crash after creator spawn but before receipt publication strands `SpawnIntent`. | The descriptor-9 launch gate prevents creator execution until the exact durable spawn receipt is published; the owner persists quiescence before releasing or compensating, and a subprocess crash cut proves fresh-process convergence. |
| Runtime presence is accepted without the exact creator-attempt generation. | Creator runtime annotations carry the exact attempt identity, and container/krun recovery reject absent, stale, or substituted annotations. |
| Untagged PEP reload can erase durable reload-attempt identity. | The untagged production API is removed; every reload and completion path carries the exact desired/attempt generation and provider lifetime. |
| The test-only running-proxy iterator can drop lifetime guards before sockets. | The iterator and proxy-local copied claim were removed; the batch remains the sole lifetime authority for every running proxy in the fixture. |
| Machine-forwarder `/all` absence is unauthenticated. | The fallback is deleted. Only a typed receipt matching provider instance, generation, endpoint, protocol, and `Withdrawn`/`ExactAlreadyAbsent` authorizes release; generic status, text, EOF, refusal, or timeout is ambiguous. |
| Pre-receipt creator cancellation does not persist proven quiescence. | The cancellation path records exact durable quiescence before completing compensation, and restart tests authenticate it without respawn. |
| A fresh server process cannot re-adopt the exact inherited listener. | The server uses one stable semantic external-main listener identity and dead-owner reclaim capability; a fresh authority re-adopts the same surviving descriptor with a strictly newer lifetime generation. |
| Server reserve commits before bind-lifetime claim. | One atomic reserve-and-claim-with-lifetime operation now covers server, KV, and machine SSH producers; external bind effects occur only after the lifetime-backed claim is durable. |

The ninth finding exposed the same reserve/claim crash window in the KV and
machine-listener adapters; the first exposed legacy no-lifetime wrapper
siblings. Those are the same touched-owner bug classes, not adjacent roadmap
expansion. Finding five is test-only but is a small directly related ownership
cleanup. No finding changes the transport-free dependency invariant or moves a
provider effect into `nimbus-network`.

## Corrected-Candidate Review Disposition

The next GPT-5.6 Sol/xhigh/fast review inspected the frozen staged tree
`842ca4c2c6205ede7a6e710dac4cd1409f63c3c8`. Its three bounded review threads
were one review of the NNC3.8 candidate, not three reviews:

- `019fa7bb-5c0b-7280-9e92-1a7ccf3ca2a7`;
- `019fa7c3-b7db-7a21-b11e-42014320df39`; and
- `019fa7ce-55a7-7aa3-b4f9-864ff244bbb1`.

The review produced 15 findings. Fourteen were accepted and corrected. One was
rejected only after exact source and behavioral verification:

| Finding | Disposition and corrected proof |
| --- | --- |
| Generic release can clear a live lifetime. | Accepted. Generic `release` now rejects `Withdrawing` records with live lifetime evidence; exact release authenticates the lifetime. Live/exact/stale tests pass. |
| Confirmed-stop process-bound rebind is rejected by an over-narrow invariant. | Accepted. Cleanup-pending rebind accepts an exact bind claim plus active lifetime after confirmed descriptor death while preserving all other fences. |
| External listener crash between claim and binding publication strands authority. | Accepted. Dead-owner recovery adopts the exact claimed external descriptor before re-adoption; server fresh-authority tests cover the crash cut. |
| `RuntimeObserved` accepts container presence without creator-attempt identity. | Accepted. Runtime annotation must authenticate the exact creator attempt; absent, stale, and substituted annotations fail closed. |
| Krun restart cannot recover a dead Netavark claimed batch. | Accepted. Exact provider absence moves the dead claim batch to one rebindable reserved generation; ambiguity remains fenced. |
| Gvproxy request/response lacks stable provider incarnation identity. | Accepted. Every request and typed receipt carries provider instance plus generation; CLI derives a boot-scoped stable instance so API restarts are stable and VM/provider restarts cannot alias. |
| Krun manifest write failure drops the launch lifetime too early. | Accepted. Failed publication retains the exact lifetime through compensating cleanup and retry. |
| Nonempty `TerminalNoEffect` runner outcome can erase effect evidence. | Accepted. Terminal-no-effect is valid only when the effect set is empty. |
| Later runner exit can overwrite the first durable exit result. | Accepted. First durable exit wins and replay is byte-stable. |
| Krun terminal publication does not require shutdown intent. | Accepted. Terminal finality requires explicit shutdown intent; the natural-exit owner records it before terminal projection. |
| IPAM setup claim is accepted after deletion has begun. | Accepted. Setup claims are rejected in both `Deleting` and `DetachedProjectionPending`. |
| Runner result file is not an independent immutable crash anchor. | Accepted. A create-once result anchor is staged, synced, and exclusively hard-linked before decision publication; fresh-process recovery covers the anchor/decision crash window. |
| A4 proof still claims global `/all` absence authority. | Accepted. The stale proof claim is removed; only an exact typed receipt authorizes release. |
| The 110-path checkpoint omits correction-owned paths. | Accepted. The final staged list is regenerated mechanically from the complete candidate. |
| Container `RestartRetained` terminal cleanup is missing. | Rejected. The exact release already exists, and `restart_retained_machine_listener_releases_without_process_registry` passes `1/1`. |

Correction-focused evidence passes: portable lifetime `22/22`; external server
recovery `2/2`; KV listener `9/9` with two child-only ignores; runner handoff
`8/8`, recovery `11/11`, and reliability `25/25`; Krun/IPAM `6/6`;
forwarding `7/7` plus constructor/CLI provider-incarnation cases; and the seven
regressions first exposed by full sandbox replay pass `7/7`. The subsequent
full affected suites and quality gates are recorded above.

## Final Full-Candidate Review Disposition

The final full-candidate review inspected frozen staged tree
`1c4540df60aeaa32c7bb9a8ea2e9c2566bb955c8` with GPT-5.6 Sol, xhigh reasoning,
and fast service tier. Its three bounded bundle threads were one review
invocation, not three reviews:

- `019fa812-ecd9-7383-a0db-3647d276d7be`;
- `019fa819-1eb6-77d3-8f34-62d2aba1eb86`; and
- `019fa820-f0ae-7082-ae43-a7c865f43203`.

All five findings were accepted and corrected:

| Finding | Corrected disposition and proof |
| --- | --- |
| Provider-managed recovery required the result binding's complete opaque handle to equal the bind-attempt handle. | Portable recovery now authenticates the stable provider registration while allowing distinct attempt/result resource handles. A foreign registration still fails without mutation. The exact fail-before returned `InvalidTransition`; the corrected focused cases pass. |
| Provider rebind retained `confirmed_stopped_binding` after adopting a new live binding. | Successful adoption of the exact new binding clears only that obsolete stopped-binding evidence. The fail-before returned a corrupt-authority error; the complete lifetime owner now passes `23/23`. |
| A newly rebound external socket could substitute for a prior external provider incarnation at the same address. | `ExternalServerListenerContext` carries the exact opaque provider incarnation and resource generation. Only that context, persisted and replayed with the inherited descriptor, may reclaim; a rebound incarnation and a stale generation both fail without durable mutation. Listener lifecycle passes `14/14`. |
| The Netavark delete-response-loss fixture claimed provider absence while its namespace marker still existed, allowing a false-green A10 proof. | The crash child commits and syncs provider deletion before persisting exact absent evidence. Recovery cross-checks evidence against the real marker, invokes teardown once for present setup compensation and zero times for committed delete absence, and proves absence before IPAM release. The expected-red caught the contradiction; the fresh-process matrix and full sandbox suite pass. |
| The A1 fresh-process parent remained ignored and its child used lifetime-unaware adoption APIs. | Both completed parent proofs are enabled. The child retains its live guard and atomically adopts/activates through the lifetime-authenticated API. The forced pre-fix parent failed on stale/substituted lifetime evidence; the corrected process suite passes `6/6` with only two child entry points ignored. |

These corrections do not expand ownership: portable code still interprets
only provider registration and fenced lifecycle identity; server owns sockets
and external-provider context; sandbox owns Netavark effects and inspection;
the subprocess harness owns only behavioral proof. The exact correction gates
also pass affected all-target/all-feature check, workspace strict Clippy,
warning-denied rustdoc, the `67/67` authority plus `35/35` risk census, and the
live verifier at the expected `14/15` boundary solely because NNC3.9 still owns
NNCV005.

## Correction-Only Review Disposition

Exactly one bounded correction review ran against the 11-path delta from
frozen tree `1c4540df60aeaa32c7bb9a8ea2e9c2566bb955c8`. The actual reviewer was
GPT-5.6 Sol with xhigh reasoning and fast service tier; its one thread was
`019fa843-ca11-7672-bd73-6d3ef67da990`. The bundle was `73,890` bytes.

Result: zero findings; `patch is correct`; confidence `0.91`. The reviewer
explicitly confirmed all five corrections, found no actionable regression,
security issue, ownership violation, or acceptance gap, and preserved the
transport-free dependency boundary. This closes the material executable
review loop. Subsequent closeout edits are proof/ledger-only and do not trigger
another review.

## Next Proof

Stage the exact 114-path candidate, verify that the mechanical path list and
index match with no unstaged remainder, and commit NNC3.8. Then audit NNC3.9's
current production allocator/probe authority against the source-derived,
AST-assisted, compiler-resolved, and generated-code obligations before
deleting any old path. No NNC3.9 implementation path is admitted into this
checkpoint.
