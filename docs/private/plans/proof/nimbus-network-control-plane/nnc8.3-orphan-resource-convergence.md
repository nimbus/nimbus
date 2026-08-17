# NNC8.3 Orphan Resource Convergence

Status: `complete; K1-K20 pass; the commit containing this proof is the durable checkpoint`

Owner: `NNC8.3`

Starting commit: `26617d18449b5084c6aadf790c88d5c82fd80a07`

Starting tree: `bb577df5eefb8bce258667a1d18824d16b5095a6`

## Outcome

NNC8.3 converts an exact sandbox-owned orphan quarantine into confirmed
provider absence and reusable capacity. It does not create workload desire,
decide that a live workload should stop, or move provider effects into
`nimbus-network`.

The fail-before implementation had the correct first half:

1. The collector enumerated durable desired, provider, allocator, and artifact
   evidence without effects.
2. One pure classifier returned `Adopt` or a named `Quarantine` reason.
3. Startup applied only exact version-fenced and claim-fenced quarantine.

NNC8.3 adds the missing convergence through one sandbox-private seam. It reuses the existing OCI
provider-operation, namespace, port/PEP, IPAM, segment, and portable attachment
state machines. It does not add a second desired-state store or a second
workload saga coordinator.

## Current Ownership Census

| Subject | Canonical authority | Existing behavior | NNC8.3 disposition |
| --- | --- | --- | --- |
| Portable attachment identity, version, phase, association, stable handle | `LocalNetworkAttachmentAuthority` in `nimbus-network` | Exact read, transition, and terminal fencing; no provider effects. | Retain. Use only exact current version/association/handle evidence. |
| Segment reservation, hold, cleanup token, allocation reuse | Injected `OciSegmentAllocator` over the network-owned store | Exact inspect/quarantine/release/finalize exists. | Retain. Cleanup cannot bypass its claim/epoch fences. |
| IPAM and Netavark attempt | `OciIpamAuthority` and `NetavarkProviderOperation` in sandbox | Exact `Reserved` through `Detached` state machine; inspect-before-retry for ambiguous delete. | Retain. It remains the provider-attempt journal. |
| Netavark and namespace effects | `OciAttachmentLifecycle`, `AttachmentHostEffects`, and `netavark.rs` in sandbox | Shared Container/Krun setup and teardown effects. | Retain. NNC8.3 may invoke only the existing sandbox effect port. |
| Port and PEP lifetime | `OciPortLeaseCoordinator`, PEP registry, and portable port leases | Exact lifetime/owner-death recovery and retained cleanup. | Retain. Unknown or live ownership blocks later release. |
| Manifest | Container or Krun backend | Authenticated execution context and provider-local progress. | Evidence and cleanup context only. It is never desired state or network lease authority. |
| Netns/status paths | `OciNetworkLayout` plus descriptor-safe artifact inspection | Untrusted observed artifacts. | Never identity. Remove only under an authenticated provider generation; otherwise quarantine. |
| Startup classification and fencing | `orphan_evidence` plus `startup_reconciliation` | Collect once, classify once, exact quarantine, then fail admission. | Retain as the read/fence stage; add convergence after the fence is durable. |
| Workload stop and cross-domain order | `nimbus-compute` workload saga | Sole coordinator for normal workload teardown. | Unchanged. NNC8.3 cleans only network-local orphan authority that cannot represent a current adoptable generation. |

No current owner drives the last row from durable quarantine through provider
and namespace absence to IPAM/hold/allocation release. The existing normal
`DetachNetwork`/`ReleaseNetwork` path cannot substitute for this gap: it
requires an exact workload execution reference, manifest command state, and
detached proof. An effect-only orphan or a startup-quarantined generation can
lack that complete workload command envelope.

## Current Call Graph And Gap

```text
Container/Krun construction
  -> collect_oci_orphan_evidence
  -> classify_oci_orphan_evidence
  -> exact desired and allocator quarantine
  -> return startup admission error

Normal compute teardown
  -> exact DetachNetwork command
  -> provider + namespace absence, all authority retained
  -> exact ReleaseNetwork command
  -> IPAM + segment + port + portable attachment release

Missing NNC8.3 path
  durable orphan quarantine
  -> exact cleanup eligibility and live-owner fencing
  -> existing sandbox provider cleanup
  -> exact namespace absence
  -> exact IPAM/hold/allocation finalization
  -> terminal portable attachment when one exists
```

## Frozen Architectural Decisions

1. **Quarantine precedes effects.** The convergence owner consumes only an
   already classified, durably fenced subject. It cannot combine observation
   and deletion into one optimistic step.
2. **Absence is not guessed.** One missing record cannot authorize cleanup or
   release. This rule covers manifests, netns paths, status files, process
   records, and provider handles.
3. **A current adoptable generation is never reaped.** Exact `Adopt` remains
   byte-preserving. A live or unknown owner remains fenced for normal compute
   reconciliation.
4. **Sandbox owns provider effects.** The private seam calls those owners.
   `nimbus-network` stays free of provider dependencies.
5. **Manifest is context, not authority.** Container and Krun authenticate an
   exact manifest to recover backend cleanup context. A manifest cannot mint
   attachment, claim, segment, epoch, provider, or cleanup authority.
6. **No manifest means no effectful delete.** Effect-bearing provider evidence
   without authenticated cleanup context remains quarantined. This is a safe
   leak, not a false success. Exact no-effect compensation may proceed only
   when every port, provider-operation, IPAM, and allocator witness proves no
   external effect.
7. **One process-shared effect fence.** Cleanup holds the existing provider
   command/lifecycle authority across effect, inspection, and publication.
   It cannot race a live setup, restart, or normal teardown owner.
8. **Unknown stays `CleanupPending`.** Every uncertain or failed cleanup state
   retains each reusable fence. This rule covers permission failure, corrupt
   evidence, live owners, ambiguous results, replacements, and crossed handles.
9. **Reuse follows the complete order.** Effectful cleanup first proves
   provider and persistent-netns absence. It then settles ports, PEP, IPAM,
   holds, bridge cleanup, and allocation finalization. Portable terminal
   publication occurs last. The load-bearing order is provider effect ->
   persistent netns -> hold -> allocation removal.
10. **Sandbox invents no workload policy.** Exact network authority must reject
    an adoptable current generation before orphan cleanup. All effect owners
    must also authorize cleanup. Normal workload teardown continues to require
    compute-issued commands.

## Frozen Disposition Matrix

| Evidence shape | Required outcome |
| --- | --- |
| Exact current desired + provider + hold + required effects | `Adopt`; zero mutation and zero effect. |
| Exact current desired but incomplete, crossed, or unknown required evidence | Durable `CleanupPending`; no provider delete until the normal owner or exact orphan cleanup fence authorizes it. |
| No desired; provider operation and hold prove no effect; every launch/port lifetime is dead and never bound | Exact reverse-order no-effect compensation, then terminal removal. |
| No desired or exact `CleanupPending`; provider effect present; exact authenticated backend manifest and stopped/lifetime evidence exist | Run existing provider cleanup once, remove exact namespace, then release/finalize authority in order. |
| Effect present but manifest/context is missing, corrupt, crossed, or from another artifact realm | Retain quarantine; no delete and no release. |
| Hold exists without desired or provider effect | Exact no-effect cleanup only after claim, epoch, IPAM, and port authority prove no effect; otherwise quarantine. |
| Manifest exists without durable desired/provider/hold authority | Manifest owner retains it; network reports/quarantines the unmatched artifact and does not delete workload state. |
| Netns/status artifact exists without an authenticated tenant-qualified provider generation | Quarantine the untrusted artifact; path text never becomes identity or delete authority. |
| Provider delete returned an ambiguous result | Inspect the exact provider generation; never repeat delete while effect may remain. |
| Any required inspection is unknown | Remain `CleanupPending` with stable diagnostic and no reusable authority. |
| Exact cleanup is already terminal | Idempotent no-effect replay; no repeated provider or bridge delete. |
| Stale generation/claim/segment/epoch/handle callback after replacement | Reject byte-preserving; replacement remains untouched. |

## Frozen K1-K20 Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | The source-derived census above names every desired, provider-attempt, artifact, manifest, port/PEP, IPAM, hold, cleanup, and workload coordinator owner. |
| K2 | One concept-owned sandbox-private orphan convergence module consumes the existing immutable report/classifications and exact authority adapters; no god provider or duplicate state store appears. |
| K3 | `Adopt` remains byte-preserving and cannot call a provider, remove an artifact, or release authority. |
| K4 | Cleanup eligibility requires an already durable exact quarantine plus complete tenant, attachment, claim, segment, epoch, backend, realm, generation, digest, and provider-handle agreement. |
| K5 | Container and Krun supply backend-owned manifest parsing, lifecycle locking, execution-stop, port/PEP, and provider context through a small substitutable cleanup-context seam. Manifest contents cannot create cleanup authority. |
| K6 | A live setup/restart/teardown owner wins; orphan cleanup returns a named retained fence without provider I/O. A dead owner grants at most one exact cleanup executor. |
| K7 | The no-effect row releases only a complete dead never-bound port plan, terminal/no-effect IPAM attempt, exact hold, and exact portable generation. A missing member prevents every mutation. |
| K8 | The effectful row invokes the existing sandbox Netavark teardown state machine. `Provisioning`/`Deleting` and response-loss cuts inspect before retry; duplicate setup/delete is impossible. |
| K9 | Netns-path existence alone never reports readiness, liveness, ownership, or cleanup success. Descriptor-safe inspection/removal rejects symlink and replacement races. |
| K10 | Provider absence is durable before status/netns removal; netns absence is proven before IPAM/hold release; bridge cleanup is proven before allocation finalization and reuse. |
| K11 | Port, PEP, forwarding, and auxiliary listener authority is settled before IPAM/segment release. Unknown provider-managed authority remains fenced. |
| K12 | Successful cleanup removes or terminally records only the exact orphan generation. A present portable attachment reaches `Released` only after every reusable authority is absent. |
| K13 | Effect-bearing evidence without exact cleanup context, unmatched artifacts, foreign realms, corrupt records, permission failures, and unknown inspection remain `CleanupPending` with deterministic diagnostics. |
| K14 | Stale generation, claim, segment, epoch, provider handle, manifest, and post-replacement callbacks mutate no current authority and perform no effect. |
| K15 | Real-process crash cuts at quarantine, cleanup claim, provider delete intent, provider response, provider-absence publication, status removal, netns removal, IPAM release, hold release, bridge removal, allocation finalization, and portable terminal publication converge or remain fenced. |
| K16 | The full adopt/remove/quarantine matrix covers both Container and Krun, exact replay, sibling isolation, multi-tenant isolation, and allocation reuse exactly once. |
| K17 | Existing compute `DetachNetwork`/`ReleaseNetwork`, normal stop, restart, failed-provision compensation, terminal finality, and startup admission behavior remain green; no second workload coordinator or command-result journal is added. |
| K18 | `nimbus-network -> nimbus-core` remains the only network workspace edge, and static scans prove no provider, manifest, socket, process, filesystem, cluster, policy, name, proxy, or projection effect moved into it. |
| K19 | Focused fail-before/pass-after tests, full affected suites, strict Clippy/Rustdoc, format/diff, dependency/effect/verifier/proof-lint, and docs gates pass with exact counts. |
| K20 | Exactly one GPT-5.6 Sol/xhigh/fast item review runs after K1-K19 are candidate-green. One narrow correction review is allowed only for an accepted executable defect. The exact item is then committed without push or PR. |

## Expected-Red Design

The first behavioral fail-before will use the real startup classifier,
portable attachment authority, IPAM provider-operation journal, allocator, and
substituted sandbox host effects. It will create an exact current generation,
cross a provider-effect boundary, durably quarantine it, and call the new
convergence entry point expected by K2.

Before implementation the test must fail because no such entry point exists.
After implementation it must prove:

1. Delete the provider effect once.
2. Prove status and persistent-netns absence next.
3. Settle port, PEP, and IPAM authority before hold removal.
4. Complete tenant bridge cleanup before allocation finalization.
5. Reuse the old location once under a new stable segment identity.
6. Produce no provider or bridge effect during replay.

Companion expected-red rows will cover a dead never-effected reservation and
a response-loss cut. Retained green controls will cover exact adoption,
missing/crossed manifest context, unknown inspection, and a stale replacement.

## Expected-Red Evidence

The accepted fail-before is the exact ignored Container test:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::container::runtime::tests::lifecycle::provider_cleanup::startup_fencing::nnc8_3_exact_quarantined_orphan_converges_before_capacity_reuse \
  -- --ignored --exact --nocapture
```

It exited `101` after one test: `0/1`, with `1,196` filtered out. The fixture
reached the intended terminal assertion. A fresh
backend retained `startup_reconciliation_error` with the exact diagnostic
`network namespace missing` instead of settling the already durable
quarantine and releasing its authority. This proves the missing convergence
behavior, not a compile or fixture defect.

Rejected harness attempts are not acceptance evidence. One failed to compile
because its visibility and reservation assertion were wrong. One short
`--exact` filter ran zero tests. One fixture failed before startup because the
fixture did not reserve portable desired authority. The accepted behavioral
red includes each correction.

## Initial Owned Paths

The audit first owned this proof and the canonical recovery ledger. Product
ownership started after the expected-red test identified the smallest seam.
The expected product owners were:

- `crates/nimbus-sandbox/src/backends/oci/network/`.
- the narrow Container/Krun manifest cleanup-context adapters.
- their exact tests.

The fail-before harness now owns:

- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/startup_fencing.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/netavark.rs`.

The implementation could add one OCI network child and two narrow backend
adapters. `nimbus-network`, compute, services, proxy, system, cluster, policy,
and public API paths remained closed.

## Audit Checkpoint

- branch: `codex/nimbus-network-architecture-audit`.
- `HEAD`: `26617d18449b5084c6aadf790c88d5c82fd80a07`.
- tree: `bb577df5eefb8bce258667a1d18824d16b5095a6`.
- `origin/main`: `8877eaff43a36d9606a1feaa0ab31d0377539d9d`.
- divergence: `0 behind / 166 ahead`.
- product edits at freeze: test-only setup adapter and one ignored behavioral
  fail-before. Production behavior remained unchanged.
- blocker: none.
- next action at freeze: implement ordered terminal cleanup for the exact safe
  quarantine.

## Implementation Checkpoint 1

The first production slice added one identity-only cleanup subject. It also
added one Container cleanup-context adapter. Startup applied exact quarantine
before the adapter ran. The adapter held the existing Container lifecycle
lock. It authenticated the tenant-qualified manifest and required
`NotSpawned` runtime authority without runtime receipts. The existing
attachment lifecycle retained every effect and state transition.

The original fail-before passed unchanged and passed a second fresh-process
replay: `1/0`, with `1,196` filtered out. The replay closed one necessary
classifier row. Exact portable `Released` state and terminal provider
`Detached` state combine with absent effects and one retained manifest. The
classifier adopts that complete row without mutation. Any incomplete member
still quarantines. `cargo check -p
nimbus-sandbox --all-targets` passes. Krun substitution, no-effect cleanup,
ambiguity/stale/context-negative rows, crash cuts, and full gates remain open.

## Implementation Checkpoint 2

Krun consumed the same identity-only subject through its own manifest adapter.
It held the launch lifecycle lock and authenticated the tenant-qualified
manifest. It also proved `NotSpawned` creator state without runtime receipts.
The same OCI attachment lifecycle retained every cleanup transition. The
test-only setup helper now names the shared host-managed capability.

The Container and Krun effectful-orphan paths passed `2/0`. Their second
fresh-process replays also passed. The Krun command reported `1/0`, with
`1,197` filtered. `cargo check -p nimbus-sandbox --all-targets` passed. K7-K16
remained open at this checkpoint.

## Candidate Checkpoint

The shared compiler selects three closed work kinds: dead never-effected
compensation, effectful orphan cleanup, and terminal manifest publication.
Container and Krun authenticate the same identity-only subject under their
existing lifecycle locks. They then use the existing cleanup state machines.

The compiler reauthenticates the tenant, attachment, selected provider,
provider handle, provider attempt, claim, segment, and both allocator evidence
sources. A crossed same-provider handle therefore cannot become cleanup
authority. Both adapters also require an open workload-teardown admission gate
and an untouched stop phase. This condition prevents startup cleanup from
racing the normal durable workload owner.

| Acceptance | Result | Evidence |
| --- | --- | --- |
| K1-K5 ownership and seam | `pass` | One identity compiler and one cleanup-context trait sit below two backend adapters. `nimbus-network` is unchanged. |
| K6 live-owner fencing | `pass` | Both adapters reject active setup, restart, or teardown ownership. The existing Krun two-process teardown contender passes `1/0`. |
| K7-K14 cleanup order and fencing | `pass` | The corrected NNC8.3 set passes `9`, with one subprocess child ignored by design. It proves durable `Deleting` recovery, exact present-provider cleanup, terminal-provider release continuation, publication-before-IPAM-retirement, crossed handles, stale claims, corrupt context, response loss, auxiliary lifetime, terminal publication, and exact replay. |
| K15 process and crash proof | `pass` | Container and Krun retain their 22 durable teardown checkpoints. Shared attachment crash cuts, Netavark response loss, the two-cut bridge/allocation subprocess proof, and the two no-effect publication-cut tests pass. The subprocess child is ignored in the normal suite, fails closed without its environment, and runs only through its exact parent with `--ignored`. |
| K16 isolation and reuse | `pass` | Both backends prove replay, sibling isolation, tenant-qualified identity, and reuse only after terminal cleanup. The multi-tenant verifier passes `16/16`. |
| K17 affected behavior | `pass` | Sandbox passes `1,175` with `31` ignored. Compute passes `476` with one ignored. CLI passes `1,008` with four ignored. The canonical serialized server suite passes `659` with `35` ignored. The sandbox Cargo process passed; its outer zsh logger then failed on the reserved variable name `status`, so the green Cargo summary in `/tmp/nnc83-sandbox.log` is the acceptance evidence and the suite was not repeated. |
| K18 architecture | `pass` | The live verifier passes `38/38`. The pre-narrow aggregate passes `588/588`. After the narrow correction, the changed NNCV018 condition passes its base case and all `17/17` owned mutations. `nimbus-network -> nimbus-core` remains the only network workspace edge. The generic startup owner has no direct provider effect. |
| K19 quality and docs | `pass` | After the final executable correction, focused NNC8.3 behavior, sandbox check, strict Clippy, and warning-denied Rustdoc pass. Rustfmt, diff, Prettier, Node/Bash syntax, multi-tenant `16/16`, docs `108`, site `17/17`, and strict proof lint pass. |
| K20 review and checkpoint | `pass` | The sole full GPT-5.6 Sol/xhigh/fast review accepted six findings. The sole narrow review accepted one P2. All seven findings are corrected and proven. The cadence permits no third review. The commit containing this proof and the completed ledger row is the exact item checkpoint. |

The first full sandbox run found three failures. The generic artifact fence
changed two expected diagnostics for corrupt manifests. The third exposed a
real race with normal durable workload teardown.
The accepted correction added the exact workload-owner gate to both backend
adapters. The full sandbox suite and the existing two-process contender then
passed. No other acceptance defect remains open.

The multi-tenant verifier first reported two stale source paths. Its corrected
contract recognizes the reaper-owned hold lifecycle and both new backend
cleanup adapters. It also rejects direct Netavark or detach capability in the
generic startup owner. The corrected gate passes `16/16`.

## Full Review And Correction Dispositions

The sole full item review used GPT-5.6 Sol with xhigh reasoning and fast mode.
It reported six findings and classified the candidate as incorrect at
confidence `0.98`. TruffleHog reported no secrets. We accepted all six findings
because they map directly to K8, K15, or the recovery contract.

| Finding | Disposition | Correction and proof |
| --- | --- | --- |
| P1: durable `Deleting` attempts did not resume | `accepted; corrected` | The effectful compiler accepts the exact durable delete states and routes them through inspect-before-retry. The Container response-loss test proves one delete and exact replay. |
| P1: provider-terminal rows could not finish allocator release | `accepted; corrected` | The compiler accepts terminal provider evidence with retained exact allocator authority. The classifier regression and both adapters prove reverse-order completion. |
| P1: present exact namespace/provider artifacts could not be removed | `accepted; corrected` | Exact present or absent provider artifacts are authenticated inputs. The Krun proof starts with a present namespace and status artifact and proves their removal through the existing provider owner. |
| P2: no-effect release could crash before terminal manifest publication | `accepted; corrected` | The reaper publishes terminal backend evidence after reusable authority is final but before terminal IPAM retry evidence retires. Container and Krun publication-cut tests prove fresh-start repair and replay. |
| P3: the crash child passed as an empty normal test | `accepted; corrected` | The child is ignored, requires its environment with `expect`, and is invoked only by the parent with `--ignored`. The static mutation removes this protection and fails closed. |
| P3: the recovery header routed back to completed work | `accepted; corrected` | The recovery header now identifies correction verification, the one narrow review, and the exact item commit as the only remaining actions. |

The strengthened NNCV018 contract adds seven fail-closed mutations:

1. `missing-deleting-resume`.
2. `missing-terminal-resume`.
3. `absent-only-effectful-artifacts`.
4. `terminal-after-effectful`.
5. `retire-before-publication`.
6. `missing-publication-cut-proof`.
7. `no-op-crash-child`.

Each direct mutation fails as specified. The complete aggregate passes
`588/588`.

## Exact Owned Paths

The corrected candidate owns 27 paths:

- `crates/nimbus-sandbox/src/backends/container/runtime.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/planning.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/startup_orphan_convergence.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/startup_fencing.rs`.
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provision_phases.rs`.
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`.
- `crates/nimbus-sandbox/src/backends/krun/vm/startup_orphan_convergence.rs`.
- `crates/nimbus-sandbox/src/backends/krun/vm/tests/startup_fencing.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/recovery.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/netavark.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/orphan_convergence.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence/classifier.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence/classifier/tests.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/reaper.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/reaper/tests.rs`.
- `crates/nimbus-sandbox/src/backends/oci/network/startup_reconciliation.rs`.
- `docs/private/plans/nimbus-network-control-plane-plan.md`.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json`.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc8.3-orphan-resource-convergence.md`.
- `scripts/nimbus-network-control-plane/startup-orphan-reconciliation-contract.sh`.
- `scripts/verify-multi-tenant-network.sh`.
- `scripts/verify-nimbus-network-control-plane.sh`.
- `scripts/verify-nimbus-network-startup-orphan-reconciliation.mjs`.

No upper crate, `nimbus-network`, dependency manifest, provider policy,
service naming, proxy, system, or cluster path changed.

The sole narrow GPT-5.6 Sol/xhigh/fast review reported one P2 at confidence
`0.94`. Container published repaired no-effect manifest finality but retained
the terminal IPAM retry witness until a second restart. The finding is valid.

An added same-process assertion failed `0/1` at the intended terminal witness.
The correction retires no-desired Container and Krun witnesses immediately
after durable publication. The exact backend pair passes `2/2`. Desired
portable generations retain the previous full-terminal-finality rule.

We used the full review and one narrow review. The cadence permits no third
review.

## Final Acceptance Evidence

| Gate | Final evidence |
| --- | --- |
| Narrow defect fail-before | The Container same-process IPAM-retirement assertion failed `0/1` at the retained terminal witness, with `1,205` filtered tests. |
| Narrow defect correction | The Container and Krun publication-cut pair passes `2/2`, with `1,204` filtered tests. Desired portable generations retain their full-terminal-finality fence. |
| Focused behavior | `cargo test -p nimbus-sandbox --lib nnc8_3_ -- --nocapture` passes `9`, ignores the one child-only subprocess entry point, and filters `1,196`. |
| Full affected behavior | Sandbox passes `1,175 + 31 ignored`; compute passes `476 + 1`; CLI passes `1,008 + 4`; serialized server passes `659 + 35`. The narrow correction changes only sandbox-owned startup finalization and its tests. |
| Strict affected quality | Sandbox all-target/all-feature check and strict no-dependency Clippy pass. Warning-denied sandbox Rustdoc passes. Only unchanged vendored Brotli warnings remain outside the strict owner. |
| Static architecture | The live verifier passes `38/38`. NNCV018 passes directly and all `17/17` owned mutations fail closed. The earlier complete aggregate passes `588/588`; the final correction reruns only its changed condition. |
| Documentation and format | Multi-tenant passes `16/16`; docs pass `108`; site passes `17/17`; Rustfmt, Prettier, Node/Bash syntax, diff, and strict proof lint pass. |
| Review | Full Sol/xhigh/fast: six accepted findings, confidence `0.98`. Narrow Sol/xhigh/fast: one accepted P2, confidence `0.94`. TruffleHog reported no secrets. No third review ran. |

The narrow-review input used staged tree
`181841e104501c3adfe5b9a38de860309c8396f1` and binary patch SHA-256
`075e4c6cbcdc34f758ad96b3e5e48d703645ae02b383de0675387e46a8109c7a`.
The final executable/static candidate before this ledger closeout used tree
`143b635dfd05ab80c23d13f1a7dcf6240d601219` and binary patch SHA-256
`94476031780bb60d3d7f241837f36be33eb54d43da429c4317defa3574205103`.
It contains 27 paths, including 19 Rust paths, with 3,408 insertions and 187
deletions. The item commit is the final self-authenticating identity.
