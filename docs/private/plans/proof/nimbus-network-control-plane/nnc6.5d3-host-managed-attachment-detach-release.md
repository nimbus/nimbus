# NNC6.5d3 Host-Managed Attachment Detach And Release

Status: `complete; K1-K29 green; item commit contains this checkpoint`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5d3 separates exact host-managed Container and Krun attachment detach
from final authority release. It adds both real compute attachment capability
adapters. Detach proves provider and namespace absence while it retains every
reusable authority. Release requires that compound proof and is the only new
path that can free the retained port, PEP, IPAM, segment, and attachment
authority.

This item does not stop a runtime. It does not add a Machine API request,
forwarded-machine behavior, parent publication behavior, caller cutover, or
portable network phase. It does not delete the coarse cleanup path. It does
not change service naming, tenant policy, proxy forwarding, cluster transport,
or `nimbus-network` dependencies or effects.

The read-only audit ran at
`15872e4192a0c66865a3aadff30ca79d8f9a08e3` from a clean owner worktree. No
product-source file changed during the audit or fail-before capture.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | The read-only source audit names the current Container, Krun, shared OCI attachment, listener, PEP, IPAM, segment, portable attachment, provider-journal, compute-command, and capability-registry authorities. |
| K2 | One sandbox-owned, workload-neutral network teardown command carries tenant, sandbox, execution attempt, attachment, exact compiled-plan identity, operation, provider key, and the existing complete provider claim. It introduces no `nimbus-workloads` dependency in `nimbus-sandbox`. |
| K3 | Compute lowers the retained `WorkloadExecutionReference` and network subject into that command. It authenticates provider role and ID, node, tenant, workload generation, desired digest, source digest, network-plan digest, selection evidence, execution locator, attachment identity, command attempt, and dispatch epoch before provider mutation or effect. |
| K4 | `ProviderCommandAttemptJournal` remains the only sandbox provider-dispatch journal. `DetachNetwork` and `ReleaseNetwork` have independent exact claims, epochs, durable results, and compute result CAS operations. No backend manifest becomes a second command-result journal. |
| K5 | One real Container adapter and one real Krun adapter implement both `NetworkDetachmentCapability` and `NetworkReleaseCapability` for their admitted host-managed attachment provider IDs. Registry selection is exact and has no fallback or no-op provider. |
| K6 | Each backend locates one exact manifest from the retained execution reference and tenant-qualified artifact path; it does not scan tenant roots by sandbox ID. It authenticates tenant, sandbox, execution attempt, generation, desired/source/plan evidence, attachment association, selected provider, and stable provider handle before journal mutation or effect. A missing file is never provider absence. |
| K7 | Each backend uses one required, strict manifest substate for independent detach and release progress. Missing, unknown, crossed, or corrupt fields fail closed. Execution teardown and network teardown remain different concept-owned states. |
| K8 | Detach requires exact durable `ExecutionStopped` evidence for the same workload generation. It never signals, stops, or deletes a runtime and cannot infer terminality from status text, a PID path, or missing process state. |
| K9 | Execute takes the provider stream lock before the exclusive backend lifecycle lock. Inspect synchronizes with the provider stream and uses the shared lifecycle lock. Every path re-reads and reauthenticates the manifest after lock acquisition. No path takes these locks in reverse order. |
| K10 | Detach moves the portable attachment to `Deleting` and durably quarantines the exact segment before the first PEP, listener, provider-delete, or namespace-remove effect. `AttachmentTeardownMode::Restart` stays a same-generation rebind path and is not detach success. |
| K11 | PEP stop has a durable may-exist or withdrawal boundary before provider shutdown. Exact shutdown and trust-anchor absence settle the PEP listener into retained, non-bindable authority. A fresh process can inspect or recover the exact retained state without treating an empty process registry as proof. |
| K12 | Published listener cleanup uses exact lifetime or owner-death recovery authority. Provider absence settles every listener into retained, non-bindable state. Detach releases no host port and no other workload can reserve one of those ports. |
| K13 | Netavark delete intent is durable before provider deletion. Ambiguous delete requires inspect-before-retry. Success requires the exact provider attempt and status projection to be absent; no command return alone proves absence. |
| K14 | Namespace removal has an explicit may-exist boundary and exact filesystem inspection. Success requires explicit absence of the exact persistent namespace. A missing or unreadable parent, wrong artifact type, permission error, or crossed path is ambiguous, not absent. |
| K15 | Detach persists one compound proof bound to tenant, sandbox, execution attempt, attachment, plan, generation, lease epoch, association, selected provider, stable handle, provider-delete evidence, namespace absence, retained PEP/listener evidence, quarantined segment evidence, retained IPAM/segment/port/PEP/attachment authority, and the exact detach claim. |
| K16 | Detach succeeds to compute only when the compound proof and exact `DetachNetwork` provider-journal success both exist. The portable attachment remains `Deleting`; IPAM, segment, port, PEP, and attachment authority remain retained and fenced. |
| K17 | Release reauthenticates the complete current command, exact manifest, compound detached proof, exact prior `DetachNetwork` journal success, provider absence, namespace absence, retained listener/PEP state, live IPAM, quarantined segment, and the exact nonterminal attachment state before its first release effect. The normal state is `Deleting`. Startup-quarantined `CleanupPending` is valid only with the same complete immutable detached proof and all retained authority. |
| K18 | Release performs no runtime stop, provider delete, namespace remove, listener bind, PEP start, or attachment reprovision effect. Unknown, active, or crossed evidence preserves every retained authority and requires inspection. |
| K19 | Release settles retained PEP and published-listener authority first, then IPAM, then the segment hold, and finally the portable attachment. Every completed step is durable and idempotent before the next step starts. |
| K20 | Only release transitions the portable attachment to `Released`. The manifest publishes release completion only after all reusable authority is absent. Terminal IPAM retry evidence retires only under the existing finality owner. |
| K21 | Execute maps definite invalid, crossed, stale, skipped, and ordering failures to their frozen stable codes. Missing or corrupt authority and ambiguous effects remain `Ambiguous`. Inspect is byte-stable and returns `NotCompleted` only when exact evidence proves that no older operation can complete. |
| K22 | Exact duplicate commands replay without a new effect. Adjacent authorized retries rebase provider-local progress to the current fence. Stale callbacks cannot publish a result or mutate a successor. |
| K23 | Two thread contenders, two process contenders, and a concurrent Inspect produce one phase-local effect/result winner. Inspect never reports `NotCompleted` while an older exact effect is live or can still publish. |
| K24 | Fresh-process recovery passes every frozen DetachNetwork and ReleaseNetwork cut with only the durable workload store, provider journal, manifest, provider state, and lease roots. No in-memory snapshot is recovery authority. |
| K25 | Both real compute substitutions execute and inspect detach and release through reopened Container and Krun backends. Every callback matches the complete confirmed command and returns the correct network success evidence. |
| K26 | The 1,522-line shared lifecycle composition root moves host-managed detach/release behavior into concept-owned children and falls below the explicit-reason threshold. New Container, Krun, sandbox-contract, and compute roots stay thin; test-heavy modules remain concept-owned. |
| K27 | Legacy coarse stop, launch-failure compensation, restart detach, forwarded-machine cleanup, and product caller routes remain behaviorally unchanged for later owners NNC6.5d4-NNC6.5g. No compatibility decoder, feature flag, speculative provider interface, or portable phase is added. |
| K28 | Focused contract, shared lifecycle, PEP/listener, Container, Krun, compute substitution, schema, replay, contention, and crash tests pass. Full affected crates, strict Clippy, warning-denied rustdoc, format, dependency/effect scans, modularity census, NNCV035 arithmetic, proof lint, docs, and site gates pass with exact counts. |
| K29 | Exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast item review runs only after K1-K28 are green. Only an accepted material executable finding permits one narrow correction review. |

## Current Ownership And Call Graphs

### Shared host-managed attachment

```text
Container release_execution_artifacts(Final)
Krun release_network_artifacts(Final)
  -> OciAttachmentAdapter::detach_host_managed(Final)
     -> authenticate exact IPAM generation and allocator association
     -> inspect portable attachment and Netavark state
     -> portable attachment -> Deleting
     -> caller callback deletes runtime when Container and stops PEP
     -> quarantine segment
     -> claim listener cleanup authority
     -> Netavark delete
     -> remove persistent namespace
     -> settle or release listeners
     -> release PEP/never-bound listeners
     -> release IPAM
     -> release segment
     -> portable attachment -> Released
```

`attachment_lifecycle.rs` owns this combined algorithm. It has 1,522
handwritten lines. `detach_release.rs` owns only the final 78-line IPAM and
segment release tail. `recovery.rs` moves `Deleting` directly to `Released` for `Final`, while
`Restart` returns to `Provisioning`. There is no durable detached proof and no
release-only entry point.

The current callback is also too broad for the exact capability: Container
deletes the runtime inside host-managed attachment detach. Exact execution
stop now owns runtime terminality and must be a prerequisite, not a repeated
network effect. Krun's callback stops the PEP but has no independent exact
network command or manifest progress.

### Container

```text
ContainerSandboxBackend::stop / runner cleanup / launch compensation
  -> release_execution_artifacts
     -> authenticate creator and manifest
     -> remove runner pointer
     -> host-managed: detach_host_managed(Final)
        -> delete conmon runtime and stop PEP
        -> release all network authority
     -> remove launch artifacts
     -> network_cleanup_complete = true
```

The Container manifest has strict execution drain/stop progress but only one
coarse `network_cleanup_complete` finality bit. It has no detach fence,
compound proof, or release progress. The existing exact execution stop leaves
the runtime terminal and every network byte stable, which is the required
prerequisite for the new network owner.

### Krun

```text
Krun coarse stop / provider-failure cleanup
  -> release_network_artifacts(Final)
     -> authenticate creator and launch authority
     -> detach_host_managed(Final)
        -> stop PEP
        -> release all network authority
     -> launch_authority = Released
```

The Krun manifest has strict execution drain/stop progress and a coarse
`KrunLaunchAuthority`, but no independent detach proof or release command
progress. The new path must not reuse provider-failure cleanup or coarse stop
as its result authority.

### Compute and provider journal

Compute already owns exact `NetworkDetachmentCapability` and
`NetworkReleaseCapability` traits, separate `DetachNetwork` and
`ReleaseNetwork` saga steps, exact confirmed commands, callback fences, and
result CAS operations. `ProviderCommandOperation` already contains both
network operations. The real registry has Container and Krun execution
adapters only. No real host-managed attachment adapter exists.

`ConfirmedWorkloadTeardownCommand` already retains both the portable network
subject and the exact `WorkloadExecutionReference`. That is sufficient to keep
the portable subject provider-neutral while the sandbox adapter locates and
authenticates one manifest.

## Target Ownership And Call Graph

```text
confirmed DetachNetwork
  -> exact Container or Krun attachment adapter
  -> one existing backend ProviderCommandAttemptJournal
  -> exclusive backend lifecycle lock
  -> reauthenticate manifest and exact ExecutionStopped proof
  -> shared host-managed detach owner
     -> attachment Deleting -> segment quarantined
     -> PEP/listeners stopped and retained
     -> provider delete -> exact absence
     -> namespace remove -> exact absence
     -> persist compound detached proof
  -> exact DetachNetwork provider result
  -> compute result CAS -> NetworkDetached

confirmed ReleaseNetwork
  -> same exact backend attachment adapter
  -> independent ReleaseNetwork claim in the same journal
  -> reauthenticate manifest + compound proof + prior detach success
  -> shared host-managed release owner
     -> release retained PEP/listeners
     -> release IPAM
     -> release segment
     -> attachment Released
     -> persist release completion
  -> exact ReleaseNetwork provider result
  -> compute result CAS -> NetworkReleased
```

The portable `NetworkResourcePhase` remains `Deleting` between the two
commands. The backend manifest holds provider-local progress and compound
evidence. The provider journal holds command identity and result. Compute
alone owns lifecycle order and result CAS. These authorities do not overlap.

## Frozen Path Ownership

Primary product ownership for this item is:

- `crates/nimbus-sandbox/src/teardown.rs` or one concept-owned sibling for the
  neutral network teardown command and observation, plus narrow exports and
  contract tests.
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
  and concept-owned host detach, release, state, recovery, and tests.
- narrow `crates/nimbus-sandbox/src/backends/oci/port_lifecycle/` and
  `crates/nimbus-sandbox/src/backends/oci/egress/` retained-stop/release
  changes and attributed tests.
- narrow OCI IPAM and segment inspection/release changes only when the
  compound proof needs an existing authority to expose exact observation.
- `crates/nimbus-sandbox/src/backends/container/runtime/attachment_teardown.rs`,
  strict manifest state, mechanical module registration, and tests.
- `crates/nimbus-sandbox/src/backends/krun/vm/attachment_teardown.rs`, strict
  manifest state, mechanical module registration, and tests.
- `crates/nimbus-compute/src/workload_saga/teardown_sandbox/attachment.rs`,
  shared lowering/result reuse, narrow exports, and tests.
- this proof, the canonical plan/status ledger, routing index, and only the
  source-derived static-verifier coordinates required by owned new paths.

Forbidden paths and seams include runtime signal/stop effects, Machine API
transport, forwarded-machine behavior, and parent publication authority. They
also include services, Compose, definition callers, tenant policy, proxy
forwarding, and cluster transport. The item cannot add coarse cleanup deletion,
a second journal, a second saga coordinator, `nimbus-network` provider effects,
or a new `nimbus-network` workspace dependency.

## Complexity And Pattern Decisions

1. `attachment_lifecycle.rs` is above the 1,500-line explicit-reason threshold
   because the host-managed detach state machine remains inline. The item
   moves that state machine into a concept-owned child. The root keeps sealed
   adapter construction and composition only.
2. Detach and release are two ports-and-adapters capabilities over one shared
   attachment state machine. They are not two provider implementations and do
   not justify a god `NetworkProvider` trait.
3. The manifest records effect progress. The existing journal records command
   progress. A compound proof binds the two without copying the journal result
   into a second result authority.
4. PEP and published-listener teardown reuse their exact lease/lifetime
   authorities. Detach ends in a stopped, retained state. Release consumes
   that state. Socket bindability is never evidence.
5. Netavark and namespace effects remain sandbox-owned. The shared lifecycle
   injects small host-effect ports for deterministic tests. No provider binary
   or filesystem effect enters `nimbus-network`.
6. Inspection is a synchronized query, not reconciliation. It may classify a
   durable may-exist record, but it cannot write, stop, remove, release, or
   repair.
7. The current Container and Krun `read_manifest(SandboxId)` helpers scan tenant
   roots. Exact attachment commands derive the tenant-qualified
   manifest path from the authenticated command, then reauthenticate the
   manifest after the lifecycle lock. Equal local sandbox IDs in different
   tenants can never select each other.
8. The existing provider journal has independent streams for `DetachNetwork`
   and `ReleaseNetwork`. The item does not grow its 1,510-line root unless a
   evidence proves a generic correctness defect. Port and egress changes stay in
   existing concept-owned child modules rather than growing their 1,589-line
   and 1,703-line roots.

## Fail-Before Baseline

All checks ran against the clean source base before a product edit. Each
command exited `1`, as required.

| Check | Expected-red result |
| --- | --- |
| `rg -q SandboxNetworkTeardownCommand crates/nimbus-sandbox/src` | No neutral sandbox network teardown command exists. |
| `test -f crates/nimbus-compute/src/workload_saga/teardown_sandbox/attachment.rs` | No real compute attachment adapter module exists. |
| `test -f crates/nimbus-sandbox/src/backends/container/runtime/attachment_teardown.rs` | Container has no exact attachment command state machine. |
| `test -f crates/nimbus-sandbox/src/backends/krun/vm/attachment_teardown.rs` | Krun has no exact attachment command state machine. |
| `rg -q 'DetachedNetworkProof\|NetworkDetachedProof\|DetachedAttachmentProof' crates/nimbus-sandbox/src` | No durable compound detached proof exists. |
| `rg -q ContainerNetworkTeardownAdapter crates/nimbus-compute/src` | Container does not implement either real attachment capability. |
| `rg -q KrunNetworkTeardownAdapter crates/nimbus-compute/src` | Krun does not implement either real attachment capability. |
| `rg -q 'detach_host_managed_retaining\|detach_host_managed_retained' crates/nimbus-sandbox/src/backends/oci/network` | Shared lifecycle has no detach-with-retained-authority operation. |
| `rg -q release_host_managed_detached crates/nimbus-sandbox/src/backends/oci/network` | Shared lifecycle has no release-only operation. |

Source inspection is the behavioral fail-before proof: five production
`AttachmentTeardownMode::Final` entry points still combine provider detach and
authority release. The focused implementation tests will first encode the
red observable boundaries below before the behavior changes.

## Frozen Failure Roster

| Case | Required result and proof |
| --- | --- |
| Wrong step, subject, or provider role | `DefiniteFailure(sandbox_teardown_command_invalid)` before manifest lookup, journal claim, or effect. |
| Crossed provider, tenant, sandbox, execution attempt, attachment, node, plan, generation, desired/source/selection/target evidence | `DefiniteFailure(sandbox_teardown_command_crossed)` with every durable byte and effect unchanged. |
| Stale generation or epoch | `DefiniteFailure(sandbox_teardown_command_stale)` and byte-stable successor state. |
| Skipped epoch or crossed command/transition | `DefiniteFailure(sandbox_teardown_epoch_invalid)` with no fallback. |
| Detach before exact execution stop | `DefiniteFailure(sandbox_teardown_order_invalid)` and no network mutation. |
| Release without matching compound proof or detach-journal success | `DefiniteFailure(sandbox_teardown_order_invalid)` and no authority release. |
| Missing manifest or association | `Ambiguous` unless the exact current provider journal already has a terminal replay; missing files never prove absence. |
| Corrupt manifest, journal, provider status, namespace artifact, association, lease, or proof | `Ambiguous`; preserve evidence for diagnosis and do not repair by guess. |
| Unknown or active PEP/listener/provider/namespace/IPAM/segment state | Execute is `Ambiguous`; Inspect is `InProgress` only with exact live-owner evidence; retain every authority. |
| Exact duplicate | Replay the current result with no effect. |
| Live older attempt | Inspect returns `InProgress` or `Ambiguous`, never `NotCompleted`; no later effect overlaps it. |
| Stale callback after successor | Reject the callback; preserve successor and stale evidence. |
| Address or port used as identity | `DefiniteFailure(sandbox_teardown_identity_invalid)` before lookup. |

## Crash, Restart, And Concurrency Matrix

Every capability has its own workload claim, confirmed command,
provider-journal claim, provider result, and compute result CAS. A new process
receives no in-memory snapshot.

Detach has the nine frozen host-managed cuts for Container and Krun
independently:

1. `Deleting` and segment quarantine are durable.
2. Local listener and PEP stop intent is durable before each stop effect.
3. Listener and PEP retained-state settlement is durable.
4. Provider-detach-may-exist evidence is durable before provider deletion.
5. Provider deletion returns or loses its response.
6. Exact provider absence is durable before namespace removal.
7. Namespace-removal-may-exist evidence is durable before removal.
8. Explicit namespace absence and the compound detached proof are durable.
9. every IPAM, segment, port, PEP, listener, and attachment authority remains
   retained and fenced.

Release has the six frozen host-managed cuts for Container and Krun
independently:

1. The adapter reauthenticates the full compound detached proof before release
   intent.
2. Listener and PEP final-release intent is durable before each release.
3. Exact listener and PEP release is durable before IPAM release intent.
4. IPAM-release-may-exist evidence is durable before the IPAM effect.
5. Segment-release-may-exist evidence is durable before the segment effect.
6. All reusable authority is absent before the attachment becomes `Released`.

Both phase-local matrices run inside the frozen outer matrix. It covers the
workload claim, provider claim, effect progress, and lost response or process
death. It also covers fresh-process inspect-before-retry and the provider
result. The compute result must be durable before its CAS. The next capability
stays unclaimed until the current CAS is durable.

For each phase, two synchronized thread contenders, two subprocess contenders,
and one Inspect contender prove one effect/result winner and no premature
reuse. Each phase reopens the port, IPAM, segment, PEP, and attachment
authorities from disk. This step proves retained or released state directly.

## Implementation Bands

These are dependency-ordered work bands inside one canonical item. They do not
create additional review units.

1. Add the neutral sandbox network command/observation and exact compute
   lowering tests.
2. Add strict shared detached-proof vocabulary and split the shared OCI
   detach/release algorithm into concept-owned children.
3. Make PEP and published-listener retained-stop and final-release behavior
   explicit, durable, and recoverable.
4. Add the exact Container network state machine, replay, contention, crash,
   and no-reuse proofs.
5. Add the exact Krun network state machine with the same contract.
6. Add both real compute capability adapters and cross-backend substitution
   proofs.
7. Run the complete item gates and freeze the candidate. Run one full item
   review. Resolve accepted findings and commit the exact item.

Only this item remains `in_progress`. The canonical ledger cannot mark partial
bands complete, and partial bands cannot trigger structured autoreview.

## Acceptance Commands

The evidence ledger records the exact final counts.

```sh
cargo test -p nimbus-sandbox network_teardown_contract
cargo test -p nimbus-sandbox attachment_lifecycle -- --test-threads=1
cargo test -p nimbus-sandbox egress -- --test-threads=1
cargo test -p nimbus-sandbox container_network_teardown -- --test-threads=1
cargo test -p nimbus-sandbox krun_network_teardown -- --test-threads=1
cargo test -p nimbus-sandbox fresh_process_network_teardown -- --test-threads=1
cargo test -p nimbus-compute teardown_sandbox -- --test-threads=1
cargo test -p nimbus-sandbox
cargo test -p nimbus-compute
cargo clippy -p nimbus-sandbox -p nimbus-compute --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-sandbox -p nimbus-compute --no-deps
cargo fmt --all --check
git diff --check
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Closeout also runs the network dependency/effect scan, changed-file modularity
census, NNCV035 self-test/direct/aggregate arithmetic, ledger bijection check,
technical-writing lint, and proof lint. Structured autoreview is forbidden
until every written criterion is green and the complete item is
candidate-frozen.

## Retained Later Owners And Non-Goals

- NNC6.5d4 owns forwarded-machine provider adapters and guest phase envelopes.
- NNC6.5e owns native service, sandbox, and definition caller cutover.
- NNC6.5f owns Compose, forwarded composition, and physical-machine caller
  cutover.
- NNC6.5g owns legacy coarse-cleanup deletion, failed-provision convergence,
  and final NNCV035 convergence.
- NNC6.6 owns service-resolution fencing.
- NNC6.1e2 owns final startup and tenant-retirement convergence.
- NNC8.3 owns orphan cleanup finalization and capacity-reuse convergence.

The item does not add a portable `Detached` phase or service name resolver. It
does not add a tenant admission rule, TLS certificate owner, forwarding
provider, or DNS/xDS provider. It also excludes cloud SDKs, cluster transport,
and IP-address identity.

## Evidence Ledger

| Evidence | Result |
| --- | --- |
| Source base and worktree | `15872e4192a0c66865a3aadff30ca79d8f9a08e3`; clean before audit. |
| Read-only source census | Complete for shared OCI lifecycle, Container, Krun, compute registry/commands, provider journal, PEP, listeners, IPAM, segment, and manifests. |
| Complexity census | Shared lifecycle `1,522`; provider journal `1,510`; port lifecycle root `1,589`; egress root `1,703`; port lifecycle tests root `1,923`; lifecycle tests root `1,490`; Container cleanup `288`; Krun lifecycle `1,377`; Krun launch-compensation tests `1,709`; compute sandbox adapter root `500`. New behavior belongs in concept-owned children. |
| Fail-before | Nine narrow checks exited `1`; product source unchanged. |
| Parallel audit packets | Container, Krun, and shared compute/attachment audits complete. All three inspected the same source base, ran no product tests, and reported zero changed or staged paths. |
| Static/docs audit gates | Format and diff pass. Proof lint passes with three advisory passive-voice warnings. NNCV035 self-test is `55/55`; direct is the expected `0/7`. The aggregate verifier is `35/36`, with only NNCV035 red. Docs pass `108`; site passes `17/17`. |
| Band 1 command and lowering | Fail-before compilation exited `101` for missing attachment command/lowering. The neutral command now binds its typed identity to a canonical effect subject and provider-target digest. Sandbox and compute focused tests pass `1/1` each. K2-K3 are green. |
| Shared lifecycle modularity | Existing host-managed teardown moved from the `1,522`-line root into the `388`-line `attachment_lifecycle/host_teardown.rs` owner. Exact retained detach, final release, and strict progress state live in `535`, `371`, and `724`-line concept children. The root is `1,165` lines. |
| Exact provider behavior | Container and Krun each use one strict manifest substate, one shared provider journal, one exact tenant-qualified manifest preflight, and independent Detach/Release claims. The combined `network_teardown` lane passes `15` with two declared subprocess ignores. |
| Fresh-process and contention proof | Each backend passes all 11 Detach and all 11 Release crash cuts: 44 writer deaths and 44 recovery processes in total. Thread and process contenders publish one result. Concurrent Inspect cannot report `NotCompleted` while an exact claim can still finish. Both fresh-process parents pass `2/2`. |
| Focused acceptance | Sandbox network command `1`; shared attachment lifecycle `81 + 5` declared ignores; egress/PEP `92 + 4` declared ignores across unit and platform lanes; Container exact network `1`; Krun exact network `2`; compute teardown adapter `23`; real cross-backend substitution `8`. Review-correction regressions pass: one authenticated batch snapshot, 12 K14 filesystem cases, and one generic plus one Container plus one Krun semantic lock-contention case. |
| Full affected behavior | `nimbus-network` passes `273` with one declared child-process ignore. The final default-concurrency sandbox run passes `1,117` with `47` declared platform/subprocess ignores across all crate targets. Compute passes `369` with one declared child-process ignore. |
| Quality checkpoint | Strict all-target/all-feature Clippy and warning-denied rustdoc pass for network, sandbox, and compute. Format and diff checks pass. Only unchanged vendored Brotli warnings appear outside the denied workspace lints. |
| Dependency and effect boundary | Cargo metadata/tree and NNCV004 prove that `nimbus-core` is the only workspace edge from `nimbus-network`. NNCV006, NNCV012, NNCV015, NNCV017, and NNCV022-NNCV024 pass after source-derived test-fixture, line, split-owner, and exact-inspection updates. The test-only socket reservation remains in the compute test module; sandbox test hooks receive only the held port and release callback. |
| Static arithmetic | NNCV035 self-test is exact `55/55`. Its direct expected-red result is exact `0/7`. Aggregate verification is exact `35/36`, with NNCV035 as the only red condition. NNCV008 confirms one recoverable active ledger row and the band/ledger bijection. |
| Read-only audit dispositions | Four backend/compute gaps were accepted and corrected: pre-journal exact-manifest preflight, live-claim inspection, stream-to-lifecycle lock order, and the portable `Deleting` gate. Legacy Krun attempt matching and restart authority were restored. Contention adopters now wait read-only for the sole winner. No audit packet changed a path. |
| Full structured review | The one complete-item GPT-5.6 Sol/xhigh/fast review ran against staged tree `c15a7b073d581f2c4be4f28349d7edb6d1e2b927`, patch SHA-256 `9198fedb67e64fa860568a34f4143cab428398f493f095e69741c7cdeb3aa1d5`, and threads `019fea5e-0015-7b73-9bae-e6be78ee67b2` plus `019fea64-a9c9-7f92-a76e-a266c10c3318`. It reported four findings at confidence `0.93`. Internal two-pass chunking was one item-review invocation and did not create review units. |
| Review dispositions | The stable-digest finding is rejected: `NetworkResourceVersion` is immutable across phase transitions, while mutable store revision is outside the digest. Three findings are accepted and corrected: planned listener/PEP members now come from one authenticated store transaction; namespace inspect/read/remove uses a pinned no-follow parent and descriptor-relative target; provider contention tests wait for the real `WouldBlock` branch. |
| Correction fail-before | Missing batch inspection and real lock-contention probe tests failed to compile with exit `101`. The deterministic parent-replacement test failed because old inspection returned `ExplicitlyAbsent` for the replacement directory. A related empty-selection release test failed with `ReservationLifetimeOwnerLive`; the authority now returns a byte-stable empty no-op before lifetime acquisition. |
| Correction convergence | The first post-review full sandbox run passed `1,100`, failed one empty-listener Krun recovery row, and declared `31` unit ignores. The exact row passed alone. The new deterministic empty-selection test reproduced the lock failure before its two-line no-op correction. The initial final-entry correction full run then had one Container contender child exceed its unchanged 15-second bound under full-suite process pressure; that exact parent passed in `1.23s`, and the unchanged default-concurrency full suite passed on immediate rerun. Final affected behavior is `1,117 + 47`; no timeout or assertion was weakened. |
| Corrected candidate identity | Pre-ledger-closeout staged tree `a5540956a7105c50a1c5a4c4d779b30560418763`; patch SHA-256 `0d8a0bf456b643c719bea25b12e575c00a748539074299d9cd4874a6acafcd39`; 79 paths, including 68 Rust paths. The item commit containing this proof is the final self-authenticating closeout identity. |
| Durable recovery | The commit containing this proof is the exact NNC6.5d3 item checkpoint. Require a clean owner worktree before NNC6.5d4 product edits. K1-K29 are green. |
| Narrow structured review | The one authorized narrow correction review ran through the Nimbus wrapper with GPT-5.6 Sol, `xhigh`, and fast service tier against staged tree `6834896b412ab6a98ef8830417e6e20e36e30508`, patch SHA-256 `300cbd4c6390a04f6c77f3629628fcb2371d91ded4c0415555adbb8d298b2fd6`, 79 paths, and 68 Rust paths. The wrapper used one invocation with two internal bundle passes. It reported two P2 findings and classified the pre-disposition patch as incorrect at confidence `0.96`. The buffered ephemeral wrapper did not emit persistent thread IDs; its exact validated result is recorded here. An earlier invalid absolute `--dataset` attempt stopped before reviewer execution and does not count. |
| Narrow-review dispositions | The plan-snapshot finding is rejected. The second call discards every returned record; expected bindings derive only from immutable `SandboxPortBinding`, `PortLeaseRequest`, and provider inputs. All lifecycle classification fields come from the first atomic `inspect_plan_members` snapshot, and reserved port identity cannot change after adoption. The final-entry finding is accepted: parent identity alone could not detect child creation, removal, or replacement. Inspect/read/remove now retain the exact no-follow target descriptor and device/inode, revalidate its directory entry, use the target descriptor for Linux unmount, and verify final absence. Five deterministic replacement/creation regressions failed to compile before the test hooks existed; all pass after correction, together with the exact-removal success case. |
| Review cadence | The one full item review and one narrow correction review are complete. All accepted findings are corrected and proven; all rejected findings have source evidence. No third review is authorized or warranted. K29 is green. |

## Current Modularity Dispositions

All new production concept files are below 725 lines. The largest new test
owner is 862 lines. These changed inherited roots cross a repository threshold
and retain one explicit disposition:

| File | Lines | Current ownership disposition |
| --- | ---: | --- |
| `crates/nimbus-network/src/port_lease.rs` | 1,918 | Existing public durable lease state-machine root and explicit deep-module exception. Lifetime, rebind, terminal settlement, and batch behavior live in concept children. The added batch read stays beside the same authenticated store transaction owner. |
| `crates/nimbus-network/src/port_lease/lifetime.rs` | 2,791 | One durable lifetime state machine owns lock order, exact lifetime authentication, owner-death recovery, and atomic transitions. Complete-batch mechanics and tests are already children; splitting one transition from the same lock/fence authority would duplicate invariants. |
| `crates/nimbus-network/src/port_lease/lifetime/batch_reservation/tests.rs` | 1,608 | Test-only owner for the complete atomic batch-reservation state machine. The added one-transaction snapshot proof remains with that concept and contains no production composition or effect authority. |
| `crates/nimbus-sandbox/src/provider_command.rs` | 1,561 | One provider-command journal root owns claim serialization, effect/result publication order, and the process-shared stream lock. The added test-only contention probe reports the real lock-wait branch and adds no production authority. |
| `crates/nimbus-sandbox/src/backends/container/runtime.rs` | 1,601 | Container composition root. Manifest, provision, runner, teardown, attachment teardown, and tests remain concept-owned children; the new attachment lifecycle is not implemented here. |
| `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs` | 2,114 | Inherited exact runner handoff and inspection-lock state machine. The item changes only the adjacent lifecycle fence needed by teardown and adds no second process owner. |
| `crates/nimbus-sandbox/src/backends/krun/vm/teardown/tests.rs` | 1,818 | Test-only owner for the legacy Krun execution teardown contract. New network teardown and fresh-process matrices live in separate concept children. |
| `crates/nimbus-sandbox/src/backends/oci/port_lease.rs` | 2,097 | One OCI adapter maps exact portable lease transitions and error semantics for scalar and complete-plan calls. The new retained/final operations remain adjacent to the same authenticated authority. |
| `crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs` | 1,588 | Existing OCI port transition composition owner. Authority construction, machine behavior, planned-Netavark behavior, and state live in concept children. |
