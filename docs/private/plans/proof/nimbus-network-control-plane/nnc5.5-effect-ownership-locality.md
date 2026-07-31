# NNC5.5 — Sandbox Effect Ownership And Locality

Status: `complete; R1-R12 green`

Source checkpoint:

- commit: `8fe2ccfb527a53f657361943b574c2e571db7563`
- tree: `bfa6318b367dad81e7626a7e1afd5c1977a48ed5`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- source was clean when the item started
- original dirty checkout and clean `machine-os` companion were inspected only
  and remain unchanged

## Unit Of Value

NNC5.5 proves and seals one architectural property: privileged networking and
application-readiness effects remain substitutable, bounded, and local to
`nimbus-sandbox`, while `nimbus-network` remains the transport-free owner of
portable connectivity-resource state with exactly one workspace dependency,
`nimbus-core`.

The item covers the existing Netavark, network-namespace, IPAM, nftables,
gvproxy, local proxy, and TCP/HTTP readiness effects. It does not move those
effects into the portable crate or create a generic `NetworkProvider`.

No structured autoreview ran during audit, fail-before work, implementation,
cleanup, or acceptance convergence. The one full GPT-5.6 Sol/xhigh/fast review
ran only after R1-R11 and every affected gate were green and the complete item
was candidate-frozen. It found five accepted material executable defects. All
five now have exact corrections and proofs; exactly one narrow correction
review remains, focused only on those defects.

## Read-Only Audit

### Portable boundary

`crates/nimbus-network/Cargo.toml` currently has one normal workspace edge:
`nimbus-core`. Its other normal dependencies are portable serialization,
hashing, identity, locking, and platform filesystem dependencies. It has no
socket, async runtime, HTTP, provider SDK, cloud SDK, namespace, firewall,
Netavark, gvproxy, tenant, sandbox, server, system, or cluster-transport
dependency.

The crate uses `IpAddr` and `SocketAddr` as validated values. It does not bind,
listen, accept, connect, send protocol bytes, launch commands, or realize a
provider. Durable filesystem and lock I/O in the portable lease/state
authorities is deliberate and is not packet transport or provider I/O.

`NetworkSegmentAllocator` consumes stable tenant-qualified attachment and
claim identities, resource versions, segments, and epochs. Its signatures do
not carry a namespace path, socket, command, Netavark request, provider-effect
callback, or sandbox type. `provider.rs` and `capability_registry.rs` contain
opaque handles and serializable capability reports, not effect methods.

### Production effect and call-path census

| Effect | Sole current production owner | Authorized production caller |
| --- | --- | --- |
| Create, mount, inspect, unmount, and remove a persistent network namespace | `oci/network/netns.rs` | real `AttachmentHostEffects` adapter |
| Prepare and durably journal Netavark setup/teardown attempts | `oci/network/netavark.rs` | real `AttachmentHostEffects` adapter |
| Launch Netavark and exchange its stdin/stdout/stderr | private `run_netavark` in `oci/network/netavark.rs` | prepared Netavark capability methods |
| Allocate/deallocate static addresses and authenticate provider attempts | `oci/network/ipam.rs` plus `ipam/*` | shared attachment lifecycle |
| Apply and inspect nftables pins through `nsenter nft` | `oci/network/egress_pin.rs` | shared attachment lifecycle/readiness |
| Inspect, expose, and withdraw native gvproxy routes | real `MachinePortForwardingProvider` in `oci/network/forwarding.rs` | machine-publication coordinator |
| Bind and accept a host listener and connect/copy to a guest target | `oci/network/proxy.rs` | Container machine-port composition |
| Observe current proxy-process lifetime | `oci/network/process/machine_proxy_lifetime.rs` | readiness/cleanup composition |
| Probe a workload TCP or HTTP endpoint | duplicated in Container `runtime/status.rs` and Krun `vm/readiness.rs` | each backend status path |

Container and Krun converge their portable attachment effects through the same
`OciAttachmentLifecycle`, real `AttachmentHostEffects` adapter, injected
`NetworkSegmentAllocator`, `OciEgressPinProvider`, and provider-specific
machine-forwarding adapter. Desired state, durable attempt/lease state,
process-local lifetime, and fresh observed provider evidence remain distinct.

`OciIpamAuthority` is the only durable IPAM authority. Direct and runner
construction are explicit composition handles over the same locked
`LocalNetworkStateStore`, not sibling authorities. NNC4.6c's documented
direct/runner construction exception remains valid; this item does not imply
that only `OciNetworkProcess` may construct a backend.

### Substitution inventory

| Capability | Real adapter | Deterministic substitute | Verdict |
| --- | --- | --- | --- |
| Attachment namespace/Netavark host effects | `RealAttachmentHostEffects` | deterministic lifecycle test adapter | Correct and sandbox-private. |
| Segment allocation | injected `dyn NetworkSegmentAllocator` | portable deterministic authorities | Correct; no realization capability crosses the seam. |
| nft apply/inspect | real `OciEgressPinProvider` | fixed/call-count provider | Correct, but readiness receives apply authority it does not need. |
| gvproxy inspect/expose/withdraw | `OciMachinePortForwarderConfig` | deterministic and stateful ambiguity providers | Correct and sandbox-private. |
| host listener/worker lifetime | `MachinePortProxyLifetimeRegistry` | deterministic process-lifetime evidence | Correct; process liveness is not durable provider authority. |
| application readiness probe | direct duplicated `TcpStream` effects | none | Incorrect: two effect owners and no deterministic capability. |

### Current versus target

```text
current
  nimbus-network
    portable identities/plans/leases/state only

  nimbus-sandbox
    deep Netavark/netns/IPAM/nft/gvproxy/proxy adapters
    + two copied direct TCP/HTTP probe implementations
    + namespace primitives visible crate-wide
    + readiness receives nft mutation authority
    + static verifier misses exact production syntax

target
  nimbus-network
    unchanged transport-free product surface
    + exact dependency/effect guards

  nimbus-sandbox
    one private adapter per privileged effect family
    + one bounded, substitutable readiness-probe capability
    + least-authority nft observation capability
    + sealed namespace/Netavark callers
    + mutation-tested locality/capability guards
```

## Findings And Binding Decisions

### 1. Consolidate application-readiness effects

Container and Krun currently duplicate the TCP/HTTP probe byte-for-byte and
perform direct `TcpStream` I/O. Introduce one concept-owned,
crate-private readiness-probe module below `backends/`. It owns:

- target selection from declared HTTP, HTTPS, and TCP endpoints;
- a small `ReadinessProbeProvider` capability;
- the sole real bounded socket implementation;
- deterministic fixed/recording substitutes; and
- the common Ready, NotReady, and Unknown reduction.

Container and Krun keep manifest-specific timeout and status projection. They
must not retain direct socket I/O, copied HTTP parsing, or a second target
selector. HTTPS remains a TCP reachability probe unless a deliberate TLS
provider is introduced by another owner.

### 2. Bound every probe and nft observation wait

The real probe sets connect and read deadlines but omits the write deadline.
The nft inspection path calls `Command::output()` without a child-process
deadline. NNC5.5 adds:

- connect, read, and write deadlines for application probes;
- response-size and status-line bounds already enforced by the probe;
- a bounded nft inspection runner that kills and reaps a child on timeout;
- deterministic timeout/error proofs; and
- bounded listener/test joins so a failed assertion cannot park a test.

Timeout, malformed output, unavailable provider, child-kill failure, or
ambiguous observation remains `Unknown`; it cannot mint readiness.

### 3. Narrow capability authority

Readiness only observes nft state. Split an `OciEgressPinObserver` capability
from the mutating `OciEgressPinProvider`; the provider extends the observer.
The core readiness composer accepts only the observer. Apply remains available
to lifecycle reconciliation, never to the read-only composer.

Namespace creation/removal primitives are narrowed from crate-wide visibility
to the lifecycle-owned effect path. Test access is deliberate and does not
widen the production capability. The stale DNS comment in `layout.rs` is
corrected to match the shared `enable_dns: false` production configuration.

### 4. Strengthen the portable dependency/effect guard

NNCV004 must prove the exact approved dependency envelope, including:

- `nimbus-core` is a normal, unconditional, non-optional workspace edge;
- no second workspace edge exists;
- no async runtime, HTTP/transport library, provider/cloud SDK, or equivalent
  external dependency is admitted; and
- Windows dependencies cannot enable networking features.

NNCV012 must recognize production syntax actually used by the sandbox:

- grouped imports followed by `TcpStream::connect_timeout`;
- grouped imports followed by unqualified `Command::new`;
- `libc::mount` and `libc::umount2`;
- provider DTO/effect traits even when no direct socket call appears; and
- an upper-crate import such as `nimbus_sandbox`.

Comments, string literals, tests, address value types, and intentional durable
filesystem I/O remain valid.

### 5. Add sandbox-locality and capability seals

Add two production-aware, mutation-tested aggregate conditions:

- **NNCV022 — sandbox provider-effect locality:** one structural owner for
  namespace, Netavark, nft, gvproxy, proxy-listener, and readiness-probe
  effects, with only the named composition callers.
- **NNCV023 — sealed provider-effect capabilities:** privileged traits and
  prepared provider capabilities stay private, are not crate-root reexports,
  portable allocator signatures cannot acquire effect/path/callback
  parameters, and read-only composers cannot receive mutation authority.

The conditions use concept/path ownership and syntax structure, not line-number
anchors. Existing bind-census authority remains the owner of bind/allocation
classification; NNCV022 consumes or complements that proof and must not create
a competing census.

## Frozen Failure Evidence

At the source checkpoint, the following expected reds are recorded before any
executable edit:

1. The one-owner probe census exits `1` because
   `probe_target_ready` exists in both Container and Krun.
2. A grouped `TcpStream` import plus `TcpStream::connect_timeout` injected into
   the NNCV012 self-test exits `0`; this is a false green.
3. A grouped `Command` import plus unqualified `Command::new("nsenter")`
   injected into the NNCV012 self-test exits `0`; this is a false green.
4. Independent `libc::mount`, `libc::umount2`, `nimbus_sandbox` import, and
   portable `ForwardingProvider` trait fixtures each exit `0`; all four are
   false greens.

Exact source-checkpoint commands used the existing
`NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1` and
`NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT` fixture path. The duplicate-owner
census reported the two exact files and exited `1`; all six forbidden-effect
fixtures above emitted no diagnostic and exited `0`.

The implementation must add exclusive mutation cases for:

| Mutation | Required failure |
| --- | --- |
| grouped `TcpStream::connect_timeout` in portable production source | NNCV012 only |
| grouped `Command::new("nsenter").arg("nft")` in portable production source | NNCV012 only |
| `libc::mount` or `libc::umount2` in portable production source | NNCV012 only |
| `nimbus_sandbox` import or portable forwarding/effect trait | NNCV012 only |
| wrong `nimbus-core` dependency profile, `tokio`, or Windows networking feature | NNCV004 only |
| direct namespace or prepared-Netavark call from Container/Krun | NNCV022 only |
| second production Netavark, nft, gvproxy, listener, or probe effect owner | NNCV022 only |
| moved gvproxy mutation call outside its coordinator | NNCV022 only |
| widened/reexported privileged provider capability | NNCV023 only |
| provider path/effect/callback added to the portable allocator | NNCV023 only |
| nft apply capability added to read-only attachment readiness | NNCV023 only |

Every mutation must also prove that all unrelated conditions remain green.

## Frozen Owned Paths

NNC5.5 may change only:

- this proof;
- the canonical plan and routing index;
- `crates/nimbus-sandbox/src/backends/mod.rs`;
- one new concept-owned readiness-probe module and its concept-owned tests;
- Container and Krun readiness/status composition needed to consume that
  capability;
- `crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs`;
- the narrow attachment-readiness composer and its capability wiring;
- namespace visibility/reexport and the stale layout comment;
- `scripts/verify-nimbus-network-source-contract.mjs`;
- the aggregate verifier orchestration for NNCV022/NNCV023; and
- one concept-owned static helper plus its mutation contract; and
- the existing production network-authority census only to refresh exact
  source-derived line projections shifted by the injected backend capability
  field/imports. Its classifications, occurrence keys, realms, and owners may
  not change.

No `nimbus-network` product source, server, tenant, services, proxy, compute,
system, machine, KV, cluster, or unrelated sandbox code is claimed. If an
acceptance criterion cannot be met inside these paths, record the conflict and
re-scope the item explicitly before crossing the boundary.

## Verifiable Acceptance Criteria

### R1 — Exact effect census

The final proof names every production Netavark, namespace, IPAM, nft,
gvproxy, listener/proxy, and application-probe effect plus every authorized
production caller. No effect or caller remains unclassified.

### R2 — Exact portable boundary

Cargo metadata and source scans prove that `nimbus-network` has exactly the
approved `nimbus-core` workspace edge and no transport, provider, cloud,
upper-crate, namespace, firewall, command, socket, or forwarding effect. Exact
grouped-import and platform-feature mutations fail closed.

### R3 — One readiness-probe effect owner

One sandbox-private module owns target selection and real TCP/HTTP probe I/O.
Container, Krun, and their composition roots contain no direct probe socket
effect or copied target selector.

### R4 — Real substitution

The readiness capability has one real provider and deterministic substitutes.
Tests prove exact captured target/timeout and call count, zero calls without a
target, recovery after NotReady, and identical Container/Krun reduction.

### R5 — Complete readiness behavior

Behavior tests cover TCP ready/refused/timeout, HTTP accepted/rejected/malformed
status, bounded response, no endpoint, HTTP-over-TCP preference, HTTPS-as-TCP,
provider Unknown, and Ready/NotReady/Unknown transitions. Every test asserts a
specific result.

### R6 — Bounded effects

Connect, read, write, command wait, child kill/reap, and test listener/join
paths are bounded and fail loudly. nft timeout/error/malformed output and probe
timeout cannot mint readiness or leak a child.

### R7 — Existing adapters remain substitutable

Focused tests preserve real/deterministic behavior and call counts for
attachment host effects, IPAM/segment substitution, nft apply/inspect, gvproxy
inspect/expose/withdraw, machine publication ambiguity, proxy lifetime, and
association rejection before effects.

### R8 — Least authority

The attachment-readiness core receives only nft observation authority.
Namespace and prepared-Netavark effect capabilities are visible only to the
deep adapter owner. A compile/static mutation proves readiness cannot call
apply and upper modules cannot bypass the adapter.

### R9 — Static locality and capability mutations

NNCV004, NNCV012, NNCV022, and NNCV023 pass live and every frozen adversarial
mutation fails exclusively in its named condition. The verifier self-test
reports exact pass/fail counts and exercises missing-input failure.

### R10 — No duplicated authority or dependency cycle

Static and manual seam checks prove that desired plans, durable leases and
attempts, provider effects, process lifetimes, and observed status each retain
one owner. The portable dependency graph remains acyclic and no god
`NetworkProvider` is introduced.

### R11 — Full affected quality proof

Focused fail-before and behavioral suites, the full `nimbus-network` and
`nimbus-sandbox` suites, affected checks, strict Clippy, warning-denied rustdoc,
format, diff, syntax, ShellCheck, dependency/effect scans, docs, and plan
verifier all pass with exact counts.

The coherent 2,105-line `nimbus-network/src/state_store.rs` remains the
explicit NNC5.2b deep-module exception. It is not decomposed by this
effect-locality item. Any newly created concept module must remain below the
repository threshold, and composition roots must become smaller rather than
accumulate logic.

### R12 — Review and exact checkpoint

After R1-R11 are green, freeze the exact executable/script digest and run one
full GPT-5.6 Sol/xhigh/fast review of the complete item. Classify every finding
with source and test evidence. Only an accepted material executable correction
permits one narrow correction review. Then record exact proof counts, update
the recovery/status ledgers, stage the exact owned diff, and commit one NNC5.5
checkpoint. Do not push or open/update a PR.

## Non-Goals

NNC5.5 does not:

- move provider effects or policy into `nimbus-network`;
- change portable network resource semantics;
- absorb tenant PDP, proxy PEP/forwarding, service naming, certificates,
  cluster transport, or system projections;
- change workload inspect/restart behavior owned by NNC5.6;
- implement orphan cleanup, release, finalization, or capacity reuse owned by
  NNC8.3;
- redesign native Netavark, nftables, or gvproxy wire contracts;
- invent DNS, xDS, Consul, overlay networking, or a god provider interface; or
- decompose an already justified unrelated deep state-store module.

## Final Implementation And Ownership Census

The implementation keeps every privileged effect in `nimbus-sandbox` and
changes no `nimbus-network` product source:

| Effect family | Sole final production owner | Authorized production consumer |
| --- | --- | --- |
| Persistent namespace create/mount/inspect/unmount/remove | private `backends/oci/network/netns.rs` | `attachment_lifecycle/host.rs` through `RealAttachmentHostEffects` |
| Netavark attempt preparation, command launch, and response parsing | `backends/oci/network/netavark.rs` | `RealAttachmentHostEffects` after the durable provider-attempt transition |
| Static IPAM allocation, inspection, and retirement | `backends/oci/network/ipam.rs` and its concept-owned children | shared `OciAttachmentLifecycle` |
| nft pin apply and observation through bounded `nsenter nft` | `backends/oci/network/egress_pin.rs` | lifecycle receives `OciEgressPinProvider`; readiness receives only `OciEgressPinObserver` |
| Native gvproxy inspect/expose/withdraw | `backends/oci/network/forwarding.rs` | machine-publication coordinator |
| Host proxy listener, guest connect, and copy pumps | `backends/oci/network/proxy.rs` | Container machine-port composition |
| Proxy-process lifetime observation | `backends/oci/network/process/machine_proxy_lifetime.rs` | readiness and cleanup composition |
| TCP/HTTP application readiness | `backends/readiness_probe.rs` | Container and Krun status adapters through injected `dyn ReadinessProbeProvider` |

The shared readiness module owns target selection, status reduction, the sole
real socket provider, and deterministic substitutes. Selection is exactly HTTP
first, HTTPS as TCP second, and TCP third. HTTP uses one absolute deadline
across connect/write/read, bounds the response to 256 bytes, accepts only
HTTP/1.0 or HTTP/1.1 status codes 100 through 599, requires a complete
newline-terminated status line, handles fragmented lines, and fails closed on
truncated, malformed, oversized, refused, or timed-out responses. No endpoint
returns Ready without invoking the provider.

`OciEgressPinObserver` is the read-only capability consumed by readiness;
`OciEgressPinProvider` extends it only for lifecycle-owned apply. Both nft apply
and inspection use a two-second command deadline, retain cleanup ownership even
when termination initially fails, kill and reap a timed-out child, drain its
pipes, and return error/Unknown rather than readiness.
Namespace functions are `pub(super)`, are no longer reexported from the network
module root, and are imported only by the private host-effects adapter.

NNCV004 now admits the exact portable dependency envelope, NNCV012 recognizes
the production Rust effect syntax that was previously missed, NNCV022 owns the
exact sandbox effect/caller census, and NNCV023 seals privileged capabilities
and least-authority visibility. NNCV022 complements rather than duplicates the
bind/allocation census; the NNC4.6f inventory changed only thirteen
source-derived line projections and no occurrence, classification, realm,
manager, or ownership field.

## Candidate Verification Evidence

| Requirement | Exact evidence |
| --- | --- |
| R1 / R10 ownership and authority | The final census above plus live NNCV005/NNCV006/NNCV015/NNCV022/NNCV023 prove one owner for plans, durable leases/attempts, provider effects, process lifetime, and observation. No `NetworkProvider` or second bind/allocation census was introduced. |
| R2 portable boundary | NNCV004 and NNCV012 pass live. The exact normal workspace edge remains `nimbus-network -> nimbus-core`; no socket, command, transport, cloud/provider SDK, upper-crate, namespace, firewall, or forwarding capability entered the portable crate. |
| R3 / R4 shared probe owner and substitution | The duplicate Container/Krun socket owners were deleted. One 560-line `readiness_probe.rs` owns the real provider and fixed recording substitute. Common reduction tests plus exact Container and Krun injection tests prove captured target/timeout/call count, no-target zero calls, NotReady recovery, and both adapters' delegation to the same reduction. |
| R5 / R6 readiness and bounded effects | Readiness behavior is `10/10`: TCP ready/refused, deterministic timeout, HTTP valid/malformed/truncated/range/version, fragmented line, 256-byte bound, no-response deadline, target priority, no endpoint, and Ready/NotReady/Unknown transitions. The exact truncated-line review regression is `1/1`. nft apply/inspect timeout kills and reaps a real `sleep 5` child in less than one second; the injected first-termination-failure regression is `1/1` and proves `waitpid == -1/ECHILD` after detached cleanup. The complete egress-pin suite is `12/12`. |
| R7 existing substitution | The focused host-effects, attachment, allocator/IPAM/segment, nft, forwarding, proxy, machine-publication, and lifetime lane passes `162/162`, with `785` unrelated tests skipped by the expression and one declared slow case. |
| R8 least authority | Readiness accepts `dyn OciEgressPinObserver`, not the mutating provider. Namespace effects are `pub(super)` and have no root reexport. NNCV023 mutations for readiness apply, widened host effects, namespace reexport, public forwarding provider, and portable provider capability all fail closed. |
| R9 static mutation proof | The corrected isolated NNC5.5 contract passes all `27/27` dependency/source/locality/capability mutations: six NNCV004, six NNCV012, nine NNCV022, five NNCV023, and one portable-capability case. Each review-added dependency-profile, command-outside-owner, and readiness-wrapper mutation also exits `1` with only its named condition failing. The corrected complete aggregate rerun passes `139/139`; the first corrected run exercised the same 139 registered cases but exposed a stale hard-coded `134` success label, so that evidence-label defect was corrected and the unchanged verifier reran with captured output ending `self-test: 139 passed, 0 failed`. Live verifier: `24 passed, 0 failed`. |
| R11 behavior | Corrected `cargo nextest run -p nimbus-network -p nimbus-sandbox`: `1,156/1,156` passed, `28` declared skips, one declared slow crash-convergence test, exit `0`. The focused readiness/attachment/nft packet passed `39/39` before review; the corrected readiness packet is `10/10`, egress-pin is `12/12`, and the complete affected suite proves the correction candidate. |
| R11 quality | `cargo check` for both affected crates with all targets/features, strict `cargo clippy ... -- -D warnings`, and warning-denied rustdoc all pass. Diagnostics are limited to pre-existing vendored Brotli warnings outside the item. `cargo fmt --all --check`, `git diff --check`, Node syntax, both Bash syntax checks, and ShellCheck on the new contract pass. |
| R11 documentation | `scripts/check-docs.sh`: `108` pages link-clean; `scripts/verify-nimbus-docs-site.sh`: `17/17` conditions green, including the correction disposition. |
| Initial reviewed executable/script digest | Exact staged pre-review `git diff --cached --binary HEAD -- crates scripts` SHA-256: `1dbdff8e60d39e3ad26057839749e59807f70491260bf486b60d0351746fcfee`. |
| Initial reviewed tree | `0ada7d9bf082c8f3fc6ff60afbda8881b8981d72`; exact complete staged patch SHA-256 `bfc47b9e9edb2a1f853304206b4121ff5d15a6699e67a00019ef5d0719441ba7`. |
| Correction reviewed executable/script digest | `47ce93e78e591b7f35925d545452b08ce0aa5111d4d1b9660c33d4bbc3e24190`; complete reviewed patch SHA-256 `aed14dd28fa7113992c88211a45bf087290c8185b269a5f1a7faac6494a93fcb`; reviewed tree `dc51dbdae67642d8f3e7710ce5b17c89b8c81156`. |
| R12 review/checkpoint | The sole full GPT-5.6 Sol/xhigh/fast review is complete with five accepted findings at overall confidence `0.98`; all five are corrected and dispositioned below. The sole narrow correction review confirmed four corrections and raised one premise-invalid test-only-path claim, rejected with exact module-chain evidence below. No third review is permitted or warranted. The exact 24-path item tree is committed as one NNC5.5 checkpoint; no push or PR. |

## Full Review And Correction Disposition

The sole full structured review used GPT-5.6 Sol at `xhigh` reasoning in fast
mode over staged tree `0ada7d9bf082c8f3fc6ff60afbda8881b8981d72`.
Review thread:
`019fb9ae-d3fa-71e3-abb2-343c1cbe8145`. TruffleHog was clean. The
review produced five findings, all accepted because they directly weakened a
written NNC5.5 criterion:

| Finding | Fail-before evidence | Correction and proof |
| --- | --- | --- |
| P2 — HTTP readiness accepted EOF or buffer exhaustion without a complete status-line terminator | The exact real-HTTP test exited `101`: `0` passed, `1` failed, `938` filtered after adding an unterminated `HTTP/1.1 200 OK` response. | Readiness now requires an observed newline before parsing. EOF or 256-byte exhaustion without it is NotReady. Exact regression `1/1`; full readiness `10/10`; affected `1,156/1,156`. |
| P2 — an initial child-termination error could drop cleanup/reap ownership | The injected termination-closure test exited `101`: `0` passed, `1` failed, `938` filtered because `waitpid` returned the child PID instead of `-1/ECHILD`. | The wait helper transfers the owned child to a named detached reaper before returning; spawn/send failure recovers the child and synchronously kills, waits, and drains it. Exact regression `1/1`; egress-pin `12/12`; affected `1,156/1,156`. |
| P2 — NNCV022 scanned generic `Command::new` only in an existing effect whitelist | The full review demonstrated the false-negative path. A source-derived production fixture at Container runtime status now supplies the exact adversarial placement. | The census scans every production sandbox source and admits commands only at seven exact effect-owner paths. `command-outside-owner` exits `1` with only NNCV022 failing; isolated contract `27/27`; aggregate `139/139`. |
| P2 — NNCV023 did not seal the lifecycle readiness wrapper | The full review demonstrated that `attachment_lifecycle.rs` could regain provider/apply authority while the prior checked file stayed unchanged. | The seal scans both the readiness implementation and lifecycle wrapper; both require the observer and forbid provider/apply. `readiness-wrapper-apply-authority` exits `1` with only NNCV023 failing; isolated contract `27/27`; aggregate `139/139`. |
| P3 — NNCV004 omitted requested-feature and default-feature policy | The full review demonstrated the false-negative dependency profiles. | The exact core profile requires no requested features and defaults enabled; every approved external dependency records and verifies its default policy. `core-feature`, `core-no-default`, and `serde-no-default` each exit `1` with only NNCV004 failing; isolated contract `27/27`; aggregate `139/139`. |

The first combined isolated-contract invocation accidentally sourced Bash
arrays under Zsh and stopped after six dependency mutations with `bad
substitution`. That was a harness-shell invocation error, not a product or
verifier failure. The same command was rerun explicitly under Bash and passed
all `27/27`.

Because all five accepted findings materially changed executable Rust or
verification code, the cadence permitted exactly one narrow correction review
focused on these five defects after the correction candidate was staged. It
does not permit another full review or a review for later proof/ledger wording.

### Sole narrow correction review

The sole narrow review used GPT-5.6 Sol at `xhigh` reasoning in fast mode over
tree `dc51dbdae67642d8f3e7710ce5b17c89b8c81156`. It confirmed four
corrections and raised one P2 claim: that NNCV022 omitted a production
`container/runtime/lifecycle.rs` source.

That finding is rejected because its premise is false:

1. `crates/nimbus-sandbox/src/backends/container/runtime.rs` declares
   `mod tests` only under `#[cfg(test)]`.
2. `crates/nimbus-sandbox/src/backends/container/runtime/tests.rs` is that
   test-only module and is the sole file that declares
   `#[path = "lifecycle.rs"] mod lifecycle`.
3. `container/runtime/lifecycle.rs` contains test imports, fixture modules, and
   an ephemeral loopback-listener helper. It has no production module path.
4. NNCV022 deliberately excludes test-only source from its production effect
   census, as required by the written boundary. Including this file would
   misclassify test listeners as production effects.

No executable correction followed the rejected claim. This was the one
permitted narrow review; no third review ran.

### Modularity disposition

All new concept code remains below the repository threshold:

- `backends/readiness_probe.rs`: 560 lines;
- `scripts/verify-nimbus-network-source-contract.mjs`: 824 lines; and
- `scripts/nimbus-network-control-plane/effect-locality-contract.sh`: 179
  lines.

`scripts/verify-nimbus-network-control-plane.sh` is 1,628 lines and therefore
retains an explicit 1,500–1,999-line exception. It is the aggregate verifier
composition root accumulated across NNC0-NNC5: this item adds only named
NNCV022/NNCV023 orchestration and delegates all locality analysis and all 22
initial mutations plus five review regressions to the two concept-owned files
above. It owns no product behavior, provider effect, state machine, or inline
NNC5.5 analysis. Further condition logic must continue in concept-owned
helpers rather than accumulate in this root. The pre-existing 2,105-line
portable state store remains NNC5.2b's separate deep-module exception.

## Recovery Ledger

| Checkpoint | State | Evidence / next action |
| --- | --- | --- |
| Source | `done` | clean `8fe2ccfb5` / `bfa6318b367d`; three bounded read-only audits; portable edge and 22/22 aggregate verifier green |
| Audit | `done` | complete effect/caller and substitution census; probe duplication, bounded-wait, least-authority, visibility, and static false-green findings accepted |
| Acceptance freeze | `done` | R1-R12, exact mutations, owned paths, non-goals, cadence, and closeout gates recorded before executable edits |
| Fail-before | `done` | duplicate probe owner exits `1`; grouped TcpStream/Command, mount, umount2, upper-crate import, and portable forwarding trait each false-green with exit `0` |
| Implementation | `done` | one bounded sandbox-private readiness capability replaces two direct probe owners; nft observation is bounded and read-only; namespace effects are private; NNCV004/NNCV012/NNCV022/NNCV023 seal the result |
| Acceptance convergence | `done` | corrected behavior `1,156/1,156`; substitution `162/162`; readiness `10/10`; egress-pin `12/12`; live verifier `24/24`; isolated mutations `27/27`; aggregate mutations `139/139`; check/Clippy/rustdoc/format/diff/syntax/ShellCheck green; docs `108`; site `17/17` |
| Item review | `done` | sole full Sol/xhigh/fast review completed at `0.98`; five accepted findings corrected and proven; sole narrow review confirmed four and its one test-only-path claim is source-rejected; no third review |
| Exact checkpoint | `done` | exact 24-path item tree staged with no unstaged paths and committed once for NNC5.5; no push/PR |
