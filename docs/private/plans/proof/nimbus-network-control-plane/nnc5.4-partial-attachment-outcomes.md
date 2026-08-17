# NNC5.4 — Partial Attachment Outcome Convergence

Status: `complete; exact item commit pending`

Source checkpoint:

- commit: `041c256d34af0035e3def013de823ad08541995b`
- tree: `fd60570b516be49699b8be06c8e01174c5d6e207`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- source was clean when the item started
- original dirty checkout and clean `machine-os` companion: inspected only,
  unchanged

## Unit Of Value And Prospective Split

The original NNC5.4 criterion combined two different effect owners:

1. the shared sandbox-owned OCI attachment lifecycle used by host-managed
   Container and Krun networking;
2. Container-only machine-forwarded local proxy and gvproxy publication
   batches.

The first unit owns portable attachment phases, the sandbox IPAM/Netavark
attempt journal, persistent netns/status evidence, host-managed listener
lifetimes, and segment authority. The second unit owns provider-generation
HTTP observation, exposed/absent receipt batches, proxy workers, and per-route
withdrawal progress. Reviewing them together would make one item span two
independent state machines and repeat the oversized-item failure mode.

Before executable work, the canonical plan therefore split the units:

- NNC5.4 owns the shared Container/Krun attachment create/delete crash matrix;
- NNC5.4a depends on NNC5.4 and owns machine-forwarded fail-Nth publication and
  withdrawal ambiguity.

NNC5.6 still owns side-effect-free workload inspection and restart decisions.
NNC8.3 still owns startup orphan cleanup, artifact removal, release,
finalization, and eventual capacity reuse. Neither later authority moves here.

No structured autoreview runs during audit, fail-before work, implementation,
cleanup, or acceptance convergence. Exactly one GPT-5.6 Sol/xhigh/fast review
runs only after all NNC5.4 criteria are green and the item is candidate-frozen.
Only an accepted executable-code defect permits one narrow correction review.

## Read-Only Audit Result

The shared lifecycle already has the correct architectural authorities:

- `DurableNetworkAttachmentState` owns desired lifecycle phase, exact
  association, selected provider, stable provider handle, and generation;
- the sandbox IPAM `NetavarkProviderOperation` journal remains the sole
  transient setup/delete-attempt authority;
- provider inspection authenticates the tenant, attachment, setup attempt,
  assigned addresses, persistent namespace, and status projection before a
  retry decision;
- `LocalPortLeaseAuthority` owns durable listener phase while
  `NetavarkPortLifetimeRegistry` holds only process-local lifetime guards;
- `NetworkSegmentAllocator` retains or quarantines the exact association until
  confirmed provider absence permits release.

Existing proofs cover ordinary returned failures, same-process retry, durable
attempt-before-effect ordering, and a Netavark-only fresh-process
`Provisioning`/`Deleting` response-loss matrix. They do not run the complete
shared attachment algorithm through real process death at every lifecycle
boundary.

The first concrete gap is the publication-recovery interval:

```text
durable attachment Publishing
  + exact provider Ready/present
  + durable port binding Active
  + former process lifetime dead
  + no new process-local lifetime registry entry
```

The Active terminal path already reclaims the exact dead-owner listener
lifetime without a provider setup. The Ready/Publishing resume path instead
enters the fresh claim path. The expected-red proof must show whether it
rejects or duplicates authority rather than reclaiming the exact Active
binding.

The delete path already persists a teardown attempt before effects and
distinguishes provider `Deleting`, `DetachedProjectionPending`, and terminal
`Detached`. Its complete lifecycle still needs real process-kill proof around
provider withdrawal, provider acknowledgement, namespace removal, listener
cleanup, IPAM/segment release, and portable terminal publication.

## Current And Target Crash Decisions

```text
current
  durable association + provider attempt
  -> effect
  -> returned-error compensation / same-process tests
  -> incomplete real-process coverage

target
  durable association + exact attempt
  -> named effect boundary
  -> process killed without Drop/compensation
  -> genuinely fresh process opens only durable roots
  -> exact inspection
  -> resume without duplicate effect
     OR enter CleanupPending with every authority fence retained
     OR finish confirmed detach exactly once
```

A crash-cut marker is synchronization evidence only. It is never provider
truth. Recovery decisions use the reopened attachment, IPAM attempt, namespace,
status, listener, and allocator evidence.

## Named Create Cuts

The test vocabulary is scoped to the shared attachment owner:

| Cut | Boundary reached before process death | Required fresh-process result |
| --- | --- | --- |
| `attachment.create.provider_attempt_prepared` | exact setup attempt durable; no host effect | execute that attempt at most once; do not mint another |
| `attachment.create.namespace_created` | persistent netns created | exact inspection either resumes safely or fences CleanupPending; never blind setup |
| `attachment.create.listener_claims_held` | every listener claim/lifetime held | no claim substitution or lease reuse |
| `attachment.create.provider_ready` | provider setup and exact Ready evidence durable | no second Netavark setup |
| `attachment.create.publishing` | portable phase is Publishing | resume the same publication generation |
| `attachment.create.listeners_active` | durable bindings Active; owner lifetime still process-local | reclaim only after death proof; no second bind generation |
| `attachment.create.backend_publication_complete` | backend callback completed | idempotent callback replay may occur, but provider setup and listener bind do not |
| `attachment.create.lifetime_registered` | process-local batch registered | owner death is detected and one new lifetime generation is installed |
| `attachment.create.attachment_confirmed` | allocator hold confirmed | no duplicate hold or provider setup |
| `attachment.create.active` | exact Active record durable | readiness can be rebuilt without create |

Pre-effect generation/lease/association validation remains covered by NNC5.1
and NNC5.2a and is not mislabeled as a crash-after-effect cut.

## Named Delete Cuts

| Cut | Boundary reached before process death | Required fresh-process result |
| --- | --- | --- |
| `attachment.delete.attempt_prepared` | portable Deleting and exact teardown attempt durable | same attempt is inspected; no replacement delete attempt |
| `attachment.delete.backend_withdrawn` | runtime/PEP callback reports withdrawal | retry remains idempotent and cannot release authority early |
| `attachment.delete.segment_quarantined` | final detach quarantined exact hold | allocation is unavailable to replacement work |
| `attachment.delete.listener_cleanup_prepared` | listener claims are Withdrawing under dead process lifetime | fresh process recovers only the exact claims |
| `attachment.delete.provider_detached` | provider absence acknowledged in IPAM | provider delete is not replayed |
| `attachment.delete.namespace_removed` | persistent netns absent | absence is observed before listener/IPAM release |
| `attachment.delete.listeners_settled` | listeners are restart-retained or released as requested | no host-port reuse before provider absence |
| `attachment.delete.ipam_released` | terminal IPAM witness durable | setup cannot reuse the generation |
| `attachment.delete.segment_released` | exact hold/capacity finalized | only Final may reach this cut |
| `attachment.delete.attachment_terminal` | portable Released or restart Provisioning durable | replay is byte-stable and effect-free |

## Frozen Acceptance Criteria

| ID | Criterion |
| --- | --- |
| R1 | A real parent/child process harness kills the child at each named create and delete cut; recovery and replay run in genuinely fresh processes over only the durable roots. Every wait is bounded and diagnostics include child status/stdout/stderr. |
| R2 | Both production type-bound host-managed adapters, Container and Krun, run the identical crash matrix. Tests may use deterministic sandbox-owned host effects but may not manufacture a different lifecycle algorithm. |
| R3 | Every create recovery authenticates the exact tenant, attachment, association claim, segment, epoch, provider kind, setup attempt, stable provider handle, listener identities, and durable phase before any retry effect. |
| R4 | Prepared setup reopens with the same attempt; provider-present `Provisioning`/`Ready`/`Publishing` never performs a second setup; unknown or incomplete provider evidence enters or retains `CleanupPending`. |
| R5 | A dead owner after Active listener publication is reclaimed under a strictly newer lifetime generation. A still-live owner, substituted binding, claim, provider, or generation fails byte-preserving before recovery. |
| R6 | Fresh-process create convergence produces exactly one Active desired attachment or one named CleanupPending outcome. It never creates a second segment hold, IPAM generation, stable handle, listener lease generation, or provider setup. |
| R7 | Every delete attempt is durable before backend withdrawal/provider effects. Reopen authenticates and reuses the exact attempt or inspects committed absence; it never blindly reissues an ambiguous delete. |
| R8 | Delete response loss after provider acknowledgement performs zero duplicate provider deletes. Projection removal and namespace removal remain independently retryable and idempotent. |
| R9 | Final detach quarantines before provider/listener/IPAM/segment release. No lease, IPAM generation, segment slot, or attachment identity becomes reusable while provider/netns/listener outcome is present or unknown. |
| R10 | Restart detach retains the exact segment, IPAM, and listener generations and returns the attachment to Provisioning only after confirmed absence. Final detach reaches Released exactly once. Terminal replay is effect-free and byte-stable. |
| R11 | Primary and recovery diagnostics are both preserved. An unprovable effect produces a typed/named CleanupPending result with all retry witnesses retained, never a generic success or inferred absence. |
| R12 | Existing ordinary-error, same-process retry, Netavark response-loss, readiness, and NNC3.8 listener-lifetime proofs remain green; the new matrix adds process-death evidence rather than replacing them. |
| R13 | Production changes remain in concept-owned shared attachment/recovery/port-lifetime modules. Container/Krun callers stay thin and no duplicated backend switchboard appears. Files at or above repository thresholds are split or explicitly justified here. |
| R14 | `nimbus-network -> nimbus-core` remains the sole initial workspace edge. No socket, Netavark, nft, gvproxy, proxy, tenant, service, server, system, machine, transport, or cleanup effect enters the portable crate. |
| R15 | NNC5.4a, NNC5.6, and NNC8.3 ownership remains intact. This item adds no machine-forwarding batch manager, restart-policy decision, startup orphan remover, capacity reaper, or compatibility shim. |
| R16 | Focused happy/edge/error/crash tests, full affected suites, all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, dependency/effect scans, static verifier and adversarial mutations, and docs gates pass with exact counts. |

The structured review is a mandatory item closeout gate, not an implementation
acceptance criterion. This cadence clarification follows the owner directive
received after the criteria were frozen; it does not remove or weaken the
review. Exactly one full GPT-5.6 Sol/xhigh/fast review runs only after R1-R16
are green and the complete item is candidate-frozen. Only an accepted
executable-code defect permits one narrow correction review.

## Expected-Red Packet

Before production correction, add and run:

1. a real process-kill create case at
   `attachment.create.listeners_active` proving the current
   Ready/Publishing recovery cannot yet reclaim an exact dead-owner Active
   listener lifetime;
2. a real process-kill delete case at
   `attachment.delete.provider_detached` proving the full lifecycle, not only
   the lower Netavark helper, can reopen and finish without a duplicate delete;
3. the full named matrix as expected-red/expected-green rows, recording which
   cut first fails and preserving every already-green row;
4. substitution cases for tenant, claim, segment/epoch, provider attempt,
   listener binding, and lifetime generation, each asserting unchanged durable
   bytes and zero provider effects;
5. an unknown-provider case asserting CleanupPending and non-reuse.

Record exact command, exit status, pass/fail/ignored counts, failing assertion,
and the source checkpoint. Do not weaken a green row to make the matrix uniform.

### Captured fail-before

The first real-process parent case was added without a production correction
and run from source checkpoint
`041c256d34af0035e3def013de823ad08541995b`:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects \
  -- --nocapture
```

The command exited `101`. The parent reported `0 passed; 1 failed; 0 ignored;
892 filtered out`; the exact create-recovery child reported `0 passed; 1
failed; 0 ignored; 892 filtered out`. After the child was killed at
`attachment.create.listeners_active`, the genuinely fresh recovery process
reopened:

- portable attachment phase `Publishing`;
- exact provider `Ready`/present evidence;
- the same active port-lease generation; and
- a dead former process-local listener lifetime.

Recovery then failed before another provider effect with:

```text
port lease ... is owned by a different launch reservation coordinator
```

This is the predicted shared-lifecycle defect: `attach_with_host` authenticates
the never-bound launch reservation path before dispatching the durable
`ResumePublication` decision. Only already-terminal `Active` attachments reach
the existing exact dead-owner listener-lifetime reconciliation seam. The
correction must make publication recovery state-directed and reuse that one
authority; it must not weaken fresh-create authentication or mint a second
manager.

After the narrow publication correction, the same unchanged parent advanced
through both Container and Krun create recovery and reached the delete cut. It
then exited `101` with the recovery child at `0 passed; 1 failed; 0 ignored;
892 filtered out`. Exact IPAM operation evidence was terminal `Detached`, the
status projection was already absent, and only the persistent namespace
artifact remained. Read-only inspection misclassified that exact
provider-absent/namespace-cleanup-pending combination as generic `Unknown`:

```text
durable attachment recovery from Deleting failed closed: provider inspection
is ambiguous and remains fenced: namespace presence true and Netavark phase
detached do not prove exact presence or absence
```

The delete correction must represent this exact state separately from both
provider presence and unknown evidence. Fresh recovery may remove only the
remaining authenticated namespace artifact; it must not reissue Netavark
delete, release listeners before namespace absence, or treat the state as
fully terminal before cleanup completes.

## Implemented Convergence

The correction remains inside the shared sandbox attachment owner:

- `attach_with` dispatches exact provider-present
  `Provisioning`/`Ready`/`Publishing`/`Active` state to a state-directed
  publication recovery path before the never-bound launch path;
- publication recovery authenticates the exact association, provider, handle,
  listener batch, claim, and generation, then reuses the existing dead-owner
  listener-lifetime reconciliation seam without another provider setup;
- cleanup-only and unknown provider evidence authenticate the exact
  attachment and listener authority before portable state is fenced;
- `DetachedNamespacePending` represents an exact acknowledged provider detach
  whose only remaining provider artifact is the authenticated persistent
  namespace;
- both shared host-managed routes remove that remaining namespace without
  repeating provider delete, then settle listeners before IPAM and segment
  release; and
- terminal release ordering moved into the 77-line
  `detach_release.rs` concept owner; and
- the real-process harness writes and syncs a full immutable pre-crash witness
  before either effect sequence, then independently derives and compares the
  exact resource version, stable identity, addresses, provider handle, and
  cut-specific release state after every fresh-process recovery and replay.

The production composition root is 1,473 lines, its test parent is 1,486, the
real-process crash child is 1,187, durable recovery is 792, and the added IPAM
test witness remains inside its 1,074-line concept owner. Every changed Rust
owner remains below the repository threshold.

The real-process matrix has 10 create and 10 delete cuts. It runs identically
for the production type-bound Container and Krun adapters. For every cut the
parent kills the effect-owning child, a fresh child reopens only durable roots,
and another fresh child replays the converged state. All child waits are
bounded to 15 seconds and every failure reports status, stdout, and stderr.

Create convergence is deliberately evidence-sensitive:

- `provider_attempt_prepared` may execute the exact persisted setup once;
- `namespace_created`, `listener_claims_held`, `provider_ready`, and
  `publishing` retain all witnesses in named `CleanupPending`;
- `listeners_active`, `backend_publication_complete`, `lifetime_registered`,
  `attachment_confirmed`, and `active` converge to the one desired Active
  attachment without a second provider setup.

Every Final delete cut converges to one Released attachment. Acknowledged
provider detach is never repeated, namespace cleanup is idempotent, and the
terminal fresh-process replay performs no backend withdrawal and preserves the
portable authority file byte-for-byte.

## Acceptance Evidence

| Gate | Candidate result |
| --- | --- |
| Historical fail-before | Exact create parent and recovery child each report `0 passed; 1 failed; 0 ignored; 892 filtered`, exit `101`, at `attachment.create.listeners_active`. After the narrow publication correction, the unchanged parent advances to the delete recovery child, which reports the same `0/1/0`, 892 filtered, exit `101`, at `attachment.delete.provider_detached`. |
| Review fail-before | The full review's machine-forwarded partial-publication and auxiliary-listener substitution regressions each fail `0/1/0`, 894 filtered. The narrow review's machine-forwarded Absent fallthrough and recovered-publication activation-compensation regressions each fail `0/1/0`, 895 filtered, exit `101`. Corrected `durable_recovery` passes `6/6`, 890 filtered, across the complete phase/inspection/compensation set. |
| Real-process matrix | `fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects` passes `1/1`, 895 filtered. It executes 40 killed children, 40 fresh recovery children, and 40 fresh replay children across 10 create plus 10 delete cuts for both Container and Krun. Four create cuts converge to `CleanupPending`, six to Active, and all delete cuts to Released. A synced pre-crash witness is the immutable baseline; every fresh process independently reconstructs and compares the exact tenant, attachment, plan ID/version/generation/digest, association/epoch, provider/stable handle, IPAM addresses/generation, allocator state, and cut-specific listener/release boundary. |
| Shared lifecycle matrix | The complete attachment-lifecycle group passes `59` tests with `5` declared child-role skips and `832` filtered. It covers exact association/provider/handle/listener substitutions, ordinary errors, compensation, same-process retry, both restart-retained detaches, terminal release, readiness, durable phase inspection, and both real adapters. |
| Listener-lifetime regression | The exact NNC3.8 Netavark listener-lifetime group passes `5/5`, 891 filtered: live-owner and substituted recovery preserve bytes, dead-owner recovery installs one higher generation, explicit empty batches remain authenticated, and release stays absence-gated. |
| Full affected behavior | Frozen `timeout 600 cargo test -p nimbus-sandbox --lib` passes `870` tests, fails `0`, and reports `26` intentional skips. This run includes the complete real-process NNC5.4 matrix and all existing ordinary-error, Netavark response-loss, readiness, startup, IPAM, segment, port-lifecycle, and backend integration regressions. |
| Affected quality | `cargo check -p nimbus-sandbox --all-targets --all-features`, strict no-deps Clippy with `-D warnings`, and warning-denied no-deps rustdoc pass. Strict Clippy first rejected an eight-argument resume helper; bundling the durable record and assigned addresses into `AttachmentPublicationRecovery` closed that issue. Provider-present recovery remains in its concept-owned module, leaving the production composition root at 1,473 lines. Only existing vendored Brotli diagnostics appear outside the strict affected crate. |
| Dependency/effect boundary | `cargo metadata --format-version 1 --no-deps` reports `nimbus-core` as the sole `nimbus-network` workspace dependency. Live NNCV000-NNCV020 passes `21/21`; its adversarial suite passes `101/101`, including nine NNCV020 mutations for missing create/delete cuts, swapped create/delete label-phase mappings, never-bound publication routing, erased detached-namespace state, absent no-duplicate-delete proof, an unbounded child, and a missing pre-crash witness. |
| Script and patch quality | Node and Bash syntax pass. ShellCheck passes with the aggregate verifier's documented inherited SC2034/SC1091 exclusions. `cargo fmt --all --check` and `git diff --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` link-clean pages and `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

## Structured Review Disposition

The sole full item review ran after the original R1-R16 candidate was frozen:

- reviewer: GPT-5.6 Sol, xhigh reasoning, fast mode;
- thread: `019fb777-809f-76f3-aa8c-b371caec5a01`;
- result: `patch is incorrect`, confidence `0.96`;
- findings: four P2 and one P3.

| Finding | Disposition and proof |
| --- | --- |
| P2 `0.98`: machine-forwarded partial publication entered the shared recovery seam without its deferred batch authority. | **Accepted.** Fail-before is `0/1/0`, 894 filtered: provider-present Publishing succeeds and returns assigned IPs. The correction rejects every partial or cleanup-only machine-forwarded observation before portable mutation with an explicit NNC5.4a diagnostic; exact Active observation remains an effect-free idempotent read path. Corrected durable-recovery set passes `6/6`. No machine batch protocol moved into NNC5.4. |
| P2 `0.96`: provider-present recovery omitted the auxiliary listener authentication before Publishing mutation. | **Accepted.** Fail-before is `0/1/0`, 894 filtered: a crossed same-claim auxiliary listener reaches the forbidden publication callback. The correction authenticates its exact tenant, sandbox, request, address, port, and provider before `mark_publishing`; rejection preserves portable bytes and permits inspection only. Corrected durable-recovery set passes `6/6`. |
| P2 `0.94`: add a purported `AttachmentAttachPhase::IpamAllocated` crash cut. | **Rejected with source evidence.** That phase does not exist. `prepare_provider_setup` atomically allocates/loads exact IPAM and persists the `SetupPrepared` attempt before returning; the next observer boundary is `ProviderAttemptAuthenticated`, already represented by `attachment.create.provider_attempt_prepared`. There is no post-IPAM/pre-attempt observable interval to name without inventing a production phase. The strengthened matrix now compares exact IPAM claim/segment/provider realm/addresses across that existing cut and every replay. |
| P2 `0.96`: the crash harness asserted terminal phases but not the exact identities and retained/released fences claimed by R5/R6/R9. | **Accepted.** Every fresh create/delete child now authenticates and compares exact tenant, stable attachment ID, provider, full resource version, claim/segment/epoch association, stable handle, IPAM generation/addresses, allocator Adopted/ProviderCleanupPending/Absent state, port settlement, and namespace absence. The full 120-child matrix passes `1/1`, 895 filtered. |
| P3 `0.99`: NNCV020 checked label presence but not label-to-phase binding. | **Accepted.** The verifier parses all 20 ordered label/phase pairs and compares them exactly. New create-phase-swap and delete-phase-swap mutations fail exclusively as NNCV020; the full adversarial suite passes `101/101`. |

The four accepted findings materially changed executable code or executable
proofs, so the owner cadence permits exactly one narrow correction review
focused on these defects. The rejected finding does not add a speculative
phase. No second full review is warranted.

## Narrow Correction Review Disposition

The one cadence-permitted narrow review ran once with GPT-5.6 Sol, xhigh
reasoning, and fast mode after the four accepted full-review corrections were
candidate-frozen. TruffleHog was clean. It returned `patch is incorrect` at
confidence `0.96` with three P2 findings; all three were source-validated,
accepted, and corrected:

| Finding | Disposition and proof |
| --- | --- |
| P2 `0.94`: machine-forwarded partial publication with provider `Absent` fell through the shared host-managed recovery path. | **Accepted.** The five-row partial-publication matrix first fails `0/1/0`, 895 filtered, exit `101`. `attach_with` now authenticates machine-forwarded recovery disposition immediately after read-only provider inspection. Only exact Active+Present is an effect-free existing-attachment read; Absent+Ready, Absent+Publishing, Absent+Active, Present+Publishing, and Unknown+Publishing all fail byte-preserving to NNC5.4a. |
| P2 `0.98`: a recovered publication whose final `mark_active` transition failed returned without compensating already-completed backend publication. | **Accepted.** The fail-before observer persists `CleanupPending` at `AttachmentConfirmed`; the test fails `0/1/0`, 895 filtered, exit `101`, after `[Inspect, BackendPublication]` with no teardown. Recovery now records cleanup pending and runs the same registered reverse compensation as fresh publication. The corrected test passes for both Container and Krun. |
| P2 `0.93`: post-crash state was compared to a baseline first captured by the recovery child rather than a complete pre-crash witness. | **Accepted.** The effect-owning child now writes and syncs `PreCrashWitness { attachment, assigned_ips }` before create effects or delete begins. Every successor compares the immutable record and independently derives the full resource version; stable-handle and released-IP checks remain cut-specific. NNCV020 requires those proofs, and its new missing-witness mutation fails closed. |

No third review is permitted or warranted. Changes after this narrow review
are exactly these accepted executable/proof corrections plus ledger wording
and formatting. Their affected fail-before, focused, full affected, quality,
static, adversarial, and docs proofs are green.

## R1-R16 Disposition

| Criterion | Evidence |
| --- | --- |
| R1-R2 | The 120-child bounded real-process matrix runs all 20 named cuts through both production type-bound adapters and only the shared lifecycle algorithm. |
| R3-R6 | Durable phase matrices and substitution tests authenticate exact tenant, attachment, claim, segment/epoch, provider/attempt/handle, listener identity, and generation before retry. Provider-present recovery never repeats setup; unknown/incomplete evidence is named `CleanupPending`; dead-owner Active bindings advance exactly one lifetime generation. |
| R7-R9 | The delete attempt and quarantine cuts precede effects and release. Fresh recovery reuses the exact attempt or acknowledged absence, never repeats an acknowledged delete, independently removes namespace projection, and withholds listener/IPAM/segment release until absence. |
| R10 | `container_10_restart_detach_retains_authority`, `krun_10_restart_detach_retains_authority`, both final-release tests, and every terminal fresh-process replay pass with exact retained generations and byte-stable terminal state. |
| R11-R12 | Primary/cleanup diagnostic tests remain green; child diagnostics include status/stdout/stderr; unprovable evidence stays typed and fenced. The full 870-test affected suite retains every preceding ordinary-error, response-loss, readiness, and listener-lifetime proof. |
| R13 | Shared concept owners remain narrow: production root 1,473 lines, test root 1,486, crash matrix 1,187, durable recovery 792, active reconciliation 367, authority 271, recovery 598, terminal release 77, and IPAM owner 1,074. Container/Krun callers remain thin and provider-present recovery lives in its concept-owned module rather than rebuilding the root switchboard. |
| R14 | Exact metadata and NNCV012-NNCV015 prove the portable crate retains only the `nimbus-core` workspace edge and no provider/transport/effect authority. |
| R15 | Machine-forwarded batch HTTP/receipt work remains NNC5.4a; inspect/restart policy remains NNC5.6; startup orphan cleanup/reuse remains NNC8.3. No compatibility shim or speculative provider surface was added. |
| R16 | Focused, full affected, check, strict Clippy, rustdoc, format/diff, dependency/effect, live static, 101-mutation adversarial, and both docs gates are green with the exact counts above. |

R1-R16 are green after the four accepted full-review and three accepted narrow
review corrections. The complete item is closed; the sole full review and one
narrow correction review have run and will not be repeated.

## Final Owned Paths

Primary test owner:

- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/crash_recovery.rs`
- the parent `attachment_lifecycle/tests.rs` module declaration only

Narrow production owners, only where the red proof requires:

- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/active_reconciliation.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/authority.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/detach_release.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/machine_forwarded.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/recovery.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/test_api.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/crash_recovery.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/durable_recovery.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/ipam.rs`

Static/verifier/proof owners:

- `scripts/verify-nimbus-network-attachment-crash-convergence.mjs`
- `scripts/nimbus-network-control-plane/attachment-crash-convergence-contract.sh`
- `scripts/verify-nimbus-network-control-plane.sh`
- this proof
- the canonical plan and routing ledgers

Forbidden paths unless a fail-before result forces an explicit plan amendment:

- `crates/nimbus-network/**` other than read-only dependency verification;
- Container/Krun workload restart policy and inspection owners;
- machine-forwarding HTTP/proxy/receipt owners reserved for NNC5.4a;
- startup reconciliation, orphan reaper, release/finalization owners reserved
  for NNC8.3;
- service, tenant, proxy-policy, server, system, machine, and cluster transport
  crates.

## Recovery Ledger

| Checkpoint | Status | Evidence / next action |
| --- | --- | --- |
| Source durability | `done` | NNC5.3a commit `041c256d34af0035e3def013de823ad08541995b`, tree `fd60570b516be49699b8be06c8e01174c5d6e207`. |
| Read-only call graph | `done` | Shared lifecycle, durable attachment state, Netavark attempt journal, provider inspection, port lifetime recovery, lower-level response-loss proof, and Container/Krun call sites mapped. |
| Prospective split | `done` | Shared host-managed crash convergence remains NNC5.4; machine-forwarded fail-Nth batches are NNC5.4a. |
| Acceptance criteria | `done` | R1-R16 frozen before executable changes. |
| Fail-before | `done` | Initial create red: parent and recovery child `0/1/0`, 892 filtered, due to never-bound authentication of exact Publishing/provider-ready/Active-listener evidence. After the narrow create correction, the unchanged parent reaches delete recovery and exits `101`; child `0/1/0`, 892 filtered, because exact provider `Detached` plus a remaining namespace is collapsed into generic ambiguity. |
| Implementation | `done` | Exact provider-present publication uses state-directed recovery; cleanup-only evidence is authenticated before fencing; typed `DetachedNamespacePending` removes only the remaining namespace without a duplicate provider delete; release stays listener/IPAM/segment ordered. Full-review corrections fence machine-forwarded partial evidence to NNC5.4a and authenticate the exact auxiliary listener. Narrow-review corrections fence provider-absent machine partials, compensate a recovered-publication activation failure, and prove recovery from one immutable pre-crash witness. |
| Focused/affected gates | `done` | Crash matrix `1/1`; durable recovery `6/6`; lifecycle `59` with 5 child skips; listener lifetime `5/5`; frozen Sandbox `870/870` with 26 skips; check/Clippy/rustdoc, core-only edge, live verifier `21/21`, mutations `101/101`, format/diff, docs `108`, and site `17/17` pass. |
| Candidate-frozen review | `done` | Sole full Sol/xhigh/fast review thread `019fb777-809f-76f3-aa8c-b371caec5a01` found five items: four accepted/corrected and one source-rejected nonexistent phase. The one narrow Sol/xhigh/fast correction review found three P2 defects; all three have exact fail-before, corrections, and affected proof. No third review ran or is warranted. |
| Ledger/commit | `done` | Canonical plan, routing row, and this proof record the exact 17-path item and executable SHA-256 `f964e7dd2f6a48db5bbbbd96f6dfe410fa9d113e19541926e8f987a6f87f36e6`; commit this exact checkpoint with no push/PR. |
