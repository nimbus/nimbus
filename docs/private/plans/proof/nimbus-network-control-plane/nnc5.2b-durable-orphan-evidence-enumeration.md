# NNC5.2b Durable Orphan Evidence Enumeration

Status: `complete; exact item commit pending`

Owner: `NNC5.2b`

Starting commit: `154a5af0c8137fbdaf998f635796dd18d108d043`

Starting tree: `d784f7799852ae29fba1132ab43f60a5a53a14f6`

## Purpose

The original NNC5.2b scope was prospectively split before executable
implementation because the
audited work contains three independently reviewable units of value:

1. `NNC5.2b` supplies exact read-only durable evidence and candidate
   enumeration;
2. `NNC5.2c` owns the pure exhaustive classifier; and
3. `NNC5.2d` owns exact quarantine application, startup integration, and
   deletion of filename-derived authority.

This split prevents partial implementation or autoreview chunks from becoming
ad hoc completion units. Each sub-item has its own acceptance criteria, proof,
review, and exact commit. The aggregate NNC5.2b-d outcome remains unchanged:
only exact current evidence adopts; every mismatch or unknown result is
quarantined without provider cleanup, release, finalization, or capacity reuse.
NNC8.3 remains the sole cleanup-convergence owner.

## Read-only audit result

The current startup path is not a durable-evidence classifier:

- `reaper.rs` enumerates persistent-netns filenames into a supposedly complete
  live set;
- root and child `read_dir` errors are collapsed into empty/skip behavior and
  `.flatten()` silently discards entry errors;
- tenant and sandbox filenames are converted into `TenantId` and
  `NetworkAttachmentId`, promoting an artifact path into identity;
- that incomplete set drives `NetworkSegmentAllocator::reconcile_orphans`;
- the allocator mutates held entries without authenticating desired
  attachment state, reservation claim, segment, epoch, provider attempt, or
  generation; and
- both Container and Krun already reconstruct the canonical attachment
  authority, but `reconcile_startup_network_state` does not receive it.

The erased `OciSegmentAllocator` capability can inspect one exact
claim-qualified association but cannot enumerate allocator-owned candidates.
The IPAM journal is tenant-partitioned and can inspect one known attachment but
cannot enumerate all tenant partitions or reconstruct the sandbox artifact
locator. Reading private serialized allocator/store state from the reaper would
duplicate authority and is forbidden.

The existing ignored eight-row test remains valid fail-before evidence because
it runs eight isolated real allocator/reaper cases and aggregates every
mismatch before failing. It is not pass-after acceptance: its desired/effect
JSON files and manifest path are synthetic, production never reads their
generation markers, its result is inferred from leftovers rather than returned
by a classifier, and five rows permit the weaker `removed-or-quarantined`
outcome.

Fresh audit command:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix \
  -- --ignored --nocapture
```

Observed before implementation:

```text
exit 101
0 passed; 1 failed; 799 filtered out
all eight named rows mismatched
```

First focused executable fail-before:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  nnc5_2b_ipam_persists_reversible_provider_locator_before_effects \
  -- --nocapture
```

Observed:

```text
exit 101
0 passed; 1 failed; 800 filtered out
provider_locator.sandbox_id was Null instead of "sandbox-original"
```

An earlier `--exact` invocation with only the short test name executed zero
tests and is explicitly rejected as evidence. The corrected command above ran
the named test and proves that the current real IPAM allocation commits no
reversible locator.

## Frozen ownership decisions

### One sandbox-owned reversible locator

The sandbox IPAM/provider-attempt record will persist a reversible locator
before the first attachment effect. It contains:

- the exact sandbox ID;
- a stable sandbox-owned artifact-realm ID;
- the OCI backend kind needed to authenticate the selected attachment
  provider; and
- enough schema validation to prove that the tenant partition plus sandbox ID
  derives the exact stored `NetworkAttachmentId`.

The artifact-realm ID is derived from the canonical workload root after the
existing layout-directory preparation and maps back to a path only through the
process-injected workload root. A stored absolute path, manifest path, netns
path, IP address, or IPAM address does not select a realm or authority. Startup
classifies only evidence whose realm matches its injected root; node-wide
discovery of a realm that is not configured in the process remains NNC8.3
work.

The locator is provider-adjacent sandbox evidence, not portable desired state.
It does not enter `nimbus-network`, does not become workload identity, and does
not authorize readiness, cleanup, or adoption by itself. A path, filename,
manifest, namespace, address, or interface name remains untrusted observed
evidence.

This is a pre-launch breaking wire change. Missing locator fields are
corruption, not a compatibility mode or inferred fallback.

### One exact allocator snapshot seam

`NetworkSegmentAllocator` will replace the broad
`reconcile_orphans(complete_live_set)` mutation with exact per-candidate
inspection and, in NNC5.2d, compare-and-swap quarantine. Candidates come
from desired attachment records, IPAM/provider evidence, or unmatched
artifacts; after a candidate supplies a canonical claim, the existing
`inspect_attachment_reservation` seam authenticates its reservation state and
association. Broad allocator-only inventory is deferred to NNC8.3 because an
unplaced reservation has no artifact realm and startup classification must not
race a live coordinator in another process.

The reaper may not parse the allocator's serialized state. The exact
observation exposes no CIDR-as-identity or provider effect.

### Typed partition enumeration

The local network store will provide typed, read-only partition enumeration so
the sandbox IPAM adapter can find effect-only candidates. Unknown or malformed
partition keys fail closed. The store remains effect-free and
`nimbus-network -> nimbus-core` remains its only workspace edge.

### No-hold quarantine

NNC5.2b only enumerates and authenticates evidence. NNC5.2d will persist a
sandbox-owned startup-admission quarantine for candidates that have provider
or artifact evidence but no allocator hold. It must not mint desired
attachment state or an allocator hold from artifacts. Where an exact current
allocator allocation exists without an attachment, exact segment/epoch
quarantine may fence its already-owned capacity; otherwise the durable
startup-admission quarantine is the positive disposition. NNC8.3 alone may
inspect, clean, release, finalize, or clear it.

### Startup order

Orphan classification/fencing must run before terminal IPAM tombstone
retirement can remove evidence relevant to an orphan candidate. Terminal
retirement remains its existing owner but cannot precede candidate capture.

### Current desired state

The classifier will compare real typed attachment versions and associations.
Generation, plan digest, selected provider, claim, segment, epoch, and provider
attempt mismatches are stale/conflicting evidence. Synthetic generation marker
files are not acceptance evidence. A stale candidate cannot mutate the current
winner.

## Candidate union

| Source | Contribution | Trust |
| --- | --- | --- |
| Attachment authority | Tenant-qualified attachment, version, phase, association, selected provider | Canonical desired authority |
| Segment allocator | Reservation/hold phase, claim or receipt, segment, epoch | Canonical capacity/hold authority |
| IPAM/provider journal | Tenant-qualified attachment, locator, segment, claim, provider attempt phase/identity | Canonical attempt authority; not desired state |
| Netns/status/manifest | Optional present/absent/unknown observation | Untrusted; never identity or authority |
| Observation error | Typed unknown evidence | Quarantine/fail closed |

Canonical candidates deduplicate by tenant-qualified `NetworkAttachmentId`.
Unmatched artifact locators remain separately reported quarantine candidates;
they are never hashed or parsed into desired identity. `NotFound` is the only
filesystem absence. Permission, iteration, metadata, symlink, invalid name,
non-UTF-8, checksum, parse, or provider-inspection failures are typed unknown.

## NNC5.2b acceptance criteria

1. The IPAM provider-attempt record durably stores and validates the exact
   reversible sandbox locator before namespace, listener, Netavark,
   machine-forwarding, or cleanup effects.
2. Exact replay preserves locator bytes; tenant, sandbox, attachment, backend,
   root, segment, or claim substitution fails without mutation.
3. A genuinely reopened authority reconstructs the same locator and provider
   attempt without manifest or filename inference.
4. Typed local-store partition enumeration returns every tenant-IPAM partition
   in deterministic order while validating every durable partition key;
   malformed or unknown keys fail closed.
5. Every canonical candidate uses the injected allocator's deterministic,
   read-only exact reservation inspection; observation changes no store bytes,
   and no allocator-only candidate is invented without a realm/claim source.
6. Candidate enumeration is a deterministic union of attachment, allocator,
   IPAM/provider, and current-root artifact observations with no hidden-source
   precedence.
7. Artifact names and contents cannot create or alter tenant, attachment,
   claim, segment, epoch, generation, digest, or provider authority.
8. All non-`NotFound` observation failures are retained as typed unknown
   evidence; no `flatten`, lossy filename conversion, or empty-set downgrade
   remains in the new collector.
9. This sub-item performs no classification, quarantine, provider cleanup,
   namespace/artifact removal, allocator/IPAM release/finalize, or capacity
   reuse.
10. `nimbus-network` remains effect-free with only the `nimbus-core` workspace
    edge; no reverse dependency or new provider abstraction is introduced.
11. Fresh-process, repeat-enumeration, substitution, malformed-state, and
    two-backend route tests pass with exact counts.
12. Modularity thresholds, affected checks, strict Clippy, warning-denied
    rustdoc, format/diff, dependency/effect scans, static verifier, and docs
    gates pass before the one item review.

## Prospective owned paths

Portable evidence seam:

- `crates/nimbus-network/src/segment.rs`
- `crates/nimbus-network/src/state_store.rs`
- their concept-owned tests

Sandbox evidence adapters:

- `crates/nimbus-sandbox/src/backends/oci/network/dto.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs`
- a concept-owned IPAM evidence/locator child
- `crates/nimbus-sandbox/src/backends/oci/network/placement.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
- allocator trait/implementation paths only if exact per-candidate observation
  needs a narrowly proven addition; broad inventory is not owned here
- a concept-owned orphan evidence/candidate module and tests

Plan/proof/verifier routing may change with the item. Container/Krun startup
composition is read-only proof scope for NNC5.2b; NNC5.2d owns the
production call-site mutation.

## Modularity constraints

- Move `reaper.rs` inline tests to a concept-owned child before adding the new
  evidence suite; the production root is 1,173 lines and must remain a thin
  composition root.
- Add IPAM evidence/locator logic in a child; `ipam.rs` is 939 lines and its
  provider operation is already concept-owned.
- Keep allocator enumeration/compare logic in a concept child rather than
  growing sandbox `segment.rs` beyond 1,500 lines.
- `state_store.rs` remains an explicit deep-module exception; partition
  enumeration must be a small coherent store operation with tests, not a
  second store owner.
- The aggregate verifier is already 1,499 lines; any new condition belongs in
  a child helper/contract and the aggregate remains a thin registry.

## Candidate implementation

The candidate establishes one deterministic, read-only evidence boundary:

- `OciAttachmentProviderLocator` persists the tenant, sandbox, concrete OCI
  provider kind, and a versioned SHA-256 identity of the canonical
  process-injected workload root before provider effects. It stores no path,
  address, interface, or artifact name as identity.
- The IPAM authority persists that locator atomically with the exact
  claim/segment/IP evidence. Reopen validates tenant, derived attachment,
  provider kind, locator realm, segment, addresses, claim, and the exact
  Netavark operation attempt.
- Netavark attempts are provider-, action-, tenant-, attachment-, generation-,
  and ULID-qualified. A domain-separated digest binds the exact reservation
  claim, segment, and provider locator into the operation handle. Every
  durable phase reauthenticates the locator and validates the exact setup or
  teardown generation before provider inspection, effect, or state
  transition.
- `LocalNetworkStateStore::tenant_ipam_tenants` parses every durable partition
  key under the existing lock, returns only typed tenant-IPAM partitions in
  deterministic order, and fails the whole read on unknown or malformed keys.
- `collect_oci_orphan_evidence` receives only three read capabilities:
  desired-attachment enumeration, provider-attempt enumeration, and exact
  claim-qualified allocator inspection. The concrete mutation-capable
  attachment, IPAM, and allocator authorities are confined to adapter
  implementations outside the collector interface.
- The collector forms one tenant-qualified union from desired and provider
  records, obtains exact allocator observations only for claim-bearing
  candidates, and observes current-root artifacts as present, absent, or typed
  unknown through a pinned directory capability. Foreign realms, symlinked
  directory owners, and unmatched artifacts remain separate and cannot create
  canonical attachment identity or traverse outside the authenticated
  workload realm.

The candidate deliberately has no disposition type and no classification,
quarantine, cleanup, provider execution, artifact removal, release,
finalization, or capacity-reuse entry point. NNC5.2c owns pure
classification; NNC5.2d owns startup quarantine and filename-authority
deletion; NNC8.3 owns cleanup convergence and broad allocator inventory.

## Acceptance ledger

| Criterion | Result and proof |
| --- | --- |
| 1 | Green. The allocation transaction constructs and validates the locator before committing IPAM and before any namespace, listener, Netavark, machine-forwarding, or cleanup effect. Every later provider transition reauthenticates the same locator. Unsafe sandbox path components fail before persistence. |
| 2 | Green. Exact replay is byte-stable. Tenant, sandbox, attachment, backend, artifact root, segment, claim, provider ID, action, operation tenant/attachment, generation digest, and malformed-attempt substitutions fail without authority mutation. |
| 3 | Green. A real subprocess reopens a `Provisioning` attempt and proves the exact provider handle and locator survive without manifest, filename, or handoff memory. |
| 4 | Green. Two portable-store tests prove deterministic tenant-IPAM inventory across reopen and fail-closed rejection of every unknown or malformed durable key. |
| 5 | Green. Each desired/provider claim gets only exact injected allocator inspection. Repeated enumeration preserves attachment, IPAM, allocator, and artifact bytes; allocator-only records are not invented. |
| 6 | Green. The report canonically orders the attachment/provider union and retains foreign-realm provider evidence, unmatched artifacts, and scan unknowns separately. |
| 7 | Green. Filename, final and intermediate symlink, artifact type/content, and different-root tests prove artifacts cannot mint or rewrite tenant, attachment, claim, segment, epoch, generation, digest, or provider authority. Capability-relative traversal cannot escape the authenticated root. |
| 8 | Green. Only `NotFound` becomes absent. Permission/type/directory-entry, symlink, realm-authentication, allocator-inspection, and malformed-state failures remain typed unknown or fail closed; the collector uses no `flatten`. |
| 9 | Green by interface and source inspection. The collector receives only read traits and contains no classifier or mutation/effect capability. |
| 10 | Green. Cargo metadata reports exactly `nimbus-network -> nimbus-core`; aggregate NNCV012 and the live dependency/effect scan pass. |
| 11 | Green. The full-review fail-before is `0/5` and correction proof is `5/5`; the narrow-review fail-before is `0/2` with both tests exiting 101 and the corrected residual proof is `3/3`. Complete orphan evidence is `18/18`; the focused network/IPAM/Netavark lane is `88/88`; and the complete affected rerun is `1048/1048` with 26 declared skips. |
| 12 | Green. Check, strict Clippy, warning-denied rustdoc, format/diff, dependency/effect/census verifier, adversarial verifier self-test, and both documentation gates pass. |

## Behavioral and quality evidence

| Gate | Candidate result |
| --- | --- |
| Accepted-defect fail-before/correction | The full review's five exact tests failed `0/5` before correction and pass `5/5`. The narrow review's pinned-realm and terminal-IPAM bypasses each failed independently with exit 101 (`0/2`); the corrected pinned-realm, terminal release, and exhaustive two-safe-phase matrix pass `3/3`. Placement provider/config pairing adds a separate `1/1` cleanup proof. |
| Orphan evidence behavior | `18` passed, `0` failed, with the deliberate subprocess entrypoint excluded from the parent test selection. |
| Focused network/IPAM/Netavark lane | Final candidate `88/88` passed. One churn case crossed Nextest's 45-second slow-reporting threshold under concurrent machine load but completed within the bounded run. |
| Affected crates | The first final-candidate run passed `1047/1048` with 26 skips and exposed one unrelated Conmon process-group timing failure while six external CPU spinners and another engine stress suite were active. That exact test immediately passed `1/1` in isolation. With no source change, the complete bounded two-test-concurrency rerun passed `1048/1048` with the same 26 declared skips. |
| All-target/all-feature compilation | `cargo check -p nimbus-network -p nimbus-sandbox --all-targets --all-features` passed. |
| Strict lint | `cargo clippy -p nimbus-network -p nimbus-sandbox --all-targets --all-features -- -D warnings` passed; displayed warnings remain confined to existing vendored Brotli crates. |
| Warning-denied rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-network -p nimbus-sandbox --no-deps --all-features` passed. |
| Dependency and effect contract | Post-correction Cargo metadata reports only the `nimbus-core` workspace edge. The new `cap-std` dependency remains in the sandbox effect owner and does not enter `nimbus-network`. Live aggregate verifier passes `18/18`, including the exact production-authority census. |
| Adversarial static proof | Post-correction aggregate verifier self-test passes `67/67` under an explicit 1,800-second bound. |
| Format and patch integrity | Post-correction `cargo fmt --all --check`, `git diff --check`, and `git diff --cached --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` pages; `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

## Audit finding dispositions

Two bounded manual seam audits ran before candidate freeze; neither was a
structured autoreview and neither changed owned paths. Every finding was
resolved in this candidate:

1. provider-attempt identity now authenticates provider, action, tenant,
   attachment, and canonical ULID in every durable phase;
2. unsafe sandbox path components fail during locator construction, before
   persistence;
3. the fresh-process proof now reopens a real ambiguous `Provisioning`
   attempt, not only a `Reserved` record;
4. Container and Krun route evidence now enters through each production
   backend type's associated provider-kind constant;
5. the collector receives least-authority reader traits rather than full
   mutation-capable authorities; and
6. the partition criterion now states the implemented contract precisely:
   every tenant-IPAM partition is returned while every durable key is
   validated.

The item-local cleanup moved the intact private `reaper.rs` test module into
`reaper/tests.rs`: the production composition root is now `401` lines and the
concept-owned test child is `788` lines. After all accepted-finding
corrections, `orphan_evidence.rs` is `773` lines, its reader child remains
`65`, and its test child is `1,417`. `provider_operation.rs` is `1,015`,
`evidence.rs` is `270`, `provider_locator.rs` is `315`, `ipam.rs` is `1,046`,
its test child is `952`, and `placement.rs` is `764`.
The `2,105`-line
`state_store.rs` remains the plan's explicit coherent deep-module exception;
this item adds one small typed inventory operation and two colocated tests, not
a second store owner.

## Structured review dispositions

Exactly one full GPT-5.6 Sol review ran over the complete pre-correction item
at `xhigh` reasoning in fast mode. It found four accepted defects:

1. **Provider locator was not reauthenticated through the lifecycle.**
   Corrected setup execution/completion and teardown
   execution/confirmation/completion all authenticate the exact locator before
   transition or effect. Foreign-root fail-before tests cover each stage.
2. **Provider-attempt handles did not bind the capacity generation.**
   `NetavarkOperationGeneration` now binds attachment, reservation claim,
   segment, and provider locator into a domain-separated SHA-256 digest carried
   by the breaking `v1` handle. Claim, segment, provider, action, tenant,
   attachment, digest, and ULID substitution fail closed.
3. **Ambient artifact traversal could follow an intermediate symlink outside
   the authenticated realm.** Enumeration now pins the configured workload
   root with a directory capability and resolves all descendants relative to
   it. Symlinked directory owners and escaped descendants produce typed unknown
   evidence without reading the foreign target.
4. **Terminal evidence admitted provider phases that could still represent an
   effect.** Terminal IPAM evidence now accepts only `Reserved` or `Detached`;
   `Provisioning`, `Provisioned`, `Deprovisioning`, and unknown phases fail
   closed.

The exact fail-before bundle failed `0/5`, all corrections pass `5/5`, and the
complete affected/static proof above is green. Because these findings
materially changed executable lifecycle and observation code, the owner
cadence permitted exactly one narrow correction review limited to these four
defects.

That one GPT-5.6 Sol/xhigh/fast narrow review found two accepted residual
bypasses:

1. **Provider evidence re-resolved the ambient workload path instead of
   authenticating the directory capability actually scanned.** The breaking
   artifact-realm `v2` identity is now derived from the opened directory
   handle's stable platform identity. The collector authenticates provider
   evidence against its already-pinned `Dir`; retargeting the injected path
   cannot join evidence from the new realm to artifacts read from the old
   capability.
2. **Terminal IPAM mutation authenticated claim and phase but not the exact
   provider locator/backend.** Live-to-terminal transition, terminal replay,
   never-realized reconciliation, startup retirement, and direct retirement
   now reconstruct and compare the exact tenant/sandbox/realm/backend locator
   under the store transaction. An exhaustive test covers both permitted
   phases (`Reserved` and state-machine-proven `Detached`) and proves foreign
   realm and foreign backend rejection is byte-preserving at transition,
   replay, and retirement.

Their two fail-before tests each exited 101, and the three corrected focused
tests pass. Strict Clippy then identified two positional-argument regressions
created by exact provider threading. The item-local cleanup moved
never-bound port compensation back into the lifecycle that owns it and paired
placement provider identity with its config builder. A direct `1/1` test proves
provider/config divergence fails before IPAM. No lint allowance or
compatibility shim was introduced.

The review cadence is now exhausted: one full item review and the one justified
narrow correction review ran. No third structured review ran or is warranted.

## Candidate freeze and review gate

The pre-correction freeze contained exactly `26` owned paths and executable
SHA-256
`c2dcc380776ea8bfc5c0f51e860878c375390e935ec89b9dbec21287c8b4c6f5`.
The final freeze contains exactly `40` owned paths: `36` executable paths and
four plan/proof/census paths. The additional executable paths are the
Container/Krun/lifecycle callers and tests that must carry exact backend
identity through compensation, replay, and retirement; they do not add a new
authority. `cap-std` remains confined to the sandbox effect owner. There are
zero unstaged changes; cached and working-tree diff checks pass. The complete
staged executable SHA-256 over `Cargo.lock` plus `crates/` is
`90029aaeb486d651bee4c237e8d6a224ffc17b17b6ad2bb48fab852119f69156`;
the crate-only SHA-256 is
`80d9dde4502ee6f19b7550e15b6d0977cdad6f6698fbaf384f28c897b6deae9b`.

## Next proof step

Run the two documentation gates, restage the four documentation/census paths,
verify the executable digest is unchanged, and create one exact local NNC5.2b
commit. Then begin NNC5.2c's read-only pure-classifier audit. Do not run another
review, push, or open a PR.
