# NNC5.2 — Durable Attachment State

Status: `complete — exact local item commit pending`

Source checkpoint:

- commit: `3cf209513c103f581245ca963a082517e7f031e6`
- tree: `9e4c415d6816bc716b67c13ac8516ecf4dd235c1`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- original dirty checkout and clean `machine-os` companion: inspected only,
  unchanged

## Unit of Value

NNC5.2 is one review and commit unit. It persists the portable attachment
resource version, phase, selected provider identity, and stable opaque provider
handle, then makes both real OCI backends inspect that exact durable owner
before create, delete, or retry.

No structured autoreview runs during implementation. Focused tests, affected
gates, static scans, and owner inspection are the convergence loop. Exactly one
full GPT-5.6 Sol/xhigh/fast review runs only after every criterion below is
green and the item diff is candidate-frozen. A material accepted defect permits
one narrow correction review after affected proofs rerun.

## Read-Only Audit

Three bounded read-only audits covered Container, Krun, and the portable
network state/store seam. All reported zero changed paths. Owner inspection
confirmed:

1. `DurableNetworkResourceState` already owns exact plan/resource/generation,
   digest, lease-epoch, phase, and opaque-handle validation.
2. `LocalNetworkStateStore` already owns the crash-safe, cross-process,
   checksum/versioned transaction boundary. It lacks an attachment partition.
3. Production attachment checkpoints are no-ops; no canonical whole-attachment
   phase survives process death.
4. Existing `NetavarkProviderOperation` is exact durable provider-attempt
   evidence, but is not a complete attachment aggregate and its pure inspector
   is test-only.
5. Current attach replaces an existing namespace before consulting provider
   attempt state.
6. Stable attachment handles and transient Netavark operation attempts are
   different layers: desired attachment generation stays fixed across admitted
   workload restart, while provider attempts may repeat.
7. Container and Krun already share one lifecycle route. The durable seam must
   deepen that owner rather than add backend-local state machines.

## Binding Design

### Portable authority

Add a concrete `LocalNetworkAttachmentAuthority` in `nimbus-network`:

- one new `AttachmentStates` partition in the existing store;
- one globally keyed collection whose record explicitly carries `TenantId`;
- key validation against tenant plus embedded `NetworkAttachmentId`;
- selected `NetworkProviderId` durable even before an opaque handle exists;
- the existing `DurableNetworkResourceState` as the only phase/version owner;
- exact replay, get, deterministic list, transition, and handle-adoption
  operations;
- full collection validation before and after every transaction;
- manager-derived `attachments()` access only; no repository trait or fake.

Wrong tenant, resource kind, plan identity, generation, digest, lease epoch,
selected provider, handle provider, checksum, schema, or key/record correlation
fails without changing authority bytes.

`nimbus-network -> nimbus-core` remains the only workspace edge. The portable
crate learns no Netavark, namespace, socket, filesystem layout, policy, service,
proxy, machine, workload, or cluster effect.

### Stable handle versus transient attempt

The attachment record stores one stable, provider-scoped opaque realization
handle for the workload incarnation. It is not an IP address, namespace path,
Netavark network name, or transient setup/delete attempt.

Sandbox IPAM's `NetavarkProviderOperation` remains subordinate exact operation
evidence and may mint a new setup/delete attempt during a same-generation
restart. The aggregate record never accepts a different stable handle for the
same resource generation.

### Restart cycle

The desired attachment resource is not released during an admitted workload
restart. Add one narrowly evidenced state edge:

```text
Deleting | CleanupPending
  -- DeletionConfirmedForReprovision -->
Provisioning
```

This edge requires inspected provider absence, retains the same plan,
attachment ID, generation, digest, lease epoch, selected provider, and stable
handle, and cannot leave `Released` or `Failed`. All terminal resurrection
remains rejected. The exhaustive state truth table expands with the new
evidence value.

### Sandbox adapter

The shared OCI lifecycle receives the manager-derived attachment authority.
Sandbox owns:

- deterministic provider-neutral desired-plan compilation for the current
  tenant-qualified workload incarnation;
- the transitional generation/epoch rules already fixed by this plan;
- mapping Container/Krun registrations to exact selected provider IDs;
- stable opaque handle construction and authentication;
- side-effect-free Netavark/IPAM plus namespace inspection;
- phase/outcome decisions before any retry effect;
- durable phase checkpoints around the existing shared algorithm.

Provider-specific observed data never enters `nimbus-network`.

## Phase and Inspection Contract

Every entry authenticates durable version/provider state and performs provider
inspection before create, delete, or retry:

| Durable phase | Confirmed absent | Exact present | Unknown/conflict |
| --- | --- | --- | --- |
| `Reserved` | enter `Provisioning`; create may proceed | fail closed; no effect | fail closed; no effect |
| `Provisioning` | retry create | adopt exact stable handle, enter/resume `Ready` | enter `CleanupPending`; no retry |
| `Ready` | fail closed for later NNC5.4 convergence | resume publication without create | enter `CleanupPending` |
| `Publishing` | fail closed for later NNC5.4 convergence | resume publication without create | enter `CleanupPending` |
| `Active` | fail closed for later NNC5.4 convergence | exact replay; no create | enter `CleanupPending` |
| `Withdrawing` | continue fenced teardown | continue exact teardown | enter `CleanupPending` |
| `Draining` | continue fenced teardown | continue exact teardown | enter `CleanupPending` |
| `Deleting` | release or reprovision according to explicit caller mode | continue exact teardown | enter `CleanupPending` |
| `CleanupPending` | release or reprovision only with confirmed evidence | continue exact teardown | remain fenced |
| `Released` | terminal no-op | invariant incident; no effect | invariant incident; no effect |
| `Failed` | terminal no-op | invariant incident; no effect | invariant incident; no effect |

NNC5.2 does not claim complete crash convergence from every host effect; NNC5.4
owns real process-kill cuts and idempotent partial-effect convergence. NNC5.2
proves the load-bearing prerequisite: no persisted phase blindly recreates or
deletes before authenticated inspection.

## Later-Owner Boundaries

NNC5.2a remains the sole owner of:

- durable coordinator reservation plus exact attachment-to-segment
  association in the canonical record;
- provider operation attempt before the first namespace/provider effect;
- closing the namespace-created-before-attempt crash interval;
- replacing filename-only orphan inference;
- the eight-row hold/desired/effect/manifest/stale/unknown orphan matrix.

NNC5.3 remains the sole owner of complete readiness beyond coarse provider
presence. NNC5.4 owns real process-kill crash cuts. NNC5.6 and NNC6 own
side-effect-free workload inspection, restart-policy decisions, exit-receipt
coordination, and the durable cross-domain workload saga. NNC5.2 does not add
attachment phase fields to Container or Krun manifests; manifests remain
supporting evidence, not a second authority.

## Expected-Red Proof Packet

Before correcting production behavior, capture failures for:

1. real-store reopen preserving tenant, exact resource version, phase, selected
   provider, and redacted stable handle;
2. wrong tenant, attachment/resource kind, plan ID, generation, digest, epoch,
   selected provider, and handle provider failing byte-preservingly;
3. checksum-valid key/record mismatch refusing the portable authority;
4. explicit deletion-confirmed reprovision being the only nonterminal
   delete-to-provision edge;
5. the identical Container/Krun table covering all eleven durable phases and
   absent/present/unknown provider evidence;
6. provider inspection being call one before namespace create/remove,
   Netavark setup/teardown, listener mutation, or backend publication;
7. `Provisioning + present` adopting the exact stable handle without a second
   provider setup;
8. terminal contradictory evidence performing zero provider effects;
9. fresh `LocalNetworkAttachmentAuthority`/backend construction over the same
   state root making the same decision without handed-over in-memory state;
10. store corruption or lock failure occurring before provider inspection and
    effects.

NNC5.2a's eight orphan rows and first-effect attempt proof remain explicitly
red. NNC5.4's real subprocess kill-at-Nth-effect cuts remain explicitly red.

## Owned Paths

Portable owner:

- `crates/nimbus-network/src/attachment_state.rs` (new);
- `crates/nimbus-network/src/attachment_state/tests.rs` (new);
- `crates/nimbus-network/src/state.rs`;
- `crates/nimbus-network/src/state_store.rs` (partition only);
- `crates/nimbus-network/src/manager.rs`;
- `crates/nimbus-network/src/lib.rs`.

Sandbox owner:

- `crates/nimbus-sandbox/src/backends/capabilities.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/network_launch.rs`;
- directly related stale recovery fixtures in
  `container/runtime/launch_cleanup.rs` and
  `container/runtime/tests/provider_cleanup/netavark_restart.rs`;
- `crates/nimbus-sandbox/src/backends/container/runtime/tests.rs` and new
  `tests/attachment_authority.rs`;
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`;
- `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs` and new
  `tests/attachment_authority.rs`;
- `crates/nimbus-sandbox/src/backends/oci/network.rs`;
- `crates/nimbus-sandbox/src/backends/oci/network/process.rs`;
- `crates/nimbus-sandbox/src/backends/oci/network/process/tests.rs`;
- `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs` and a concept child
  if required to keep the current deep exception below 2,000 lines;
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`;
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests.rs`;
- new concept-owned `attachment_lifecycle/host.rs`, `state.rs`, `recovery.rs`,
  and `tests/durable_recovery.rs`.

Mechanical proof owner:

- `scripts/verify-nimbus-network-composition-census.mjs`;
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json`;
- this proof and the canonical plan ledger.

This list does not authorize orphan-reaper, service, compute, proxy, tenant,
system, machine, server, cluster, or unrelated cleanup edits.

## Acceptance Criteria

NNC5.2 is complete only when:

1. one concrete manager-derived attachment authority is canonical;
2. tenant, resource version, selected provider, phase, and stable opaque handle
   survive a genuinely fresh store/backend reopen;
3. all listed substitutions fail before effects and preserve bytes;
4. stable attachment handle and transient provider attempts cannot replace one
   another;
5. the explicit reprovision edge is exhaustive, evidence-gated, and cannot
   resurrect terminal state;
6. Container and Krun execute the identical eleven-phase inspection contract
   through their real backend-type routes;
7. every row proves inspection precedes create/delete/retry;
8. exact present state is adopted/resumed without duplicate provider setup;
9. unknown/conflicting evidence remains fenced and never becomes ready,
   released, or recreated;
10. terminal records perform zero provider effects;
11. store/schema/lock failures occur before provider inspection/effects;
12. no provider effect or new workspace edge enters `nimbus-network`;
13. NNC5.2a, NNC5.3, NNC5.4, NNC5.6, and NNC6 boundaries remain red and
    unduplicated;
14. every touched production module satisfies the repository modularity rule;
15. focused suites, full affected suites, all-target/all-feature check, strict
    Clippy, warning-denied rustdoc, format/diff, dependency/effect verifier, and
    private-doc gates pass with exact counts;
16. exactly one candidate-frozen Sol/xhigh/fast item review is dispositioned,
    with one narrow correction review only if an accepted executable defect
    materially changes code;
17. the plan header/ledger and this proof record exact commands, counts,
    executable digest, review identity/result, changed paths, and remaining-red
    later owners before the exact item commit.

## Planned Verification

```text
timeout 300 cargo nextest run -p nimbus-network --lib \
  -E 'test(/attachment_state|state::tests/)'
timeout 300 cargo nextest run -p nimbus-sandbox --lib \
  -E 'test(/attachment_lifecycle::tests::durable_recovery/)'
timeout 600 cargo nextest run -p nimbus-sandbox --lib \
  -E 'test(/backends::container::runtime|backends::krun::vm/)'
timeout 900 cargo nextest run -p nimbus-network -p nimbus-sandbox
timeout 600 cargo check -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features
timeout 900 cargo clippy -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-network -p nimbus-sandbox \
  --no-deps --all-features
cargo fmt --all --check
git diff --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Exact counts and any additional risk-driven lanes will be recorded after they
run. Passing compilation without behavioral assertions is not acceptance.

## Candidate Implementation

The candidate introduces one portable, manager-derived attachment authority
without moving provider effects into `nimbus-network`:

- `AttachmentStates` is a new partition in the existing checksum/versioned,
  file-locked `LocalNetworkStateStore`;
- `DurableNetworkAttachmentState` persists the tenant, selected provider, exact
  plan/resource/generation/digest/lease epoch, lifecycle phase, and redacted
  opaque provider handle;
- `LocalNetworkAttachmentAuthority` validates the entire collection before and
  after each transaction and exposes exact reserve, inspect, list, transition,
  and stable-handle adoption operations;
- the state machine permits `Deleting | CleanupPending -> Provisioning` only
  with `DeletionConfirmedForReprovision`, while `Released` and `Failed` remain
  terminal;
- `LocalNetworkAuthority::attachments()` derives the only production
  attachment handle from the manager-owned store, and `OciNetworkProcess`
  injects that handle into Container and Krun;
- direct backend construction reconstructs the same portable authority and
  caches any open/corruption failure as a startup fence before attachment
  inspection or effects;
- the common OCI lifecycle compiles one tenant-qualified portable desired plan,
  authenticates the durable record, and inspects namespace, exact IPAM
  generation, and exact Netavark operation before create, delete, or retry;
- exact present state is adopted without duplicate provider setup, exact
  in-flight operation state is cleanup-only, and unknown/conflicting evidence
  is fenced in `CleanupPending`;
- the stable handle is tenant-qualified by plan and attachment identity and is
  deliberately distinct from transient Netavark operation attempts.

Provider-specific observation and privileged netns/Netavark effects remain in
`nimbus-sandbox`. `nimbus-network` retains only `nimbus-core` as a workspace
dependency and imports no socket, Axum, Pingora, Netavark, nftables, gvproxy,
Iroh, tenant-policy, service-name, proxy-forwarding, machine-provider, or
cluster-transport owner.

## Behavioral And Quality Evidence

| Gate | Candidate result |
| --- | --- |
| Portable attachment authority/state | `15/15` passed. Reopen, redacted handle, tenant/resource/plan/generation/digest/epoch/provider substitutions, checksum-valid schema and key corruption, stable-versus-transient handles, terminal finality, reprovision evidence, and store contention are asserted behaviorally. |
| Shared OCI durable lifecycle/recovery plus production routes | Post-review `38/38` passed. The suite includes all `11 phases × 3 observations × 2 adapter profiles = 66` decisions, fresh authority reopen per row, corruption before inspection for both adapter profiles, exact-present adoption, cleanup-only in-flight attempts, terminal no-effect, ambiguity fencing, and four new tests entering through the actual Container/Krun constructors and private production `configure_network` routes. |
| Container/Krun affected lane | Post-review `332/332` passed, `458` filtered. The pre-review result was `328/328`. |
| Full affected crates | Post-review `1003/1003` passed with `24` declared skips. The pre-review final was `999/999` with one nextest leaky-process diagnostic. An earlier loaded pre-review run passed `998/999` and transiently failed the pre-existing concurrent preselected-identity test; that test immediately passed `1/1` in isolation, passed in the `328/328` lane, and passed in both final full runs. No production or test change was made on an unreproduced failure. |
| All-target/all-feature compilation | `cargo check -p nimbus-network -p nimbus-sandbox --all-targets --all-features` passed. |
| Strict lint | `cargo clippy -p nimbus-network -p nimbus-sandbox --all-targets --all-features -- -D warnings` passed. Displayed warnings are confined to existing vendored Brotli crates. |
| Warning-denied rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-network -p nimbus-sandbox --no-deps --all-features` passed. |
| Dependency/effect boundary | Cargo metadata and the aggregate verifier prove `nimbus-network -> nimbus-core` is the exact workspace edge and provider effects remain above it. |
| Aggregate verifier | Live verifier `17/17`; adversarial self-test `62/62`. The self-test's first `300s` harness bound expired after 21 passing mutation cases; the correctly bounded `900s` rerun completed `62/62`, including the `70`-case sovereignty mutation suite. |
| Format and patch integrity | `cargo fmt --all --check` and `git diff --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` pages; `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

## Modularity And Authority Check

- `attachment_lifecycle.rs` is a coherent deep lifecycle owner at `1,488`
  lines, below the repository review threshold after privileged effects moved
  to `68`-line `host.rs` and teardown/recovery decisions moved to `477`-line
  `recovery.rs`.
- The new portable production owner is `424` lines; its `598` lines of
  behavior tests are concept-owned separately.
- Existing `state_store.rs` and `ipam.rs` remain below 2,000 lines and retain
  their previously documented deep-module exceptions; NNC5.2 adds only a
  partition and narrow inspection surface respectively.
- No second attachment phase record was added to Container/Krun manifests.
  Manifests and IPAM attempts remain supporting/provider evidence, not desired
  attachment authority.
- NNC5.2a still owns durable exact attachment-to-segment association, the
  attempt-before-first-effect interval, and filename-independent orphan
  classification. NNC5.3 owns complete readiness; NNC5.4 owns process-kill
  cuts; NNC5.6 owns side-effect-free workload inspection; NNC6 owns the
  cross-domain saga.

## Exact Candidate Paths

The final candidate owns exactly these 32 paths. The reviewed correction had
31; the only post-review addition is the required plan-index routing truth-up:

```text
crates/nimbus-network/src/attachment_state.rs
crates/nimbus-network/src/attachment_state/tests.rs
crates/nimbus-network/src/lib.rs
crates/nimbus-network/src/manager.rs
crates/nimbus-network/src/state.rs
crates/nimbus-network/src/state_store.rs
crates/nimbus-sandbox/src/backends/capabilities.rs
crates/nimbus-sandbox/src/backends/container/runtime.rs
crates/nimbus-sandbox/src/backends/container/runtime/launch_cleanup.rs
crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs
crates/nimbus-sandbox/src/backends/container/runtime/network_launch.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/attachment_authority.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/netavark_restart.rs
crates/nimbus-sandbox/src/backends/krun/vm.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/attachment_authority.rs
crates/nimbus-sandbox/src/backends/oci/network.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/recovery.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/state.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests.rs
crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/durable_recovery.rs
crates/nimbus-sandbox/src/backends/oci/network/ipam.rs
crates/nimbus-sandbox/src/backends/oci/network/process.rs
crates/nimbus-sandbox/src/backends/oci/network/process/tests.rs
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
docs/private/plans/proof/nimbus-network-control-plane/nnc5.2-durable-attachment-state.md
scripts/verify-nimbus-network-composition-census.mjs
```

The original checkout retains exactly its four pre-existing user-owned paths,
and the `machine-os` companion remains clean. No rebase, destructive recovery,
push, or PR occurred.

## Candidate Review Gate

Criteria 1-15 are green, including the documentation gates over this exact
candidate wording. Criterion 16 now permits exactly one complete GPT-5.6
Sol/xhigh/fast structured review over this coherent item. Criterion 17 closes
only after the review identity/result, every finding disposition, executable
digest, final ledger truth, and exact local item commit are recorded.

The frozen initial pre-review executable diff has SHA-256
`bed1628a61092d3ce0507857a5408065387bf25cbdd0d9952fb5df96a4829b81`,
computed over the complete staged binary diff beneath `crates/` and `scripts/`.
The initial 27-path candidate had zero unstaged changes and passed
`git diff --cached --check`.

## Full Review And Accepted Correction

The one complete NNC5.2 item review ran only after criteria 1-15 were green.
The actual reviewer was GPT-5.6 Sol at `xhigh` reasoning in fast mode. It
reviewed the coherent candidate in one integrated `193,201`-byte pass under
thread `019fb301-f7f5-72c2-a4bb-f9cfa3dc7819` and reported one P1 test-gap
finding at confidence `0.98`, with overall incorrect probability `0.96`.

The finding is **accepted**. Although the 66-decision matrix uses the real
backend types' type-bound adapter constructors, it supplied a lifecycle and
host observer directly. It therefore could not detect a wrong
`attachment_authority` field, wrong-root reconstruction, omitted injection
into a backend's lifecycle, or failure to retain constructor corruption. That
left acceptance criteria 2, 6, and 11 under-proved.

The correction adds four concept-owned tests:

1. a fresh Container backend opens an existing conflicting selected-provider
   record, plans/adopts an exact launch, and reaches that conflict through the
   private production `configure_network` route before migration, namespace,
   or provider effects;
2. the identical proof runs through the fresh Krun constructor and production
   route;
3. a checksum-corrupt attachment store makes Container construction retain a
   startup fence, and normal planning leaves corrupt bytes and provider paths
   unchanged; and
4. the identical construction/planning fence runs for Krun.

The first execution was deliberately informative red at `0/4`: two fixtures
had not yet adopted the allocator reservation, and two had created a new
world-readable truncated file rather than corrupting an existing owner-mode
authority. After correcting those prerequisites, the exact route suite passes
`4/4`. The broader lifecycle/route filter passes `38/38`, the backend lane
passes `332/332`, and the full affected crates pass `1003/1003` with `24`
declared skips. Affected all-target/all-feature check, strict Clippy,
warning-denied rustdoc, format, and diff checks pass.

This correction changes only test modules and module routing; production
behavior is unchanged. It materially strengthens executable acceptance proof,
so the cadence permits exactly one narrow Sol/xhigh/fast correction review
focused on this accepted defect. No second full-item review is permitted.

The corrected executable diff has SHA-256
`fb7838cab3bd63940d8c6d41dc414e876ca522e1c53d33feea9bef22ce7fe0b7`,
computed over the complete staged binary diff beneath `crates/` and `scripts/`.
The corrected 31-path candidate has zero unstaged changes and passes cached
diff validation; documentation and ledger metadata remain outside that
executable identity.

## Narrow Correction Review

The one permitted narrow review ran only after every affected proof was green.
The actual reviewer was GPT-5.6 Sol at `xhigh` reasoning in fast mode. It
reviewed the complete corrected candidate in one integrated `211,240`-byte
pass under thread `019fb314-eac2-79b3-bc10-27e8d3e2e88e` and reported:

- zero findings;
- `patch is correct`;
- overall confidence `0.99`; and
- explicit confirmation that the four routed tests close the accepted proof
  gap without changing production behavior or crossing deferred NNC owners.

The full review's sole finding is therefore accepted, corrected, and closed.
No further NNC5.2 structured review ran or is warranted.

## Final Checkpoint

All seventeen written criteria are green. The final proof set is:

- production backend routes `4/4`, after the recorded informative fixture red
  run at `0/4`;
- portable attachment authority/state `15/15`;
- lifecycle plus actual backend routes `38/38`;
- affected Container/Krun lane `332/332`;
- full affected crates `1003/1003` with `24` declared skips;
- affected all-target/all-feature check, strict Clippy, warning-denied
  rustdoc, format, and cached/worktree diff checks pass;
- live dependency/effect/composition verifier `17/17`, with unchanged
  adversarial self-test evidence `62/62`;
- private-doc routing `108` pages and public-site validation `17/17`; and
- corrected executable SHA-256
  `fb7838cab3bd63940d8c6d41dc414e876ca522e1c53d33feea9bef22ce7fe0b7`
  over the executable subset of the exact 32-path staged candidate.

NNC5.2a durable exact attachment-to-segment association and attempt-before-
first-effect/orphan convergence, NNC5.3 complete readiness, NNC5.4 real
process-kill cuts, NNC5.5 band-wide locality, NNC5.6 side-effect-free workload
inspection, and NNC6 workload/network saga coordination remain intentionally
red in their canonical owners. The exact local NNC5.2 item commit is the only
remaining durability action; no push or PR is authorized.
