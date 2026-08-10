# NNC6.5d3 Host-Managed Attachment Detach And Release

Status: `audit checkpoint complete; K1 green; K2-K29 pending implementation`

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
| K17 | Release reauthenticates the complete current command, exact manifest, compound detached proof, exact prior `DetachNetwork` journal success, provider absence, namespace absence, retained listener/PEP state, live IPAM, quarantined segment, and `Deleting` attachment before its first release effect. |
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
reuse. The port, IPAM, segment, PEP, and attachment authorities are reopened
from disk between phases to prove retained or released state directly.

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

Only this item remains `in_progress`. Partial bands cannot be marked complete
in the canonical ledger and cannot trigger structured autoreview.

## Acceptance Commands

The exact final counts will be recorded at closeout.

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
| Product implementation | Not started. |
| Structured review | Not authorized before K1-K28 are green and candidate is frozen. |
