# NNC5.6 — Side-Effect-Free Sandbox Inspection

Status: `complete; R1-R14 green; exact 69-path HEAD checkpoint`

Source checkpoint:

- commit: `604a3a13e8059d655f930a83242bca3b5cfe91b0`
- tree: `0834338931c767c830d851b7b620376faf2a1fbf`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- source was clean except for the owner plan's NNC5.5-to-NNC5.6 recovery
  transition before implementation began
- three delegated acceptance audits and three final read-only proof audits
  changed no path
- the sole full structured review ran over the initial 66-path candidate; its
  seven accepted findings are corrected and proven below
- the sole narrow correction review found two incomplete corrections; both
  have exact fail-before and corrected proofs below, and no third review runs
- the complete correction candidate is an owned 69-path diff from the NNC5.5
  checkpoint;
  `crates/nimbus-network/**` remains unchanged

## Unit Of Value

NNC5.6 establishes one command/query boundary across every sandbox backend:
`SandboxBackend::inspect` is a read-only query that returns typed execution,
restart, cleanup, and snapshot evidence. It cannot restart, repair, clean,
release, publish, or persist anything.

Container and Krun remain owners of runtime and network provider effects.
`nimbus-workloads` remains owner of desired workload and saga vocabulary.
`nimbus-compute` remains the only future restart coordinator. An inspection is
comparison evidence, never desired state or launch authority.

The safe intermediate behavior before NNC6.4a is deliberately fail-closed:
an exited or explicitly absent workload that retains provider/network
authority projects `Stopping`, publishes no endpoint, and remains owned until
an explicit command converges restart or teardown.

## Audit Verdict

The current method name hides four mutation authorities:

1. restart scheduling, reset, and provider relaunch;
2. natural-exit provider/network cleanup and authority release;
3. Container PEP repair/replay during readiness observation; and
4. ordinary durable status/manifest publication.

Both observation locks may also create their lock artifact. Krun runtime-state
inspection uses an unbounded child command. Deleting only
`maybe_restart_after_exit` would therefore leave three mutation classes and
one unbounded query path intact.

### Current graph

```text
ComputeState / ServiceManager / Compose / Machine API
                         |
                         v
                SandboxBackend::inspect
                         |
              +----------+----------+
              |                     |
         Container                 Krun
              |                     |
       observe runtime       observe runtime
       repair PEP            write projection
       choose restart        choose restart
       reset/detach          reset/detach
       launch workload       launch workload
       terminal cleanup      terminal cleanup
       write manifest        write manifest
```

### Target graph

```text
Container/Krun read-only adapters
              |
              v
      SandboxInspection
      - projected handle
      - execution evidence
      - restart assessment
      - cleanup finality
      - snapshot version
              |
       services/machine
       typed passthrough
              |
              v
         ComputeState

future NNC6.4a only:
  desired generation + inspection version
    -> durable saga CAS
    -> explicit fenced sandbox command
```

## Source-Derived Call Graph

### Container

```text
SandboxBackend::inspect
  -> ContainerSandboxBackend::inspect_sync
     -> maybe_restart_after_exit
        -> mark_restart_decision_after_exit
           [last_exit, deadline, count, status mutation]
        -> reset_runtime_for_restart
           [runtime delete, PEP stop, detach, publication withdrawal,
            receipt removal]
        -> launch_manifest
           [attachment, Netavark, nft, PEP, creator/runtime effects]
     -> detect_runtime_status
        -> ensure_egress_proxy_running
           [missing-PEP repair/reload replay]
     -> release_execution_artifacts
        [terminal network/provider/IPAM/port cleanup]
     -> write_existing_workload_manifest
```

Owners:

- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs`

### Krun

```text
SandboxBackend::inspect
  -> KrunSandboxBackend::inspect_sync
     -> maybe_restart_after_exit
        -> reset_runtime_for_restart
           [runtime delete, PEP stop, attachment detach, receipt removal]
        -> launch_manifest
           [attachment, Netavark, nft, PEP, creator/VMM effects]
     -> finalize_natural_exit
        -> release_network_artifacts
        -> cleanup_manifest_launch_artifacts
        -> release durable launch/IPAM authority
     -> persist_effect_barrier
        -> write_manifest
```

Owners:

- `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/conmon/lifecycle.rs`

The legitimate initial Krun launch remains
`start -> finish_start -> execute_start -> launch_manifest(true)`. NNC5.6
removes only the inspection edge into effectful lifecycle code.

## Binding Contract

NNC5.6 makes a breaking trait change. It does not retain a parallel
handle-only compatibility method.

The canonical sandbox-owned value seam is:

```rust
pub struct SandboxInspection {
    pub handle: SandboxHandle,
    pub execution: SandboxExecutionObservation,
    pub restart: SandboxRestartAssessment,
    pub cleanup: SandboxCleanupObservation,
    pub version: SandboxInspectionVersion,
}

pub enum SandboxExecutionObservation {
    PlanOnly,
    Present,
    Exited { exit_code: i32 },
    AbsentWithoutExit,
    Unknown { reason: SandboxObservationUnknownReason },
}

pub enum SandboxRestartAssessment {
    Ineligible { reason: SandboxRestartIneligibility },
    Candidate {
        exit_code: i32,
        completed_restarts: u32,
        retry_delay_millis: u64,
        persisted_not_before_millis: Option<u64>,
        blocker: Option<SandboxRestartBlocker>,
    },
}

pub enum SandboxCleanupObservation {
    NotRequired,
    Retained,
    Finalized,
}
```

Final spelling may become tuple variants or concept-owned structs when it
improves exhaustive matching, but the five semantic fields and closed
vocabularies are acceptance requirements.

`SandboxRestartIneligibility` names at least:

- `PlanOnly`;
- `RuntimePresent`;
- `ShutdownRequested`;
- `PolicyNever`;
- `SuccessfulExitExcluded`;
- `AttemptsExhausted`;
- `CleanupPending`;
- `RuntimeAbsenceUnproven`; and
- `ObservationUnknown`.

`SandboxRestartBlocker` names
`StartupReconciliationUnavailable`. A blocked candidate remains evidence of
the exited workload and policy result, but it is not executable.

`SandboxInspectionVersion` is an opaque SHA-256 over the authenticated,
tenant-qualified backend manifest snapshot plus the exit/runtime evidence
used for the returned classification. It is:

- stable for the same snapshot and observation;
- changed by substituted manifest or execution evidence;
- serializable through the internal machine API;
- comparison evidence only;
- never a desired generation, capability, provider handle, or workload
  identity; and
- never derived from an IP address or port.

The pure restart classifier consumes policy, authenticated exit code,
completed attempts, optional existing durable deadline, shutdown state, and
current backend blocker. It never reads a clock, mutates a manifest, increments
an attempt, or creates an absolute deadline.

The existing `next_restart_at_millis` may be reported as historical durable
evidence during NNC5.6, but inspection may not initialize or change it.
NNC6.4a owns moving admitted scheduling/count authority into the workload saga
and deleting any obsolete backend scheduler field. NNC5.6 does not invent that
saga early.

## Projection And Finality Rules

1. Only an observed `Ready` workload may return published endpoints.
2. Exited or explicitly absent workloads with retained provider/network
   authority return `Stopping`, empty endpoints, and
   `SandboxCleanupObservation::Retained`.
3. `Stopped` or `Failed` is cleanup-final only when the backend's existing
   exact finality predicate passes. That result reports `Finalized`.
4. A natural exit does not request shutdown, perform cleanup, release a lease,
   retire IPAM evidence, or write a terminal manifest.
5. Missing PEP registration returns `NotReady`; inspection cannot start,
   repair, or replay the PEP.
6. Ambiguous/malformed evidence returns a typed unknown or error and preserves
   all bytes and effects.
7. Repeated inspection over an unchanged snapshot returns an equal value and
   version.
8. A missing manifest returns `None` without creating a directory or lock.

## Lock And Process Reliability

Inspection opens an existing lifecycle lock and never calls `create_dir_all`
or opens with `create(true)`. A manifest without its synchronization artifact
fails closed with a named error. Start, stop, recovery, and future restart
commands retain exclusive creating lock authority.

Runtime-state observation is bounded. The observer owns:

- child spawn;
- deadline;
- kill on timeout;
- reap on every path; and
- bounded stdout/stderr drain and diagnostic rendering.

Timeout or kill/reap ambiguity cannot become absence, readiness, restart
eligibility, cleanup finality, or success.

## Consumer Migration

The full inspection must survive these internal seams:

- `nimbus-services` refresh and standalone-sandbox inspection;
- `nimbus-compute` service and sandbox GET paths;
- CLI Compose lifecycle inspection;
- parent forwarded-machine backend;
- internal machine client and machine DTO;
- guest machine service-workload adapter; and
- all real/test `SandboxBackend` implementations.

Services may continue caching the projected handle if the full typed result is
returned to compute in the same call. It must not remove or replace a
`Stopping`/`Retained` workload. It must expose a typed inspection path to
compute rather than discard restart evidence before the boundary.

Public service/sandbox HTTP resources may continue projecting only handle
state. The internal Machine API must carry `Option<SandboxInspection>` exactly;
the forwarded adapter cannot synthesize or discard the version, assessment, or
cleanup evidence.

The guest host-lifecycle adapter currently writes node status back into a
PlanOnly sandbox manifest during inspection. It must instead combine the
read-only PlanOnly sandbox snapshot with the read-only node observation and
return a projected inspection value. It may not persist that projection.

Compose treats `Stopping` as active, so explicit `stop` remains the only
teardown action. Services lazy activation must not create a replacement while
cleanup is retained. Full subordination/deletion of lazy activation remains
NNC6.1e-owned.

## Current-To-Target State Matrix

| Evidence | Target execution/restart result | Required side-effect result |
| --- | --- | --- |
| No manifest | `None` | No directory or lock creation. |
| PlanOnly | `PlanOnly` / ineligible | No manifest publication. |
| Launch claimed before effects | current pending projection / ineligible | No effect or write. |
| Runtime running and complete | `Present`, Ready / runtime-present | Readiness observation only. |
| Runtime running, PEP missing | `Present`, NotReady / runtime-present | Zero PEP repair/replay. |
| Runtime creating | `Present`, Starting / runtime-present | No write. |
| Runtime paused | `Present`, Stopping / cleanup-pending | No write. |
| Explicit absence, no exit | `AbsentWithoutExit`, Stopping / absence-unproven | Retain all authority. |
| Exit + shutdown requested | `Exited`, Stopping / shutdown-requested | Retain until explicit stop converges. |
| Exit + `Never` | `Exited`, Stopping / policy-never | No cleanup or release. |
| Exit 0 + `OnFailure` | `Exited`, Stopping / successful-exit-excluded | No cleanup or release. |
| Exit nonzero + `OnFailure`, below limit | `Exited`, Stopping / candidate | No reset, detach, or launch. |
| Any exit + `Always`, below limit | `Exited`, Stopping / candidate | No reset, detach, or launch. |
| Eligible exit + startup fence | candidate with blocker, Stopping | No stale Ready endpoints. |
| Retry limit reached | `Exited`, Stopping / attempts-exhausted | Counter and authority unchanged. |
| Existing durable deadline | candidate reports exact optional deadline | No clock-derived write. |
| Already final manifest | exited/final projection / cleanup-final | Byte-stable. |
| Malformed/ambiguous evidence | typed unknown or error | Byte/effect stable. |

## Fail-Before Packet

The historical NNC0.6a tests are informative but their fixtures have drifted:

- Container now returns early behind retained startup-reconciliation failure,
  before its provider-launch barrier.
- Krun reaches a host-specific missing `/usr/libexec/nimbus/crun` state command
  before its barrier.

That drift is not accepted as a safety pass. Before product correction, restore
deterministic semantic reds by:

1. clearing only the test backend's startup-reconciliation fixture error;
2. using an exact portable shell provider-state response for Krun;
3. pre-releasing the launch probe so the current implementation cannot park;
4. snapshotting the manifest and network authority;
5. calling inspection; and
6. failing at the target zero-launch/byte-stability/typed-candidate assertion.

Convert both ignored baselines into active NNC5.6 proofs. After correction they
assert:

- candidate exit code `42`;
- projected `Stopping`;
- empty endpoints;
- zero provider-launch entries;
- equal manifest and authority bytes; and
- equal repeated inspection/version.

Additional fail-before rows:

- Container natural exit mutates cleanup/manifest bytes.
- Container missing-PEP observation repairs registration.
- Krun running observation reaches `persist_effect_barrier`.
- Krun natural exit records allocator/network cleanup operations.
- Krun startup-fenced exit returns stale Ready endpoints.
- Krun provider-state child exceeds the observation deadline.
- Machine/guest inspection writes PlanOnly status.
- Services activation attempts replacement after retained exit evidence.

Every red must fail at its named target assertion after reaching the intended
production boundary. Fixture/bootstrap errors do not count.

### Restored semantic baseline

The two historical fixtures were restored without changing production code:

- both test backends clear only their startup-reconciliation fixture error;
- both use a portable `/bin/sh` state command that reports exact absence for
  the expected runtime ID;
- Container adopts its exact reserved attachment, releases the never-bound
  listener claim, and clears the initial claim before modeling a
  provider-owned restart; and
- the concurrent withdrawal is durably `Stopping`, matching retained
  nonterminal authority instead of attempting invalid terminal publication.

The exact substring commands each selected one ignored test and exited `101`
at the intended final NNCF20 assertion:

```text
timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6a_container_inspect_must_not_restart_after_withdrawal \
  -- --ignored --nocapture
# 0 passed; 1 failed; 938 filtered.
# left provider-launch effects: 1; required: 0.

timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6a_krun_inspect_must_not_restart_after_withdrawal \
  -- --ignored --nocapture
# 0 passed; 1 failed; 938 filtered.
# left provider-launch effects: 1; required: 0.
```

An initial exact-name invocation selected zero tests because the Container test
is routed through its private `lifecycle` owner module. It is not evidence.
Two earlier fixture attempts also do not count: one returned behind the live
launch claim, and one attempted an invalid cleanup-final `Stopped`
publication. The accepted runs above reached the real provider-launch
interceptor, persisted a valid concurrent withdrawal, and failed only on the
zero-effect safety predicate.

## Behavioral Proof Obligations

### Backend-neutral

- exhaustive pure policy matrix: `Never`, `OnFailure` clean/failing, `Always`,
  below/at limit, and shutdown precedence;
- snapshot version repeatability and manifest/runtime substitution detection;
- no IP/port identity;
- equality/serde round trip for every value variant;
- no compatibility handle-only inspection method.

### Container and Krun

- running Ready and NotReady;
- creating, paused, stopped, and explicitly absent runtime states;
- eligible and ineligible exits;
- existing deadline without deadline creation;
- startup-reconciliation blocker;
- cleanup-retained and already-final states;
- malformed exit receipt, ambiguous provider state, inaccessible pidfile,
  root substitution, missing lock, and lock timeout;
- two concurrent inspectors;
- inspection racing durable withdrawal;
- fresh backend over the same root;
- repeated inspection byte/version equality;
- zero launch/reset/detach/Netavark/nft/PEP/forwarding/port/IPAM effects;
- explicit stop after inspection converges exactly once and replay is
  idempotent; and
- no endpoint for exited/absent retained authority.

### Upper consumers

- service GET and standalone sandbox GET cause no effect;
- typed assessment/version reaches `ComputeState`;
- service lazy activation does not replace retained authority;
- Compose pre-stop inspection does no work and explicit stop performs cleanup;
- internal Machine API round-trips every inspection field;
- forwarded Machine API preserves the exact version;
- guest node inspection does not publish a PlanOnly manifest; and
- test substitutions cover every `SandboxBackend` implementation.

Linux smoke tests that currently poll `inspect` to cause restart must move to
the NNC6.4a explicit restart-command proof. They may not be weakened, silently
skipped, or used to reintroduce query-side restart.

## Static Contract — NNCV024

Add `NNCV024 side-effect-free-sandbox-inspection` to the live aggregate
verifier and a concept-owned mutation helper.

It proves:

- exactly two real `inspect_sync` owners;
- `SandboxBackend::inspect` returns `SandboxInspection`;
- inspection modules contain no restart, launch, reset, release, cleanup,
  finalization, PEP-start, manifest-write, or effect-barrier call;
- both backends use one pure restart classifier;
- observation locks cannot create filesystem state;
- the internal machine DTO carries the full inspection;
- forwarded adapters preserve rather than synthesize evidence;
- services expose typed inspection to compute and retain cleanup-pending
  handles;
- no `nimbus-network` dependency/source/effect changed; and
- production launch callers are explicit command paths only.

Independent mutations inject each forbidden call, a creating lock, a third
inspection owner, handle-only machine DTO, discarded service assessment,
fabricated forwarded candidate, and an `nimbus-network` effect. Each mutation
must fail NNCV024 precisely. Behavioral tests remain primary.

## Owned Paths

The frozen production set is:

- `crates/nimbus-sandbox/src/backend.rs`
- `crates/nimbus-sandbox/src/instance.rs` or a concept-owned
  `crates/nimbus-sandbox/src/inspection.rs`
- `crates/nimbus-sandbox/src/lib.rs`
- `crates/nimbus-sandbox/src/backends/inspection.rs`
- `crates/nimbus-sandbox/src/backends/conmon/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- new `crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/status.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`
- new `crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm/readiness.rs`
- `crates/nimbus-services/src/manager/{handles,sandboxes,types}.rs`
- `crates/nimbus-compute/src/{services,sandboxes}.rs`
- `crates/nimbus-machine/src/api.rs`
- `crates/nimbus-cli/src/compose/lifecycle.rs`
- `crates/nimbus-cli/src/machine/{backend,client}.rs`
- `crates/nimbus-cli/src/machine/api/{routes,service_workloads,state}.rs`
- all directly affected real/test `SandboxBackend` implementations and
  concept-owned focused tests
- `scripts/verify-nimbus-network-source-contract.mjs`
- `scripts/verify-nimbus-network-control-plane.sh`
- one concept-owned NNC5.6 verifier/mutation helper
- this proof, the canonical plan ledger, and the routing index.

The exact changed set may be a strict subset or may include compile-driven
test/caller adaptations, but no unlisted production domain owner may enter
without first recording why it is required by this same seam.

Forbidden paths/seams:

- `crates/nimbus-network/**`
- tenant policy
- proxy forwarding policy/effects
- service-name resolution
- system projections
- cluster transport/membership
- workload saga persistence/schema
- speculative provider interfaces
- compatibility shims.

## Final Implementation And Ownership Census

The implementation replaces the handle-only query with one closed,
sandbox-owned value contract:

- `SandboxInspection` carries the exact projected handle, execution evidence,
  restart assessment, cleanup finality, and opaque SHA-256 comparison version;
- `backends/inspection.rs` owns one pure policy classifier. It consumes no
  clock or provider capability and never advances an attempt or deadline;
- Container and Krun each own one concept-named read-only inspection child.
  Those two modules are the only real `inspect_sync` owners;
- both inspection paths open an already-existing lifecycle lock in shared
  mode, reread the canonical manifest under that lock, and fail closed when
  the lock, manifest, runtime receipt, pidfile, or provider observation is
  missing or ambiguous;
- `run_bounded_command_output` owns its provider child and process group,
  captures output in anonymous regular files, bounds retained diagnostics,
  kills/reaps on timeout and every post-spawn error, and cannot be held by a
  daemonized descendant retaining inherited output descriptors;
- services, standalone sandbox resources, ComputeState, Compose, the guest
  node projection, the forwarded Machine API, the internal machine DTO, and
  every real/test backend preserve the typed inspection;
- service, standalone, and Compose consumers authenticate exact sandbox ID,
  tenant, service/name, and backend before cache, resource, activation, or
  stop effects; and
- inspection no longer reaches restart/reset, provider launch, natural-exit
  cleanup/finalization, PEP repair, attachment/port/IPAM release, effect
  barriers, or manifest publication.

Provider effects remain in `nimbus-sandbox`; restart scheduling and
desired-generation fencing remain NNC6.4a-owned. Exited or explicitly absent
workloads with retained authority project `Stopping`, publish no endpoint, and
remain present for explicit teardown. `nimbus-network` has no changed or
untracked path, so its exact initial workspace edge and transport-free
boundary are unchanged.

Two compile-driven scope additions are exact and do not broaden authority:

- `tempfile` moved from the sandbox dev-dependency table to the normal
  dependency table because production bounded observation now uses anonymous
  regular-file captures; and
- the NNC4.6f production-authority census changed four source line numbers
  only. Occurrence, classification, realm, manager, symbol, and ownership
  fields are byte-identical.

## Candidate Verification Evidence

### Expected-red and corrected command/query boundary

Before production correction, the restored Container and Krun NNC0.6a
substring runs each selected exactly one ignored test and failed `0/1` with
`938` filtered. Each reached the real launch interceptor after a valid
concurrent withdrawal and observed one provider-launch effect versus the
required zero. The corrected full sandbox run executes both tests normally
and reports them green with zero launch and byte-stability assertions.

### Behavioral and reliability proof

| Requirement | Exact candidate evidence |
| --- | --- |
| R2-R7 typed contract, policy, byte/effect stability, projection, and races | Final full `nimbus-sandbox`: `947` passed, `0` failed, `25` declared skips across seven binaries. This includes the exhaustive policy matrix, stable/snapshot-sensitive version, exact external runtime/exit/handoff evidence, Container/Krun runtime-state matrices, missing-lock/no-state creation, concurrent/fresh inspectors, withdrawal races, PEP/natural-exit zero-effect assertions, and both corrected NNC0.6a regressions. |
| R8 bounded runtime observation | The command-owner suite passes `4/4`: actual 64 KiB capture enforcement; prompt return when an escaped-session writer retains the inherited descriptor; timeout termination and reaping of both the shell and its same-process-group descendant; and explicit reap-ownership transfer after injected termination failure. Anonymous regular-file capture plus inherited `RLIMIT_FSIZE` bounds every descriptor-holding writer without pipe-drain threads. |
| R9 explicit convergence | Container/Krun explicit-stop, retained cleanup, restart fencing, port/attachment cleanup, and replay cases pass inside the `947` executed sandbox tests. Services additionally proves retained service and standalone sandbox teardown each converges exactly once. |
| R10 upper consumers | `nimbus-cli` `937` passed, `0` failed, `1` skipped; `nimbus-compute` `71/71`; `nimbus-machine` unit `22/22` plus provider networking `5/5`; `nimbus-services` `95` passed, `0` failed, `1` ignored. Total upper consumers: `1,130` passed, `0` failed, `2` declared skips. Crossed ID/tenant/name/backend evidence is rejected before service cache/lifecycle, standalone resource/lifecycle, and Compose stop effects. Missing inspection evidence for a persisted Compose owner now fails closed before replacement. |
| R10 server GET projections | Focused `nimbus-server service_manager`: `26/26`; focused `tenant_isolation_harness`: `1/1`. The service and standalone GET assertions preserve the exact start/stop counters, proving observation does not activate or stop a workload. |
| Affected behavioral total | `2,104` passed, `0` failed, `27` declared skips across final sandbox, upper-consumer, and touched server suites. |

The complete server suite was also audited. Its two failures are unrelated and
were reproduced at the exact NNC5.5 parent `604a3a13e8059d655f930a83242bca3b5cfe91b0`
before any NNC5.6 source existed:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer`
  returned `400` instead of `200`; and
- `cloud_functions_passes_runtime_owner_lifecycle_conformance` returned `409`
  instead of `200`.

The parent and candidate results therefore do not count those two pre-existing
trust-boundary failures as NNC5.6 acceptance. The exact touched server modules
pass the focused `27/27` gate above.

### Static, mutation, and quality proof

| Gate | Exact candidate evidence |
| --- | --- |
| Live architectural verifier | `25` passed, `0` failed, including NNCV024. |
| Aggregate mutation/self-test | `158` passed, `0` failed. NNCV024 contributes `19/19`: restart, launch, reset, release, cleanup, finalize, PEP start, manifest write, effect barrier, creating lock, third owner, handle-only trait, handle-only machine DTO, missing Krun classifier, discarded service assessment, retained-cleanup eviction, fabricated forwarded candidate, implicit launch caller, and `nimbus-network` effect. |
| Default build matrix | `cargo check` passes for sandbox, services, compute, machine, CLI, and server with all targets. |
| Feature build matrix | The same six crates pass all targets/all features in the isolated V8 target directory required by the pointer-compression last-writer guard. |
| Strict lint | All six affected crates pass all-target/all-feature Clippy with `-D warnings`; diagnostics are limited to inherited vendored Brotli warnings outside the item. |
| Rustdoc | All six affected crates pass all-feature, no-dependency rustdoc with `RUSTDOCFLAGS=-D warnings`. |
| Patch/script quality | `cargo fmt --all --check`, `git diff --check`, Node syntax, both Bash syntax checks, ShellCheck on the NNC5.6 contract, and aggregate ShellCheck with its documented SC2034/SC1091 exclusions pass. |

| Initial reviewed executable/script digest | Exact staged pre-review `git diff --cached --binary HEAD -- crates scripts` SHA-256: `866a602ca43c0f130493be859de082c3c70f0ab5c418d2da3ba60bcab0388e8c`. |
| Initial reviewed tree | `dbb86cbfcdef200b9d6c9583426b7489becf2de8`; exact complete staged patch SHA-256 `93e404b3079ea7a0fde47c33c9bfab5eb0a08868aad53749a7ed62c449823fe7`. |
| Narrow-reviewed correction executable/script digest | Exact staged `git diff --cached --binary HEAD -- crates scripts` SHA-256: `c8dea27795ed077fd5b5e1c40245a279017db5f1c61b28c12c547956d4c58cb7`. |
| Narrow-reviewed correction tree | `52c2f4e645b58219e071d8255aa9e882235073d4`; exact complete staged patch SHA-256 `52a4df3236fcf33dbbb306d02f83fdb0f6906c657dfdba9db16064673a14fde0`. |
| Final executable/script digest | Exact post-review-correction `git diff --binary HEAD -- crates scripts` SHA-256: `789750cbdcb38e540cfd1152606f68eb9979c810fc7f50e10a44ec95783b1e96`. |
| Review state | The sole full and sole narrow reviews are complete and dispositioned. The final executable/script digest is recorded after the two narrow-review corrections are staged. No third review is permitted or warranted. |

## Full Review And Correction Disposition

The sole full structured review used GPT-5.6 Sol at `xhigh` reasoning in fast
mode over staged tree `dbb86cbfcdef200b9d6c9583426b7489becf2de8`.
Review thread: `019fbae2-1dc0-78b3-bb93-e0e23322af6e`. TruffleHog was
clean. The helper sent one complete `376,688`-byte bundle in one pass and
returned seven findings at overall confidence `0.99`. All seven were accepted
because each violated a written NNC5.6 finality, retained-authority, bounded
observation, exact-version, or non-publication criterion:

| Finding | Correction and proof |
| --- | --- |
| P1 — guest-node projection could weaken a `Finalized` base cleanup decision | Guest projection now preserves finality monotonically: `Retained` evidence cannot weaken `Finalized`, and the provider phase contributes exact cleanup evidence. The finality regression passes `1/1`; the final full sandbox suite is `947/947`. |
| P2 — Compose start could discard a terminal-looking `Retained` inspection and replace the workload | Replacement is allowed only after authenticated `Finalized` cleanup. `Retained` blocks replacement and preserves the existing owner. The Compose fence regression passes `1/1`; full CLI is `936/936` with one declared ignore. |
| P2 — tempfile capture bounded bytes read but not file growth by a descendant | The observation command inherits a 64 KiB `RLIMIT_FSIZE`; an anonymous regular file captures output without pipe-drain ownership. The real 64 KiB enforcement and escaped-session writer cases pass inside the `4/4` command-owner suite. |
| P2 — the pipe-based nft wait did not own the whole process group and could hang while draining descendant-held descriptors | Runtime and nft queries now share one regular-file capture path, create a fresh process group, and terminate/reap the live leader plus same-group descendant on timeout. The exact timeout regression proves both PIDs are gone; the escaped-session case returns without waiting for descriptor EOF. |
| P2 — drain setup or `try_wait` failure could drop the child before reap ownership was transferred | A named RAII owner retains the child and transfers reap ownership on injected termination failure. The exact regression passes and proves the command path returns only after ownership is retained or transferred. |
| P2 — inspection comparison versions omitted external provider evidence | Versions now include normalized state plus exact bounded command exit/status/stdout/stderr, raw pidfile/exit receipt, application readiness, opaque attachment-readiness evidence, runner handoff, and guest-node phase evidence. Byte-distinct but semantically equivalent runtime JSON and exit receipts produce distinct comparison versions; the exact version regression passes `1/1`. |
| P2 — contradictory non-final `PlanOnly` terminal/shutdown evidence could publish endpoints or report cleanup `NotRequired` | Container and Krun now project these states as `Stopping` with `Retained` cleanup and no endpoints. Both backend regressions pass `1/1`; the final full sandbox suite remains `947/947`. |

During the full affected-suite rerun after those corrections, the command
owner exposed one related process-group race: after `Child::try_wait` had
successfully reaped the leader, probing or signalling its numeric process
group could target a subsequently recycled group. The final design clears
process-group ownership immediately after a successful reap and never signals
that numeric group afterward. Regular-file capture removes the success-path
drain problem, while inherited `RLIMIT_FSIZE` bounds any surviving
descriptor-holder. The formerly intermittent Krun runtime matrix and the
complete affected suites are green with this correction.

Because the accepted findings materially changed executable Rust and verifier
code, the review cadence permits exactly one narrow correction review focused
on these seven defects after the correction candidate is frozen. It does not
permit another full review or any review for proof, ledger, formatting, or
closeout prose.

### Sole narrow correction review

The sole narrow review used GPT-5.6 Sol at `xhigh` reasoning in fast mode over
tree `52c2f4e645b58219e071d8255aa9e882235073d4`. Review thread:
`019fbb3c-9f50-71d2-a836-f2a2559cd178`. TruffleHog was clean. The
helper sent one complete `421,291`-byte bundle in one pass and returned two P2
findings at overall confidence `0.98`. Both were accepted as incomplete
corrections of the full review's retained-Compose and exact-version findings:

| Finding | Exact fail-before | Correction and proof |
| --- | --- | --- |
| P2 — a persisted Compose owner whose backend inspection returned `None` still resolved as absent and authorized replacement without `Finalized` evidence | The exact CLI regression failed `0/1`, `937` filtered: `resolve_live_service_handle` returned `None` where the test required a finality error. | Missing inspection evidence is now explicit ambiguity: cleanup finality remains unknown and replacement stays fenced. The regression passes `1/1`; full CLI is `937/937` with one declared skip; no start effect occurs. |
| P2 — Container exit and already-final branches omitted raw runner-handoff bytes from comparison versions | The exact Sandbox regression failed `0/1`, `962` filtered: byte-distinct semantically equivalent handoff records produced equal versions in the exit branch before the finalized assertion could run. | Exit versions commit to an exact tuple of handoff and exit bytes; already-final versions commit to exact handoff bytes. The regression proves both normalized outcomes stay equal while both exact versions differ; it passes `1/1`; full Sandbox is `947/947` with 25 declared skips. |

The combined fail-before packet was exactly `0/2`; the corrected packet is
`2/2`. After correction, full Sandbox and CLI, all-target/all-feature check,
strict Clippy with `-D warnings`, and warning-denied rustdoc are green. The
warnings printed by those commands are inherited vendored Brotli diagnostics
outside the item.

This was the one permitted narrow review. Its accepted findings were corrected
and proven manually; the cadence does not permit a third review, and none ran.

## Exact Candidate Path Census

The correction candidate contains exactly these 69 item-owned paths:

```text
crates/nimbus-cli/src/compose/lifecycle.rs
crates/nimbus-cli/src/compose/tests/lifecycle.rs
crates/nimbus-cli/src/compose/tests/support.rs
crates/nimbus-cli/src/machine/api/routes.rs
crates/nimbus-cli/src/machine/api/service_workloads.rs
crates/nimbus-cli/src/machine/api/tests.rs
crates/nimbus-cli/src/machine/api/tests/publication_evidence.rs
crates/nimbus-cli/src/machine/backend.rs
crates/nimbus-cli/src/machine/backend/tests/publication_authority.rs
crates/nimbus-cli/src/machine/client.rs
crates/nimbus-cli/src/machine/stub/backend.rs
crates/nimbus-cli/src/machine/stub/client.rs
crates/nimbus-compute/src/sandboxes.rs
crates/nimbus-compute/src/services.rs
crates/nimbus-machine/src/api.rs
crates/nimbus-sandbox/Cargo.toml
crates/nimbus-sandbox/src/backend.rs
crates/nimbus-sandbox/src/backends/conmon/lifecycle.rs
crates/nimbus-sandbox/src/backends/container/runtime.rs
crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs
crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs
crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs
crates/nimbus-sandbox/src/backends/container/runtime/runner.rs
crates/nimbus-sandbox/src/backends/container/runtime/status.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/absent_runtime_projection.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/egress_reload_recovery.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/execute_inspection.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/plan_only_inspection.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/network_finality.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/startup_fencing.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/restart_policy.rs
crates/nimbus-sandbox/src/backends/container/runtime/tests/status_callbacks.rs
crates/nimbus-sandbox/src/backends/inspection.rs
crates/nimbus-sandbox/src/backends/krun/vm.rs
crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs
crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs
crates/nimbus-sandbox/src/backends/krun/vm/readiness.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/egress_readiness.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/explicit_stop.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/lifecycle_locking.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/natural_exit.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/startup_fencing.rs
crates/nimbus-sandbox/src/backends/krun/vm/tests/support.rs
crates/nimbus-sandbox/src/backends/mod.rs
crates/nimbus-sandbox/src/backends/oci/command.rs
crates/nimbus-sandbox/src/backends/oci/egress/readiness.rs
crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs
crates/nimbus-sandbox/src/backends/readiness_probe.rs
crates/nimbus-sandbox/src/inspection.rs
crates/nimbus-sandbox/src/lib.rs
crates/nimbus-server/src/tests/service_manager.rs
crates/nimbus-server/src/tests/service_manager/sandboxes.rs
crates/nimbus-server/src/tests/tenant_isolation_harness.rs
crates/nimbus-services/src/manager/activation.rs
crates/nimbus-services/src/manager/definitions.rs
crates/nimbus-services/src/manager/handles.rs
crates/nimbus-services/src/manager/sandboxes.rs
crates/nimbus-services/src/manager/tests/definition_lifecycle.rs
crates/nimbus-services/src/manager/tests/lifecycle.rs
crates/nimbus-services/src/manager/tests/mod.rs
crates/nimbus-services/src/manager/tests/sandbox_resources.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
docs/private/plans/proof/nimbus-network-control-plane/nnc5.6-side-effect-free-sandbox-inspection.md
scripts/nimbus-network-control-plane/side-effect-free-sandbox-inspection-contract.sh
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
```

No path outside this list is staged or required for NNC5.6.

## Modularity

At correction convergence:

- `container/runtime.rs`: 1,378 lines;
- `container/runtime/inspection.rs`: 320 lines;
- `krun/vm/lifecycle.rs`: 1,481 lines;
- `krun/vm/inspection.rs`: 255 lines;
- `backends/oci/command.rs`: 520 lines;
- `backends/readiness_probe.rs`: 590 lines;
- `container/runtime/status.rs`: 73 lines;
- `krun/vm/readiness.rs`: 58 lines;
- `services/manager/handles.rs`: 141 lines;
- `verify-nimbus-network-source-contract.mjs`: 1,253 lines; and
- the concept-owned NNC5.6 shell contract: 61 lines.

Both former lifecycle roots are below the 1,500-line threshold after moving
inspection into concept-owned children. The bounded command owner remains a
single coherent child/process-ownership module well below the threshold.
Test-heavy growth stays in concept-owned existing test children; no production
composition root became a new switchboard.

The plan's older source-count prose must be refreshed from final source.
Broken links to proposed-but-absent sandbox plan/research files are truthed up
as non-link routing prose; this item does not invent a parallel plan.

## Ownership And Overlap

- NNC3.8 retains ambiguous restart-retained cleanup and terminal-finality
  convergence.
- NNC5.3/NNC5.5 supply read-only attachment/readiness capabilities.
- NNC6.1b-NNC6.1d own durable workload saga/store vocabulary.
- NNC6.1e owns lazy activation subordination/deletion.
- NNC6.4a owns the explicit desired-generation-fenced restart command,
  schedule, attempt count, attachment/PEP reacquisition, and activation.
- NNC8.3 owns orphan cleanup/finalization/reuse.
- NNC8.4 owns stale desired generation/epoch rejection.
- `nimbus-node` systemd `Restart=OnFailure` remains a residual provider restart
  authority that NNC6.1a/NNC6.4a must set to `No`; NNC5.6 does not claim that
  compute is already the sole global restart authority.
- horizontal scaling retains cluster transport, membership, routing, Iroh,
  and openraft.

## Acceptance Ledger

| ID | Criterion | Status |
| --- | --- | --- |
| R1 | Current/target graphs, both backend call graphs, callers, and overlap are source-derived and frozen. | green |
| R2 | One backend-neutral typed inspection contract separates handle projection, execution, restart, cleanup, and comparison version. | green |
| R3 | The pure restart classifier covers the complete policy/attempt/shutdown/blocker matrix without clock or mutation. | green |
| R4 | Every Container/Krun inspection branch is byte-stable and creates no lock/directory. | green |
| R5 | All workload/network/provider effect probes remain zero, including PEP repair and natural-exit cleanup. | green |
| R6 | Exited/absent retained authority is `Stopping`, non-publishable, typed, and never discarded. | green |
| R7 | Withdrawal races, repeated inspection, concurrent inspection, and stale snapshot substitution are deterministic and safe. | green |
| R8 | Runtime-state observation is bounded and owns kill/reap/drain on every path. | green |
| R9 | Explicit teardown after observation converges exactly once and retains existing NNC3.8/NNC8.3 fences. | green |
| R10 | Services, compute, Compose, guest/forwarded Machine API, and every backend implementation preserve the typed seam. | green |
| R11 | NNCV024 and all independent mutations pass; core-only/effect-locality invariants remain green. | green |
| R12 | Focused and full affected behavior, check, strict Clippy, warning-denied rustdoc, format, diff, syntax, and ShellCheck pass with exact counts. | green |
| R13 | Live/aggregate verifier and docs/site gates pass with exact counts. | green |
| R14 | The sole complete candidate-frozen GPT-5.6 Sol/xhigh/fast review and the one permitted narrow correction review are dispositioned; all nine accepted findings have exact corrected proof and no third review ran. | green |

NNC5.6 is complete only when R1-R14 are green, the final exact proof and
review dispositions are recorded, the item is committed with its ledger
transition, and the worktree contains no unrelated path. No push or PR is
authorized.

## Recovery Ledger

| Checkpoint | State | Evidence / next action |
| --- | --- | --- |
| Source | `done` | Clean NNC5.5 commit `604a3a13e8059d655f930a83242bca3b5cfe91b0`, tree `0834338931c767c830d851b7b620376faf2a1fbf`; six read-only audits changed no path. |
| Audit | `done` | Container/Krun call graphs, all upper consumers, lock/query effects, retained/final cleanup semantics, restart authority, version evidence, and plan overlap are source-derived. |
| Acceptance freeze | `done` | R1-R14, non-goals, exact owner boundaries, expected-red proof, and item-level review cadence were recorded before candidate review. |
| Fail-before | `done` | Both restored NNC0.6a tests select one case and fail `0/1`, 938 filtered, solely because inspection reaches one provider launch after withdrawal. |
| Implementation | `done` | One typed read-only inspection seam, pure restart assessment, existing-only shared locks, retained-authority projection, exact version evidence, bounded command ownership, and full upper-layer passthrough are implemented. |
| Initial item review | `done` | Sole full Sol/xhigh/fast review over tree `dbb86cbfcdef200b9d6c9583426b7489becf2de8`; one bundle/pass, TruffleHog clean, seven accepted findings at `0.99`. |
| Correction convergence | `done` | The seven full-review defects plus the related post-reap process-group race are corrected. The sole narrow review found two incomplete corrections; their combined exact fail-before is `0/2` and corrected packet is `2/2`. |
| Narrow correction review | `done` | Sole narrow Sol/xhigh/fast review over tree `52c2f4e645b58219e071d8255aa9e882235073d4`; one bundle/pass, TruffleHog clean, two accepted P2 findings at `0.98`; both corrected and proven; no third review. |
| Final acceptance | `done` | Sandbox `947/947`; upper `1,130/1,130`; touched server `27/27`; total `2,104/2,104`, 27 declared skips; live verifier `25/25`; mutations `158/158`; affected check/strict Clippy/warning-denied rustdoc green. |
| Exact checkpoint | `done` | Final executable/script SHA-256 `789750cbdcb38e540cfd1152606f68eb9979c810fc7f50e10a44ec95783b1e96`; exact 69-path item committed once as the NNC5.6 HEAD checkpoint with the NNC6.1 recovery transition. No push/PR. |
