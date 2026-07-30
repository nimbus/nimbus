# NNC5.1 — Sandbox Attachment Lifecycle

Status: `complete — exact local item checkpoint`

Source checkpoint:

- commit: `14789e07aafd38740a44469d83353d2d11b44a82`
- tree: `4354ed04046388f8c94802a82a7b4b68f4c7f3c4`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- original dirty checkout and clean `machine-os` companion: inspected only,
  unchanged

## Owner Boundary

NNC5.1 owns the common host-managed OCI attachment ordering and compensation
currently duplicated by the container and krun backends. The target is one
crate-private deep module in `nimbus-sandbox`; no provider effect, new workspace
edge, or public provider abstraction enters `nimbus-network`.

The lifecycle may compose the already-earned sandbox seams:

- `NetworkSegmentAllocator` / `OciSegmentAllocator`;
- `place_sandbox_on_block` and `ReservedNetworkLaunchAuthority`;
- `OciIpamAuthority`;
- `OciPortLeaseCoordinator` and `NetavarkPortLifetimeRegistry`;
- concrete netns, Netavark, egress-pin, and machine-forwarding adapters;
- `TerminalNetworkAuthoritySet` after convergence.

It does not own:

- workload status, desired intent, restart policy, or compute saga decisions;
- container runner handoff or krun creator/runtime-absence discovery;
- tenant egress policy or PEP forwarding implementation;
- logical service names, sockets, server listeners, or projections;
- cluster transport, membership, node identity, or routing;
- durable shared attachment phase/generation/provider handle, which is NNC5.2;
- complete readiness evidence, NNC5.3;
- crash-after-every-effect convergence, NNC5.4;
- side-effect-free inspection, NNC5.6.

Machine-forwarded container publication remains a concrete adapter. Krun must
not silently claim that provider mode. Restart and final teardown are explicit
dispositions: restart retains the same fenced generation, IPAM, segment, and
rebind authority; final teardown releases only after authenticated provider and
namespace absence.

## Routing Reconciliation

The active network plan is the sole NNC5.1 implementation owner.
`architecture-review-2026-07-plan.md` cedes slices owned by another active
plan. The plan index describes `nimbus-sandbox-plan.md` as proposed, but its
body is absent from this HEAD; NNC5.1 will not create or fork that missing
owner. Horizontal scaling retains future cluster transport/super-net lease
source ownership. The egress/proxy, machine, service, system, and compute owners
retain their existing boundaries.

The owner branch is intentionally 54 campaign commits ahead of and 36
post-execution-base commits behind `origin/main`. Current main does not contain
this unpushed campaign and represents unrelated later work; rebasing now would
replay or discard the completed campaign rather than simply update its base.
The continuation directive explicitly requires preserving this worktree,
ledger, and completed commits. No rebase, push, or PR is authorized.

## Read-Only Audit

Three bounded, non-overlapping audits independently inspected the container
path, krun path, and cross-backend contract. All reported zero changed paths.
Owner inspection reconciled their call graphs, semantic differences, source
census, modularity pressure, and existing tests before this checkpoint.

Current production call paths are:

```text
container plan
  -> reserve attachment/IPAM/ports
  -> persist execution fence
  -> adopt attachment
  -> caller-local configure_network
  -> PEP/readiness/runtime

krun plan
  -> persist claim
  -> reserve attachment/IPAM/ports
  -> persist Adopting
  -> adopt attachment
  -> persist Adopted
  -> caller-local configure_network
  -> PEP/readiness/runtime
```

Both setup switchboards independently encode:

```text
authenticate generation
  -> purge legacy bridge
  -> authenticate port leases
  -> create netns
  -> claim Netavark bind lifetimes
  -> run Netavark/IPAM setup
  -> activate bind lifetimes
  -> pin egress
  -> retain live lifetimes
  -> confirm held segment
```

Both terminal switchboards independently encode:

```text
authenticate cleanup authority
  -> classify listener/PEP authority
  -> quarantine held segment
  -> prove runtime/creator absence in backend owner
  -> stop PEP/publication
  -> detach Netavark
  -> remove netns
  -> complete/recover listener lifetimes
  -> release IPAM
  -> release/reap/finalize segment
  -> permit terminal publication
```

Container additionally owns machine-forwarder publication and runner handoff.
Krun additionally owns explicit `Reserved -> Adopting -> Adopted ->
ProviderOwned` manifest phases and resumable provider-failure checkpoints.
Those are real adapter differences, not reasons to duplicate common attachment
ordering.

## Effective Current Phases

| Phase | Current evidence | Failure or retry rule |
| --- | --- | --- |
| claim reserved | manifest claim plus reserved ports | reverse ports, IPAM, attachment independently |
| placed | exact segment association and IPAM reservation | claim-fenced cancellation |
| adopting/adopted | allocator reservation becomes held | inspect exact claim before choosing reserved or provider cleanup |
| netns created | persistent namespace path only | not readiness; remove only when provider absence is exact |
| provisioning | IPAM attempt plus Netavark operation | exact-attempt compensation; ambiguity stays deleting |
| provider ready | Netavark status plus active bind lifetime | pin/forwarding failures enter exact detach compensation |
| attachment active | held segment, IPAM, provider evidence, publication lifetime | runtime may activate only through the backend's existing gate |
| restart detach | runtime absence authenticated | retain generation/IPAM/segment; prepare bindings for rebind |
| terminal deleting | segment quarantined and authorities withdrawing | safe leak on ambiguity; no reuse |
| projection pending | provider absent, status deletion incomplete | retry projection removal without recreating provider |
| provider detached | provider/netns absence authenticated | release bindings, IPAM, then segment |
| terminal | all attachment authority absent | terminal finality proof may pass |

This is behavioral vocabulary only. NNC5.1 must not claim the durable
cross-backend phase/provider representation owned by NNC5.2.

## Expected-Red Evidence

At the source checkpoint:

```text
config_from_segment owners: 2
configure_network owners: 2
failed_netavark_configuration owners: 2
caller-local teardown files: 4
shared attachment module files: 0
shared lifecycle contract references: 0
container setup switchboard: 177 lines
krun setup plus inline compensation: 199 lines
container terminal cleanup switchboard: 353 lines
krun terminal/restart cleanup switchboard: 264 lines
container restart cleanup switchboard: 168 lines
```

The exact production owners are:

- container setup:
  `crates/nimbus-sandbox/src/backends/container/runtime.rs`;
- container setup compensation:
  `crates/nimbus-sandbox/src/backends/container/runtime/network_launch.rs`;
- container terminal cleanup:
  `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`;
- container restart detach:
  `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`;
- krun config/reserved cleanup:
  `crates/nimbus-sandbox/src/backends/krun/vm.rs`;
- krun setup, compensation, terminal, and restart detach:
  `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`.

The source census command is:

```bash
rg -n \
  'fn (config_from_segment|configure_network|failed_netavark_configuration)|teardown_container_network\(' \
  crates/nimbus-sandbox/src/backends/container \
  crates/nimbus-sandbox/src/backends/krun \
  -g '*.rs'
```

`cargo nextest list -p nimbus-sandbox --lib` exits zero and enumerates 727
tests. Existing container and krun tests separately cover lease authentication
before effects, reservation/adoption, setup compensation, cleanup ambiguity,
restart-retained bindings, terminal quarantine/release/finality, and stale
generation fencing. There is no suite applying one attachment contract to both
real adapters, so those green tests cannot falsify divergent lifecycle
interpretations.

## Frozen Target Seam

The target owner is:

```text
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/authority.rs
```

The production surface stays crate-private and deliberately small:

- immutable exact attachment input/context;
- explicit host-managed versus machine-forwarded publication mode;
- ordered attach under an already-authenticated claim/generation;
- exact setup compensation that preserves the primary error;
- explicit restart-retained versus terminal-release detach disposition;
- typed completed versus cleanup-pending result;
- narrow backend callbacks only for genuinely different runtime absence,
  machine publication, and manifest checkpoint behavior.

`SandboxBackend` is not expanded. No god `NetworkProvider`, compatibility shim,
or speculative public provider trait is introduced. The deletion proof is
semantic: removing the lifecycle owner would force both backends to recreate
the multi-step ordering/compensation protocol, not merely a call to one shallow
helper.

## Frozen Shared Contract

One concept-owned suite must run unchanged against the real container and krun
adapters and prove:

1. PlanOnly performs zero attachment effects.
2. The durable claim precedes attachment, IPAM, and port reservation.
3. Generation and exact lease authentication precede the first provider or
   filesystem effect.
4. Happy attach follows the single canonical phase trace.
5. Deterministic failure at every represented attach boundary compensates only
   completed phases in reverse order.
6. The primary failure survives every secondary cleanup diagnostic.
7. Failed/ambiguous compensation retains the exact claim, generation,
   namespace, and lifetime authority required for retry.
8. Retry is idempotent and produces one live desired attachment.
9. Authenticated runtime absence is required before detach; unknown or live
   evidence mutates no provider/lease/segment authority.
10. Restart detach prepares bindings for rebind and retains IPAM, segment, PEP
    assignment, and generation authority.
11. Final detach releases publication, IPAM, attachment, and segment only after
    provider and persistent-netns absence.
12. Stale tenant, attachment, claim, generation, or root provenance fails
    before effects.
13. Container machine-forwarded mode uses its explicit capability; krun
    implements only host-managed attachment and cannot construct the
    machine-forwarded adapter.
14. Repeated successful cleanup is idempotent.
15. The actual container and krun backend types execute this identical matrix
    through the same type-bound production routes used by their manifest
    callers.

Backend-specific tests remain valid for runner/creator/runtime behavior and
provider details. Equivalent attachment-order tests may move intact into the
shared concept owner; they must not be copied into a third suite.

## Complexity And Cleanup

The audit measured:

- container `runtime.rs`: 1,611 lines;
- krun `lifecycle.rs`: 1,896 lines;
- container `launch_cleanup.rs`: 1,981 lines;
- container test `lifecycle.rs`: 2,078 lines;
- krun `tests.rs`: 1,993 lines;
- OCI `ipam.rs`: 1,955 lines;
- OCI `port_lifecycle.rs`: 1,975 lines.

No NNC5.1 tests may grow the oversized caller test roots. The new contract
belongs in a concept-owned child. Production caller-local switchboards are
deleted rather than wrapped. Existing test groups move only along concept
ownership.

The corrected composition lands below the repository threshold:
`attachment_lifecycle.rs` is `1,449` lines, the contract composition root is
`1,443`, its exact-authority child is `270`, container `runtime.rs` is `1,484`,
and krun `lifecycle.rs` is `1,469`. The raw lifecycle context and host-effect
port remain private to the deep owner. Its private adapter constructor is
reachable only through small host-managed and container-only machine-forwarded
capability traits implemented by the actual backend types.

One directly related behavioral inconsistency is accepted into NNC5.1:
krun's unstarted compensation currently skips safe network release when launch
artifact cleanup fails, while container attempts independent cleanup steps and
aggregates errors. The shared reverse-compensation contract must attempt every
independent safe step and retain only the authority whose cleanup failed.

## Owned Paths

Frozen production/docs ownership:

- `docs/private/plans/nimbus-network-control-plane-plan.md`;
- `docs/private/plans/README.md`;
- this proof;
- `crates/nimbus-sandbox/src/backends/oci/network.rs`;
- new
  `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`;
- new concept-owned children beneath
  `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/`;
- `crates/nimbus-sandbox/src/backends/oci/port_lifecycle/batch_state.rs`,
  for the terminal-replay classification defect exposed when shared contract
  row 14 was deepened to retain a real listener generation and the accepted
  review correction's read-only, provider-specific authentication of one
  restart-retained auxiliary PEP listener, including its exact tenant,
  sandbox, address, and provider binding;
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json`,
  limited to four source-line anchor corrections for unchanged krun
  constructor/reconstruction occurrences shifted by deletion of the local
  lifecycle switchboard; occurrence identity, classification, realm, evidence,
  ordinal, and census cardinality remain frozen;
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/network_launch.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs` and
  `runtime/planning.rs`, limited to passing the new explicit fresh-launch
  authority into existing focused fixtures;
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`;
- `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`;
- `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs`, limited to adapting
  the existing missing-network-config fixture to the explicit fresh-launch
  authority;
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/launch_compensation.rs`,
  solely for the accepted regression proving that a failed independent launch
  artifact removal cannot suppress safe never-bound network release.

Existing backend-specific test paths may be changed only to move an intact
attachment contract case or install a thin real-adapter fixture. Any newly
discovered source path must be read, justified here, and added to the recovery
row before editing.

## In-Progress Behavioral Evidence

The initial shared matrix passed 30/30 while rows 9-11 and 14 used empty
published-listener sets. Owner inspection rejected that as insufficient proof
of the written host-port lifecycle claims and deepened those rows to attach one
real durable listener generation before detach.

The deepened matrix is intentionally expected red before the classifier
correction:

```text
30 tests run: 28 passed, 2 failed, 752 skipped
failed:
  container_14_repeated_cleanup_is_idempotent
  krun_14_repeated_cleanup_is_idempotent
```

Both profiles fail for the same shared reason: after exact final detach leaves
the listener in `Released`, terminal replay still carries the historical launch
claim in its attachment context. `classify_netavark_cleanup_batch` routes that
valid terminal record into live-provider recovery before reaching its existing
exact `Released` classification, then rejects the record because no live
provider lifetime remains. NNC5.1 owns the narrow shared classification fix;
the expected result is terminal no-effect with no recreated authority.

The first aggregate verifier run after the duplicated krun code was deleted is
also an expected mechanical red:

```text
16 passed, 1 failed
FAIL NNCV015 local-network-composition-census
```

NNCV015 reports only four stale `vm.rs` line anchors: `294 -> 290`, `296 ->
292`, `317 -> 313`, and `322 -> 318`. The source occurrence keys, symbols,
ordinals, classifications, realms, and total census remain unchanged. NNC5.1
updates only those four anchors and must rerun the live verifier plus its
adversarial self-tests. The final backend-type capability implementation adds
five lines above the same four occurrences, so the final anchors are `295`,
`297`, `318`, and `323`; identity, classification, ordinal, and cardinality
remain unchanged.

## Candidate Evidence

The corrected implementation and behavioral contract now establish:

| Proof | Result |
| --- | --- |
| Identical two-profile attachment contract | `30/30` passed, `752` skipped. Rows 9-11 and 14 carry one real Active listener generation and prove ambiguity retention, restart rebind, terminal release, and replay idempotency. |
| Shared port-lifecycle authority suite | `47/47` passed, `735` skipped. |
| Container plus krun affected lifecycle lane | `328/328` passed, `454` skipped. |
| Direct krun independent-compensation regression | `1/1` passed. |
| Full sandbox library suite | `758/758` passed, `24` declared skips. One initial all-green run reported one unnamed leaky process; an immediate leak-reporting rerun passed the same `758/758`, `24` skipped, with no leak. |
| All-target/all-feature check | `cargo check -p nimbus-sandbox --all-targets --all-features` passed. |
| Strict lint | `cargo clippy -p nimbus-sandbox --all-targets --all-features -- -D warnings` passed; displayed warnings are confined to the existing vendored Brotli crates. |
| Warning-denied rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-sandbox --no-deps --all-features` passed. |
| Network dependency/effect boundary | Cargo metadata reports exactly `nimbus-core`; live NNCV004, NNCV007, NNCV012, and the complete aggregate verifier pass. |
| Aggregate network verifier | Corrected live result `17/17`; adversarial self-test `62/62`. The fail-before result was `16/17` solely on four mechanically stale NNCV015 line anchors. |
| Format and patch integrity | `cargo fmt --all --check` and `git diff --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` pages; `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

The post-review correction candidate additionally proves:

- `AttachmentAttachAuthority::{FreshLaunch, RestartRetained}` is chosen by the
  already-authenticated caller branch rather than inferred from an optional
  manifest field;
- the exact adopted attachment generation, fresh launch claim, published
  listener batch, and auxiliary PEP tenant/sandbox/address/provider identity
  are authenticated before legacy purge, netns creation, or provider effects;
- the raw `OciAttachmentContext` is private and constructed in exactly one
  location; its adapter constructor is private to the lifecycle owner;
- reservation and attachment routes are type-bound to the real
  `ContainerSandboxBackend` and `KrunSandboxBackend` implementations, every
  contract row uses those routes, source-text assertions and `include_str!`
  proofs are absent, and container plus krun still pass `30/30`;
- a missing exact registered lifetime preserves the original primary
  diagnostic, records the recovery failure, and leaves provider, namespace,
  port, IPAM, and segment authority fenced for reconciliation.

The static ownership census now finds:

```text
common config owner:
  backends/oci/network/attachment_lifecycle.rs
common reservation/compensation/release owners:
  backends/oci/network/attachment_lifecycle.rs
common host-managed provider teardown:
  backends/oci/network/attachment_lifecycle.rs
backend configure_network functions:
  one thin container adapter
  one thin krun adapter
backend-local direct provider teardown:
  container machine-forwarded final cleanup
  container machine-forwarded restart cleanup
```

`krun::failed_netavark_configuration` remains test-only and routes its injected
fixture through the common compensator. No krun production path retains direct
Netavark/netns/IPAM/segment teardown. The container direct calls are confined to
the deliberately separate machine-forwarded provider adapter.

Later ownership remains honestly red:

- NNC5.2/NNC5.2a still lack one durable cross-backend phase/provider-attempt
  record and the complete orphan matrix;
- NNC5.3 still lacks the complete cross-provider readiness record;
- NNC5.4 still lacks named crash cuts after every effect;
- NNC5.5 remains the later band-wide locality proof, although the current
  NNCV012 effect boundary is green;
- NNC5.6 remains red because both `inspect` paths still call
  `maybe_restart_after_exit`, which can call `launch_manifest`.

No NNC5.1 code or proof claims those later results.

## Acceptance And Proof Gates

NNC5.1 is complete only when:

1. there is one common attachment ordering and one reverse-compensation owner;
2. caller-local common setup/compensation/teardown switchboards are absent;
3. container and krun production paths call the same deep lifecycle;
4. both real adapters run the identical fifteen-row shared contract;
5. restart-retained and terminal-release semantics are explicit and tested;
6. ambiguous absence retains exact fenced authority and prevents reuse;
7. machine-forwarded container behavior remains explicit and krun cannot claim
   it;
8. workload status, runtime absence, runner handoff, and inspection policy stay
   backend-owned;
9. NNC5.2-NNC5.6 expected-red tests/ownership remain honest and unchanged;
10. `nimbus-network` retains exactly the `nimbus-core` workspace edge and no
    provider effect imports;
11. the affected sandbox suite, static ownership scan, check, strict Clippy,
    rustdoc, format/diff, aggregate network verifier, and docs gates pass with
    exact counts;
12. one complete GPT-5.6 Sol/xhigh/fast structured review runs only after all
    prior criteria are green; every finding is dispositioned, with one narrow
    correction review only if accepted executable findings materially change
    code;
13. code, proof, recovery row, and plan-index truth-up land in one exact local
    item commit; no push or PR occurs.

The one complete item review ran after criteria 1-11 were green. The structured
helper assembled a GPT-5.6 Sol/xhigh/fast bundle but the external reviewer
service rejected it at its usage limit before returning content; that failed
transport attempt is not a verdict. A native read-only GPT-5.6 Sol/xhigh item
review then inspected the complete candidate and returned four actionable
findings:

1. **Accepted P1:** stale or missing launch provenance could reach provider and
   filesystem effects. The first correction added explicit fresh/restart
   authority plus adopted-segment, claim, and port authentication; the narrow
   review below found its auxiliary-listener authentication incomplete.
2. **Accepted P1:** the claimed two-backend contract used synthetic profiles
   and source-string assertions rather than the real backend adapter seam. The
   first correction removed source-text assertions and introduced concrete
   adapters; the narrow review below found the matrix still bypassed the actual
   backend-type routes.
3. **Accepted P2:** loss of the registered Netavark lifetime could replace the
   primary failure and skip an honest retained-fence diagnostic. The correction
   composes both diagnostics and performs no unauthenticated compensation.
4. **Accepted P2:** the recovery header and item row still described a
   pre-freeze verifier state. This proof and both routing ledgers now report the
   actual post-review state.

The combined F1/F3 fail-before run was `26/30`: exactly the two stale/missing
claim profiles and two missing-lifetime recovery profiles failed. After the
corrections, the identical contract passes `30/30`; port lifecycle passes
`47/47` with two declared ignores; the affected lane passes `328/328`; full
sandbox passes `758/758` with `24` declared skips; all-target/all-feature check,
strict Clippy, warning-denied rustdoc, exact core-only dependency, sealed
adapter scans, aggregate verifier `17/17`, adversarial verifier `62/62`,
format, and diff checks pass.

The one permitted narrow GPT-5.6 Sol/xhigh correction review then found two
accepted P1 gaps in those corrections:

1. the auxiliary request was authenticated only by coordinator/provider shape,
   not by its exact tenant, sandbox, persisted address, and selected port;
2. the matrix constructed adapter profiles directly and retained one
   test-only illegal publication constructor, so it could not prove the actual
   backend-type route.

Both findings reproduced. Removing fresh exact PEP identity made the two row-12
profiles fail `0/2`; separately weakening restart evidence back to provider ID
made the same profiles fail `0/2` at the retained-listener case. The correction
adds one neutral auxiliary-listener assignment, exact logical-listener and
confirmed-stop binding authentication before effects, private adapter
construction, host-managed capability implementations on both real backend
types, and a machine-forwarded capability implemented only by container.
Reservation and all fifteen matrix rows now use those backend-type production
routes; the illegal test-only constructor is deleted.

After correction: row 12 passes `2/2`; the full contract passes `30/30`; port
lifecycle passes `47/47`; the affected lane passes `328/328`; full sandbox
passes `758/758` with `24` declared skips; all-target/all-feature check, strict
Clippy, warning-denied rustdoc, format, and diff checks pass. The authority
tests moved intact to a `270`-line concept child, keeping every NNC5.1 module
below `1,500` lines.

Per the canonical cadence, this was the sole narrow correction review. Its
material findings were reproduced, corrected, and owner-verified; no third
review ran or is warranted. Criteria 1-12 are green, and this exact local item
checkpoint satisfies criterion 13 without a push or PR.
