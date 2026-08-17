# NNC5.2a — Durable Association And First-Effect Ordering

Status: `criteria 1-17 green; exact local item commit next`

Source checkpoint:

- commit: `94263586d4f53eb30d504f4f23f4d5ac1fb5bb10`
- tree: `501734a47b0ae6f9238035e879cb870c4f0dd101`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- source was clean when the item started
- original dirty checkout and clean `machine-os` companion: inspected only,
  unchanged

## Unit Of Value And Prospective Split

The original NNC5.2a combined two independently testable seams:

1. exact attachment association plus provider-attempt-before-effect ordering;
2. startup candidate discovery and the eight-row orphan classifier.

The three bounded read-only audits found that the first changes portable
attachment/allocator and live backend effect ordering, while the second changes
startup evidence collection, classification, and quarantine. Combining both
would reproduce the oversized review problem that the owner explicitly asked
the plan to avoid. Before any executable source mutation, the integration owner
therefore split the work prospectively:

- NNC5.2a is this exact association and first-effect ordering unit;
- NNC5.2b depends on NNC5.2a and owns filename-independent startup
  classification plus the eight-row matrix.

NNC8.3 still owns repeated provider cleanup, artifact removal, release, and
eventual convergence. NNC5.4 still owns real process-kill cuts. The split
weakens no original success criterion and creates two explicit
acceptance-bearing review/commit units rather than ad hoc review chunks.

No structured autoreview runs during audit, fail-before work, implementation,
cleanup, or acceptance convergence. Exactly one GPT-5.6 Sol/xhigh/fast review
runs only after this whole item is candidate-frozen and every written criterion
is green. One narrow correction review is permitted only if an accepted
finding materially changes executable code.

## Audit Result

All three read-only lanes completed without changing files. They agreed on the
defect:

- placement already reserves an unplaced attachment, selects IPAM, binds the
  exact segment, and adopts the claim before the common attach path;
- the portable allocator inspection collapses that association to a phase
  enum, so the attachment lifecycle cannot authenticate the selected segment
  and allocator epoch;
- the NNC5.2 attachment record stores tenant, selected provider, generic
  resource version/phase, and eventual stable provider handle, but not the
  reservation claim or exact segment;
- the sandbox IPAM record already stores claim, segment, addresses, and the
  Netavark provider-operation journal;
- setup currently persists the Netavark attempt only inside provider setup,
  after the legacy bridge purge, namespace creation, and listener claims;
- Container machine-forwarded restart/delete paths can bypass the common
  durable attachment lifecycle and therefore must be brought under the same
  ordering contract.

The defect is missing authenticated evidence and ordering, not a need for a
second manager, repository, provider abstraction, or cross-partition
transaction.

## Current And Target Order

Current create order:

```text
segment unplaced reservation
-> IPAM selection
-> exact segment bind
-> allocator adoption
-> generic attachment Provisioning record
-> provider inspection
-> legacy nimbus0 purge effect
-> namespace creation
-> listener claims
-> persist Netavark Provisioning attempt
-> Netavark setup
-> stable attachment handle / publication
```

Target create order:

```text
segment unplaced reservation
-> IPAM selection
-> exact segment bind
-> allocator adoption
-> read-only exact association inspection
-> portable attachment reserve with immutable exact association
-> attachment/allocator/IPAM cross-authentication
-> inspect existing sandbox provider attempt/evidence
-> persist exact sandbox provider attempt
-> namespace creation
-> listener claims
-> Netavark setup
-> stable attachment handle / publication
```

There is no cross-partition transaction. A crash before the portable attachment
reserve has created no external effect and leaves claim-fenced allocation/IPAM
evidence for compensation. A crash after provider-attempt persistence has an
exact attempt to inspect and fence. Unknown outcomes never authorize a second
effect.

The pre-launch legacy `nimbus0` migration runs an unowned effect in the middle
of attachment provisioning. Delete that compatibility behavior and its marker
rather than inventing a durable attachment claim for obsolete state.

## Frozen Owner Decisions

### One portable association authority

Add one immutable provider-neutral value:

```rust
NetworkAttachmentSegmentAssociation {
    reservation_claim: NetworkReservationClaim,
    segment_id: NetworkSegmentId,
    lease_epoch: NetworkLeaseEpoch,
}
```

`DurableNetworkAttachmentState` stores this value. It remains desired durable
attachment evidence, not observed provider state.

`LocalNetworkAttachmentAuthority::reserve` accepts the association and derives
the resource lease epoch from it. Exact replay requires equality. Claim,
segment, or epoch substitution fails without changing authority bytes.

`NetworkSegmentAllocator::inspect_attachment_reservation` is deepened to return
a read-only observation containing the reservation state and the exact
association whenever the durable state is bound, adopted, or cleanup-pending.
Unplaced or absent states carry no fictional association. Single-node,
configured, cluster-delegating, and recording test implementations obey the
same contract.

The shared network state format advances from version 1 to version 2. Old
attachment records are rejected explicitly; there is no serde default,
migration shim, or compatibility path because Nimbus has not launched.

### One sandbox provider-attempt authority

The existing sandbox IPAM `NetavarkProviderOperation` journal remains the sole
authority for transient setup/delete attempts. Do not copy that attempt into
`DurableNetworkAttachmentState` or reinterpret the portable stable
`NetworkProviderHandle` as an effect attempt.

Split provider setup into concept-owned prepare/execute/complete operations:

- prepare persists one attempt-unique setup claim against the exact
  attachment, reservation claim, segment, and generation;
- execute accepts only that prepared claim and cannot mint or substitute one;
- complete authenticates the same claim before Ready projection.

Prepare occurs before namespace creation, listener claims, Netavark, or
machine-forwarding effects. Existing exact pending attempts are inspected and
adopted; ambiguous attempts fence retry. Delete already persists its attempt
before Netavark teardown and must retain that proof through every actual
backend route.

### Effect locality and composition

`nimbus-network -> nimbus-core` remains the only initial workspace edge.
Portable code performs no filesystem, namespace, socket, command, Netavark,
machine, proxy, or cleanup effect.

Both Container and Krun use the same sandbox-owned attachment lifecycle.
Container's machine-forwarded restart and teardown routes must delegate
attachment transitions to that lifecycle rather than manually releasing
Netavark/netns/IPAM/segment state around it. Machine publication and provider
receipts remain Container-owned adapter effects.

Near-threshold composition roots receive only delegation/extraction edits:

- move attachment authentication into
  `attachment_lifecycle/authority.rs`;
- move IPAM attempt preparation/execution state transitions into
  `ipam/provider_operation.rs`;
- move exact allocator inspection into `segment/inspection.rs`;
- keep coherent tests in concept-owned children.

Do not grow `state_store.rs`, `ipam.rs`, `attachment_lifecycle.rs`, or sandbox
`segment.rs` past the repository thresholds.

## Explicit NNC5.2a Non-Goals

- Filename/manifest candidate discovery and the eight-row startup classifier:
  NNC5.2b.
- The sandbox-owned reversible artifact locator needed by that classifier:
  NNC5.2b. No `SandboxId`, IP address, or filesystem path enters
  `nimbus-network`.
- Provider cleanup execution, artifact removal, repeated reconciliation, and
  allocation release convergence: NNC8.3.
- Complete readiness evidence: NNC5.3.
- Real SIGKILL/subprocess crash cuts: NNC5.4.
- Side-effect-free workload inspection/restart policy: NNC5.6.
- Workload/network saga coordination: NNC6.
- Netavark, nftables, gvproxy, proxy, service naming, cluster transport, or
  cloud-provider seams in `nimbus-network`.

## Fail-Before Evidence

Re-run against the exact source checkpoint:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect \
  -- --ignored --nocapture
```

Result: exit 101, 0 passed, 1 failed, 789 filtered. The current implementation
left `unowned-evidence-left-behind`; the required result is `fully-removed`.
This is NNC5.2a's canonical pre-existing crash-window red.

The companion eight-row orphan test also freshly exited 101 at 0/1 and reported
all eight rows mismatched. That result is preserved in the NNC5.2b ledger and
must not be claimed by this item.

Before semantic implementation, add current-API fail-before proofs that compile
against this checkpoint and demonstrate:

1. the serialized attachment record lacks exact claim/segment/epoch evidence;
2. claim, segment, or epoch substitution cannot be authenticated by the
   attachment lifecycle;
3. Container and Krun reach namespace/listener setup before a persisted
   Netavark attempt;
4. the machine-forwarded Container path can bypass the shared durable
   attachment transition.

Save the exact red command, exit status, counts, and assertion for each before
changing the behavior.

## Required Behavioral Proofs

Portable authority:

1. `reopen_preserves_exact_attachment_segment_association`;
2. `attachment_association_substitutions_preserve_authority_bytes`;
3. `missing_or_conflicting_attachment_association_is_rejected_on_reopen`;
4. `fresh_process_reopens_exact_attachment_segment_association`;
5. `allocator_observation_reports_exact_bound_segment_and_epoch_without_mutation`.

Actual backend ordering:

6. `container_first_effect_requires_persisted_association_and_provider_attempt`;
7. `krun_first_effect_requires_persisted_association_and_provider_attempt`;
8. `association_substitution_fails_before_inspection_or_effects_for_both_backends`;
9. `prepared_provider_attempt_is_reused_after_fresh_reopen_without_duplicate_effect`;
10. `container_machine_forwarding_cannot_bypass_attachment_attempt_authority`;
11. `container_and_krun_delete_effects_require_the_exact_persisted_attempt`.

Each association-substitution row covers claim, segment, and epoch. Each
rejection asserts zero namespace, listener, Netavark, machine, or cleanup
calls; unchanged allocator, IPAM, and attachment bytes; and one typed error.
The fresh-process test reopens from a clean OS process but does not claim
NNC5.4's real process-kill proof.

## Owned Implementation Paths

Portable primary paths:

- `crates/nimbus-network/src/attachment_state.rs`
- `crates/nimbus-network/src/attachment_state/tests.rs`
- `crates/nimbus-network/src/segment.rs`
- `crates/nimbus-network/src/lib.rs`
- `crates/nimbus-network/src/state_store.rs` for the format constant only
- `crates/nimbus-network/tests/attachment_authority_process.rs`

Sandbox primary paths:

- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/authority.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/state.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs`
- concept-owned attachment lifecycle tests
- `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/ipam/provider_operation.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/netavark.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/segment.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/segment/inspection.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/cluster.rs`
- configured/recording allocator delegates and tests
- actual Container/Krun network composition and Container machine-forwarded
  restart/cleanup call sites
- `scripts/verify-nimbus-network-control-plane.sh`,
  `scripts/verify-nimbus-network-attachment-ordering.mjs`, and the
  concept-owned focused mutation-test helper
- source-derived bind/composition census line locators changed by the owned
  concept extraction:
  `nnc0.1-bind-owner-inventory.json` and
  `nnc4.6f-production-network-authority-census.json`

Paths may be narrowed after call-graph inspection. Any newly required path must
be named in the recovery ledger before edit. Compute, tenant, services, system,
egress/proxy, cluster transport, generic workload state, and unrelated runtime
paths are forbidden.

## Acceptance Criteria

NNC5.2a cannot close until all criteria are green:

1. one immutable portable association durably binds tenant-qualified
   attachment, reservation claim, stable segment ID, and allocator lease epoch;
2. attachment plan ID, generation, digest, resource ID, epoch, selected
   provider, claim, and segment authenticate as one exact generation;
3. exact replay is idempotent while claim, segment, epoch, plan, generation,
   digest, attachment, tenant, or provider substitution fails byte-preserving;
4. allocator inspection reports exact association without mutation for every
   adapter and cannot fabricate association for absent/unplaced state;
5. a fresh process reopens exactly the same association and rejects
   checksum-valid missing/conflicting evidence;
6. the state format is bumped with no compatibility default or migration shim;
7. the sandbox IPAM journal is the only transient provider-attempt authority;
8. the exact setup attempt is durable before namespace, listener, Netavark,
   machine-forwarding, or cleanup effects can begin;
9. execute/complete accept only the prepared attempt; ambiguous or substituted
   attempts cause zero duplicate effects and remain retryable/fenced;
10. Container, Krun, and Container machine-forwarded restart/teardown routes
    use the same association/attempt ordering contract;
11. obsolete per-attachment legacy bridge purge behavior and its marker are
    deleted rather than granted new authority;
12. cleanup and error paths retain the exact durable evidence needed for later
    NNC5.2b/NNC8.3 classification and never release uncertain capacity;
13. no filename, manifest, IP address, provider handle, or filesystem path is
    promoted to portable attachment identity;
14. `nimbus-network -> nimbus-core` remains the only initial workspace edge
    and no provider effect or forbidden abstraction enters the crate;
15. touched production modules satisfy modularity thresholds through
    concept-owned extraction, not mechanical splitting;
16. focused fail-before/correction proofs, full `nimbus-network` and
    `nimbus-sandbox` suites, actual backend lanes, all-target check, strict
    Clippy, warning-denied rustdoc, format/diff, dependency/effect/static
    verifier, docs, and site gates pass with exact counts and skips;
17. exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast structured review is
    fully dispositioned, with one narrow correction review only if an accepted
    finding materially changes executable code; and
18. the proof, recovery header, checkpoint ledger, exact paths, executable
    digest, commands, counts, skips, remaining-red owners, and exact local item
    commit are recorded without push or PR.

## Current Dirty-State Checkpoint

The implementation is complete in the dedicated owner worktree and remains
uncommitted only for the candidate-freeze, one-item review, ledger closeout, and
exact item commit. The original dirty checkout still has exactly its four
pre-existing user-owned paths, and the clean `machine-os` companion remains
unchanged. No rebase, destructive recovery, push, or PR occurred.

The owned candidate includes the portable attachment/segment paths; shared OCI
attachment lifecycle, provider-attempt, allocator, Netavark, and test-support
paths; actual Container and Krun composition/recovery paths and their directly
affected tests; the aggregate verifier plus its concept-owned NNCV017 helper;
the two source-derived census locators changed by those extractions; canonical
plan/proof routing; and this proof. Exact staged paths and the executable digest
will be recorded after the final documentation gates pass.

## Candidate Implementation

The candidate now has three explicit, non-duplicated authorities:

- `DurableNetworkAttachmentState` owns the desired tenant-qualified attachment
  generation and its immutable reservation-claim/segment/epoch association;
- `NetworkSegmentAllocator` owns capacity, reports the exact read-only
  association for every adapter, and never turns a CIDR into identity; and
- the sandbox IPAM journal alone owns transient Netavark setup/delete attempts.

The shared attachment lifecycle authenticates the allocator association before
provider inspection, reserves the same association in the portable attachment
record, and persists a prepared IPAM attempt before namespace, listener,
Netavark, machine-forwarding, or cleanup callbacks. Execution changes that
prepared attempt to its final pre-effect fence before invoking the provider.
Fresh reopen adopts a prepared no-effect attempt, while an attempt that crossed
the fence is inspection-only and cannot be replayed.

Container host-managed, Container machine-forwarded, and Krun attach/teardown
routes all delegate to that lifecycle. The obsolete per-attachment `nimbus0`
purge and marker are deleted. Provider effects remain in `nimbus-sandbox`;
`nimbus-network` retains only `nimbus-core` as a workspace dependency.

## Behavioral And Quality Evidence

| Gate | Candidate result |
| --- | --- |
| Required portable proofs 1-5 | All named tests pass. Exact reopen, claim/segment/epoch substitution with byte preservation, checksum-valid missing/conflicting evidence, fresh-process reopen, and read-only allocator observation are covered. |
| Required backend proofs 6-11 | All six named tests pass. Container/Krun first effects, all association substitutions, prepared-attempt fresh reopen, machine-forwarded routing, and exact delete-attempt fencing are covered. |
| Shared lifecycle suite | Corrected candidate `41/41` passed, `759` filtered, including both final-detach crash-interval proofs. |
| Container provider-cleanup suite | Corrected candidate `30/30` passed, `770` filtered. |
| Netavark provider-attempt lane | `12` passed, `3` deliberately ignored subprocess entrypoints, `783` filtered. |
| Portable crate | `233/233` passed with `1` deliberately skipped subprocess entrypoint. |
| Actual Container/Krun backend lane | `332/332` passed, `475` filtered. |
| Full sandbox crate | Corrected candidate `785/785` passed with `24` declared skips. The pre-review candidate passed `783/783`; an earlier convergence run found two directly affected Krun test contracts at `781/783`, and their isolated correction lane passed `2/2` before that full green rerun. |
| All-target/all-feature compilation | `cargo check -p nimbus-network -p nimbus-sandbox --all-targets --all-features` passed. |
| Strict lint | `cargo clippy -p nimbus-network -p nimbus-sandbox --all-targets --all-features -- -D warnings` passed. Displayed warnings remain confined to the existing vendored Brotli crates. |
| Warning-denied rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-network -p nimbus-sandbox --no-deps --all-features` passed. |
| Static dependency/effect contract | Live aggregate verifier `18/18`, including NNCV017. Its adversarial self-test passed `67/67`, including missing-association, setup-fence, teardown-fence, machine-bypass, and legacy-purge mutations. |
| Script quality | Node syntax, shell syntax, and ShellCheck pass for the aggregate verifier and NNCV017 helpers. |
| Format and patch integrity | `cargo fmt --all --check` and `git diff --check` pass after the item-local cleanup. |
| Documentation | `scripts/check-docs.sh` passes `108` pages; `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

The first full sandbox compile caught an obsolete test-only Netavark teardown
wrapper exported in production configuration. It is now `#[cfg(test)]` at both
definition and export. The first executable full-suite run then exposed two
Krun expectations written for the old lifecycle:

1. ambiguous Netavark delete was expected to execute repeatedly; it now proves
   the durable attempt is inspected without a second provider call and
   converges only after exact absence is observed; and
2. natural-exit cleanup omitted the new read-only association authentication
   from its exact operation trace.

Both are direct NNC5.2a corrections. No production behavior was weakened to
make the suite green.

## Full Item Review And Accepted Correction

The one complete item review ran only after criteria 1-16 were green and the
61-path candidate was frozen at executable SHA-256
`22ddd3732d985af93e409a5d045373d87e4bb51d7599adb5e42fdfcd73972712`.
The actual reviewer was GPT-5.6 Sol at `xhigh` reasoning in fast mode. It
reviewed one integrated `429,453`-byte bundle under thread
`019fb3a3-d6c1-7d03-b54d-b98cf74b9637` and returned two findings:

1. **Accepted, P1, confidence 0.98.** Final detach released and finalized the
   allocator immediately before publishing the portable terminal phase.
   A crash or store failure in that bounded interval left the allocator
   `Absent` while the exact portable record remained `Reserved` or `Deleting`;
   detach authentication rejected both phases and permanently wedged recovery.
   A purpose-built fail-before regression reproduced the rejection at `0/1`
   with the diagnostic `lost allocator association while durable phase
   Deleting is non-terminal`. The correction admits only exact
   allocator-absent `Final` detach with the same portable
   claim/segment/epoch/config association; `Restart` remains fenced before
   provider inspection or cleanup callbacks. The corrected host-managed
   Container/Krun and Container machine-forwarded proofs pass `2/2`, reach
   `Released`, and execute zero namespace or Netavark effects.
2. **Rejected, confidence 0.92.** The review claimed the private
   machine-forwarded restart/finalization helpers could panic when the
   persisted forwarder was absent. Both public production dispatchers first
   branch on `machine_port_forwarder.is_none()` and route that shape through
   the host-managed implementation. The private helpers' `expect` calls are
   reachable only after the same dispatcher proved the provider present.
   `release_execution_artifacts` and `reset_runtime_for_restart` therefore
   provide concrete caller evidence that the alleged no-forwarder route does
   not exist.

The accepted finding materially changed executable code, so the cadence permits
exactly one narrow correction review after the affected proofs are green. No
second full item review is warranted.

The permitted narrow review ran as one integrated `445,782`-byte GPT-5.6
Sol/xhigh/fast pass under thread
`019fb3cf-b1da-78b2-b1a1-9dfb37dd4863`. It found one P1 continuation defect at
confidence `0.98`: a fallible host pre-detach callback or either
machine-forwarded callback moved the exact allocator-absent portable record to
`CleanupPending`, but the correction admitted only `Reserved`/`Deleting` on
the next `Final` retry. That recreated the same permanent wedge.

The finding is accepted. New fail-before coverage reproduced both affected
routes at `0/2`. The final correction admits the exact `CleanupPending`
association only for allocator-absent `Final` recovery; `Restart` remains
rejected before provider inspection or callbacks. Host recovery now survives a
failed cleanup callback, and machine recovery survives failures both before
and after provider detach; the corrected proof passes `2/2` and executes zero
namespace or Netavark effects. The full lifecycle passes `41/41`, and the full
sandbox crate passes `785/785` with `24` declared skips after the correction.

The cadence permits one narrow correction review, not an unbounded review
loop. The accepted narrow finding is therefore closed by fail-before,
corrected behavioral proofs, full affected gates, static verification, and
manual inspection. No third NNC5.2a structured review ran or is warranted.

## Modularity And Seam Check

- `attachment_lifecycle.rs` is `1,435` lines and its concept-owned tests root is
  `1,459` lines after test adapters and real-adapter proofs moved to children.
- the correction proof remains concept-owned in `tests/effect_order.rs`, now
  `938` lines;
- `ipam.rs` is `939` lines after provider-operation state and tests moved to
  concept-owned children.
- sandbox `segment.rs` remains `1,419` lines.
- the aggregate verifier is `1,499` lines after NNCV017 moved to a dedicated
  Node checker and shell contract helper.
- `state_store.rs` remains the existing `1,960`-line deep-module exception;
  this item changes only the format constant and its pinned test expectation,
  with no line growth.

No attachment lifecycle state was copied into Container/Krun manifests. No
provider attempt was copied into `nimbus-network`. No socket, namespace,
Netavark, nftables, gvproxy, machine, policy, service-name, proxy, server,
system, compute, tenant, or cluster-transport authority entered the portable
crate. NNC5.2b still solely owns startup evidence enumeration and the eight-row
orphan classifier; NNC8.3 still owns repeated provider cleanup and convergence.

## Final Candidate And Commit Gate

Acceptance criteria 1-17 are green. The sole remaining action is the exact
local item commit, without push or PR.

The candidate freeze contains exactly `61` staged paths, has no unstaged
changes, and passes `git diff --cached --check`. Its executable subset beneath
`crates/` and `scripts/` has SHA-256
`22ddd3732d985af93e409a5d045373d87e4bb51d7599adb5e42fdfcd73972712`.
That is the immutable pre-correction review identity. The corrected executable
SHA-256 is
`359592d2b6bd1614c0c0903bab1b2e216c410d2b7901bcc15597e5517ae41509`
and is the identity reviewed by the narrow pass. The accepted narrow finding
produced the final executable SHA-256
`f0c8e7078b55e457776ae7d7cd83cca6f213ce3d8d56e965f1b3a741c90eaf85`.
The exact path set is:

```text
crates/nimbus-network/src/attachment_state.rs
crates/nimbus-network/src/attachment_state/tests.rs
crates/nimbus-network/src/lib.rs
crates/nimbus-network/src/segment.rs
crates/nimbus-network/src/state_store.rs
crates/nimbus-network/tests/attachment_authority_process.rs
crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs
crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs
crates/nimbus-sandbox/src/backends/container/runtime/restart.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/attachment_authority.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/netavark_restart.rs
crates/nimbus-sandbox/src/backends/krun/vm/attachment_recovery.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/attachment_authority.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/attachment_recovery.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/explicit_stop.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/launch_compensation.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/natural_exit.rs
crates/nimbus-sandbox/src/backends/oci/network.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/authority.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/machine_forwarded.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/recovery.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/state.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/test_api.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/authority.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/durable_recovery.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/effect_order.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/real_adapters.rs
crates/nimbus-sandbox/src/backends/oci/network/cluster.rs
crates/nimbus-sandbox/src/backends/oci/network/dto.rs
crates/nimbus-sandbox/src/backends/oci/network/finality.rs
crates/nimbus-sandbox/src/backends/oci/network/finality/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/ipam.rs
crates/nimbus-sandbox/src/backends/oci/network/ipam/provider_operation.rs
crates/nimbus-sandbox/src/backends/oci/network/ipam/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/layout.rs
crates/nimbus-sandbox/src/backends/oci/network/netavark.rs
crates/nimbus-sandbox/src/backends/oci/network/netavark/recovery_tests.rs
crates/nimbus-sandbox/src/backends/oci/network/netavark/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/reaper.rs
crates/nimbus-sandbox/src/backends/oci/network/segment.rs
crates/nimbus-sandbox/src/backends/oci/network/segment/cleanup.rs
crates/nimbus-sandbox/src/backends/oci/network/segment/reservation.rs
crates/nimbus-sandbox/src/backends/oci/network/test_support.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/proof/nimbus-network-control-plane/nnc0.7-orphan-listener-baselines.md
docs/private/plans/proof/nimbus-network-control-plane/nnc2.5-two-phase-detach-release-quarantine.md
docs/private/plans/proof/nimbus-network-control-plane/nnc2.7-multi-tenant-invariants.md
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
docs/private/plans/proof/nimbus-network-control-plane/nnc5.1-sandbox-attachment-lifecycle.md
docs/private/plans/proof/nimbus-network-control-plane/nnc5.2-durable-attachment-state.md
docs/private/plans/proof/nimbus-network-control-plane/nnc5.2a-durable-association-effect-ordering.md
scripts/nimbus-network-control-plane/attachment-ordering-contract.sh
scripts/verify-nimbus-network-attachment-ordering.mjs
scripts/verify-nimbus-network-control-plane.sh
```
