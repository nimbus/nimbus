# NNC9.2 — Offline Sovereign Lifecycle

Status: `complete — Run 67 passes K1-K14 twice on the final correction candidate`

Source checkpoint:

- local commit: `6cf0cefaa647ef5340d43ee69dc125ed80b3e1d6`;
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`;
- runner: `nimbus@192.168.4.29` (`minicloud`), Debian 13,
  `x86_64`, kernel `6.12.94+deb13-amd64`;
- remote proof worktree:
  `/home/nimbus/src/github.com/nimbus/nimbus-nnc92-preflight`;
- remote proof branch: `codex/nnc92-sovereignty-preflight` at the same source
  checkpoint.

## Candidate Closeout

Run 67 is the final lifecycle acceptance. Its two disjoint attempts pass
K1-K14 with the same authenticated 826,137,064-byte binary. Its SHA-256 is
`fd1a9030d69fe93c6fc3bd48168c4091c2bc4a2ac43ff2df7351a25fa1699984`.
The root-owned mode-`0500` candidate is at
`/var/lib/nimbus-nnc92-candidates/fd1a9030d69fe93c6fc3bd48168c4091c2bc4a2ac43ff2df7351a25fa1699984/nimbus`.
The warm two-job minicloud build finished in `29m31s` with Rust and Cargo
`1.97.1`. A checksum dry run after the correction sync reported no difference
across the complete executable and harness input set.

Each Run 67 attempt derives the exact 19 transitions, one automatic restart,
fresh-owner recovery, recorded retirement, terminal projection and provider
state, and endpoint refusal. Each attempt also records complete product and
outer cleanup. All six named network counters are zero. The DNS capture and
forbidden-address set are empty. The evidence result is `PASS` with exit `0`.
The lifecycle mutation harness passes `8/8`, and all ten deterministic
invariants pass. The live lifecycle must not run again unless executable
candidate code changes.

The candidate-frozen affected behavior is green:

- `nimbus-workloads`: `231` passed;
- `nimbus-compute`: `506` passed, `1` ignored;
- `nimbus-sandbox`: `1,212` passed, `47` capability-specific tests ignored;
- `nimbus-services`: `102` passed;
- serialized `nimbus-server`: `757` passed, `35` ignored;
- `nimbus-cli`: `1,011` passed, `4` ignored.

The final five-defect correction batch has direct regression evidence. The
provider journal accepts only an exact adjacent publication epoch. Foreground
cancellation proves that its tracked child settles without a timed sleep. A
second owner-reopened publication absence retains the original observation
authority, uses the latest adjacent dispatch absence, retries publication, and
reobserves. Endpoint refusal accepts only connection failure and rejects a
post-connect timeout. Teardown history requires every later observation list
to extend its prior prefix. The server's process-global network composition
requires serialized full-suite execution. The canonical serialized suite
passes in full.

Normal affected all-target compilation, strict Clippy, warning-denied Rustdoc,
Rustfmt, diff checks, durability `24/24`, Bash syntax, ShellCheck, Python
compilation, and lifecycle mutations `8/8` with ten invariants pass. A local
`--all-features` attempt correctly stopped at the repository V8 variant guard
because the shared target contained the non-pointer-compression archive. The
isolated minicloud all-feature lane used a separate target, as required by the
repository build contract, and passed all affected packages and targets in
`25m11s` with exit `0`. Every changed source, test, script, and fixture file was
byte-identical between the owner and isolated worktrees; only directory
timestamps differed. Rust and Cargo are already `1.97.1`; no toolchain update
was needed. The docs gate passes `108` link-clean pages, and the site gate
passes `17/17` conditions.

The aggregate static verifier passes `34/39`; its recovery-ledger check is
green. The five failures are not lifecycle failures: stale bind, composition,
modularity-count, provider-effect, and compiler-source inventories are explicit
NNC9.3 architecture-truth inputs. NNC9.2 does not rewrite those inventories or
historical proof records.

## Item Review Disposition

The full GPT-5.6 Sol/xhigh/fast item review reported seven findings. Its two
internal coverage passes reviewed one candidate-frozen item and were not
separate review cycles:

- source-rejected: the cancellation regression joins the exact caller task
  that owns the polling loop. No hidden polling task exists, so the requested
  post-join delay cannot prove more than the join already proves;
- accepted: `RetryAfterAbsence` loses the retained owner-reopened inspection
  origin after a second exact absence;
- accepted: the Krun process census can ignore a second same-sandbox conmon
  when its runtime path differs;
- accepted: the provider-journal proof treats missing, unknown, and malformed
  observation kinds as terminal;
- accepted: K4 trusts summary booleans instead of deriving the exact control
  assertions and reset counters;
- accepted: a TCP connection with a malformed HTTP response is treated as an
  unreachable retired endpoint;
- accepted: K10 proves final unreachability but does not authenticate the
  durable ordered `publication_absent` observation before
  `execution_stopped`.

The six accepted findings were corrected and proven. The one permitted narrow
correction review then accepted five defects:

- owner-reopened publication retry did not require every adjacent epoch;
- cancellation used elapsed time instead of the tracked child state;
- republication retry discarded durable observation lineage;
- endpoint refusal treated a post-connect timeout as a connection failure;
- teardown history did not require each later observation list to extend the
  prior prefix.

All five narrow-review findings are corrected and proven by the focused tests,
Run 67, and the affected closeout gates. The review cadence is exhausted. No
third review or additional audit is required. NNC9.3 owns the five stale
aggregate-verifier inventories and the plan/proof compression work.

The superseded correction candidate was the root-owned mode-`0500`,
826,004,184-byte binary at
`/var/lib/nimbus-nnc92-candidates/ea37b304ebb2cfe993d3084b6493afd8a5e40215c4158bbcac46db4d6d6e0537/nimbus`.
Run 65 reached terminal product teardown. Its durable document versions record
`publication_absent` at commit 83 and `execution_stopped` at commit 91 for the
same restarted execution. The new K10 verifier failed closed because it
compared those observations with the superseded pre-restart execution. The
bounded harness correction uses the authenticated restart saga as the teardown
source. The `8/8` mutation cases and ten invariants pass, including rejection
of the superseded execution, and an exact replay over Run 65's 17 teardown
versions returns true. Run 65 is preserved failure evidence. The executable
candidate did not change for Run 66. Run 67 supersedes it with the final
five-defect correction and complete two-attempt acceptance.

## Owner Boundary

NNC9.2 owns a test-only lifecycle adapter above the existing NNC4.7
sovereignty tripwire. The adapter may compose existing CLI, services, compute,
server, sandbox, and provider-effect owners. It must not add a production
provider interface, change product authority, or move sockets, netns, nft,
KVM, service naming, policy, forwarding, projection, workload coordination, or
cluster transport into `nimbus-network`.

The first real run may correct a directly proven defect in an existing
concept-owned lifecycle seam when that defect prevents a frozen criterion. The
correction must keep current authority, add fail-before evidence, and remain
limited to the failing seam. The K5 runtime-selection correction meets that
rule and is live-proven. Run 24 isolates the remaining K6 defect: libkrun
creates its TSI listener in the sandbox network namespace, but Krun readiness
and route data still describe the guest port. The existing server-owned
workload-ingress adapter must bind the host listener and forward to one
provider-private Krun endpoint. Socket ownership does not move.

The NNC4.7 empty-profile mode and its historical evidence remain valid. The
NNC9.2 mode must reuse the same positive-control, counter-reset, isolated
namespace, DNS-capture, syscall-trace, and exact-cleanup boundary. Lifecycle
logic belongs in a small concept-owned child, not in the already-large NNC4.7
isolation or evidence modules.

The target workload uses a pre-staged BusyBox root filesystem and a local
`FROM scratch` Compose build. Nimbus's existing OCI builder materializes this
root without a registry, package manager, install, fetch, or pull. The real
Compose path must select the local Krun provider and drive services, compute,
the durable workload saga, sandbox effects, and server-owned ingress.

## Frozen Acceptance Contract

The item is complete only when all K1-K14 conditions pass.

| ID | Requirement | Exact proof |
| --- | --- | --- |
| K1 | Source and input identity are authenticated. | Evidence records the exact Git commit/tree, harness digest, Nimbus binary SHA-256, BusyBox SHA-256, Compose/Dockerfile SHA-256, and every executed argv. A changed or missing input fails before lifecycle effects. |
| K2 | The named runner is suitable. | Preflight records hostname, OS, kernel, architecture, UID, effective capabilities, `/dev/kvm`, Rust/Cargo, crun/libkrun, conmon, Buildah, Netavark, Aardvark, nft, tcpdump, strace, routes, resolver state, and tool paths/versions. Missing required capability or tool is `SKIPPED` (`77`), never PASS. |
| K3 | The proof is offline after admission. | All binaries and workload inputs are staged before the boundary. The lifecycle command performs no package install, Git/Cargo fetch, registry pull, or other download. Command and syscall evidence contains no such operation. |
| K4 | Detection works before each lifecycle. | Both attempts pass the NNC4.7 UDP/TCP DNS, unenumerated-private, public IPv4, and public IPv6 positive controls, then prove an exact zero counter reset. |
| K5 | Provider selection fails closed and is exact. | `nimbus compose config` admits one Krun service; the runtime capability report selects the declared local Krun attachment plus Nimbus-owned ingress bundle with local-only control-plane and offline-restart semantics. No first-available fallback occurs. |
| K6 | Start reaches truthful readiness. | Real Compose provisioning records one stable tenant-qualified service/sandbox identity and generation, reserves before provider effects, attaches before activation, and publishes only after the workload is ready. |
| K7 | Private serving and logical lookup work. | The named service resolves through the services-owned lookup and serves HTTP through its loopback/private published endpoint. The response and exact endpoint identity/generation are recorded. |
| K8 | Restart uses the compute saga. | A deliberate first guest exit triggers exactly one automatic restart. Stable workload/service identity and generation remain constant, the execution attempt changes, ingress is withdrawn before restart stop, and the second guest serves successfully. Inspection performs no restart effect. |
| K9 | Fresh-process reconciliation is exact. | After the first foreground owner exits, a fresh Compose owner opens the same durable roots, reports the service as already running, reconstructs the current attempt, and creates no duplicate VM, attachment, listener, or lease. |
| K10 | Withdrawal fences lookup before stop. | Compose retirement removes the services-owned routable binding before execution stop. A concurrent or post-withdrawal lookup cannot return the retired endpoint, and the published port stops serving. |
| K11 | Detach, release, and teardown are exact. | Retirement reaches stopped/recorded state, removes the provider attachment and owned ingress effects, releases host-port and segment authority only after confirmed cleanup, and leaves no run-owned process, netns, interface, nft table, listener, or reusable quarantined claim. |
| K12 | The lifecycle makes no unexpected network attempt. | After passing controls, both lifecycle attempts finish with zero unexpected DNS capture, zero denied-private/public/output counters, and no forbidden address in the complete descendant network syscall trace. Allowed traffic is limited to loopback and explicitly enumerated private provider ranges. |
| K13 | Evidence is complete and reproducible. | The bundle records exact commands/exits, timestamps, logs, capability report, topology/routes/resolver, nft rules and counters before/reset/after, syscall traces, lifecycle transitions, cleanup observations, artifacts with hashes, and all skips. Evidence validation fails closed on a missing, altered, crossed, or contradictory artifact. |
| K14 | Re-entry is deterministic. | Two same-input attempts use disjoint durable roots, pass K4-K13, and prove exact cleanup before the second attempt. Deterministic self-tests cover admission failure, payload failure, timeout, nonzero counter/DNS evidence, altered input, missing transition, incomplete cleanup, and PASS-evidence mutation. |

## Preflight Facts

Read-only checks at the source checkpoint established:

- passwordless sudo and read/write KVM access;
- Rust/Cargo `1.97.1`, so no system Rust update is needed;
- Nimbus libkrun-enabled crun `1.27.1-dirty` at
  `/usr/libexec/nimbus/crun`;
- Buildah `1.39.3`, conmon `2.1.12`, Podman `5.4.2`, Netavark
  `1.14.0`, Aardvark `1.14.0`, nftables `1.1.3`, and strace are present;
- tcpdump `4.99.5` was installed before the proof boundary;
- the remote source worktree is clean and does not modify the existing clean
  `fu7-main` checkout;
- 315 GB is free, with four CPUs and approximately 8 GB RAM plus 8 GB swap.

The preflight build completed with Rust/Cargo `1.97.1`, two build jobs, and
development debug information disabled. The resulting `nimbus 0.1.45` binary
has SHA-256
`e961195b334ab96705d7b9c51f8dc353e5f9c1f98078daa7c6a431e6110e0077`.
The exported BusyBox root inventory has SHA-256
`0265b71dadb8b03964276e2079c6686069791288be1309d74fd2d9d4db2b3841`.

The preflight build uses the existing shared Cargo target. It is preparation,
not lifecycle evidence. The final candidate must be rebuilt or authenticated
from the final reviewed source and then staged before the offline boundary.

The final-source remote worktree now matches the explicit local product and
fixture paths. An optimized development build with debug information disabled
completed from the warm shared target with two jobs. The resulting
800,502,392-byte candidate at
`/home/nimbus/src/github.com/nimbus/nimbus/target/debug/nimbus` has SHA-256
`82e6483fff675c2881a9e088101d5bd039785c8db49f8d062f7be2eab018077a`.
This candidate supersedes every earlier diagnostic binary. It is prepared
input, not K1-K14 evidence.

After the final provider-stream and Krun lifecycle-fence corrections, the
complete remote source rebuilt successfully with Rust/Cargo `1.97.1`. The
resulting 801,243,040-byte binary has SHA-256
`1267e42f7ac41d22e4dc9d89d636e633bc7c759c90c7f195e4b8c89b9794b0d6`
and is staged root-owned mode `0500` at
`/var/lib/nimbus-nnc92-candidates/1267e42f7ac41d22e4dc9d89d636e633bc7c759c90c7f195e4b8c89b9794b0d6/nimbus`.
This is the sole candidate for Run 40 effect cleanup and the next complete
K1-K14 lifecycle. It is still prepared input, not acceptance evidence.

## Current Result

The acceptance contract stayed frozen throughout diagnosis. Run 64 now claims
K1-K14. All earlier diagnostic roots remain retained. They proved sequential
K6-K11 defects without crossing authority boundaries:

- the first provider-journal claim could not establish a deeply nested Krun
  state root; the exact fail-before regression and symlink refusal now pass;
- Krun reservation tried to resolve OCI image defaults before preparation;
  sparse reservation now preserves those inputs and its exact regression
  passes;
- after local image preparation succeeded, foreground Compose rejected the
  compute saga's bounded `attach_network` waiting result before exact resume.
- after foreground convergence retained that wait, Run 7 traced Netavark setup
  and compensating teardown to `ENOENT`: direct Compose/Krun searched only its
  sanitized `PATH`, while the pre-staged provider binary remained at the
  canonical `/usr/lib/podman/netavark` helper path;
- after canonical helper discovery succeeded, Run 8 authenticated Netavark,
  IPAM, active attachment authority, the PEP lease, and the exact nft pin;
- Runs 17-19 then located the retry in exact attachment inspection and decoded
  `EgressPinNotReady`. At the live classifier boundary, loopback returned its
  expected class while the nft 1.1.3 established/related rule returned `None`.
  The production command is `nft -j -nn`; GDB proved its parsed elements are
  the kernel state bits `2` and `4`, while a manual command without `-nn`
  rendered symbolic strings. Namespace inspection at the same stop showed
  exactly one default-drop table and the required three rules. The exact
  numeric fixture reproduced the fail-before reason: `nft pin chain contains
  a duplicate, substituted, or unrecognized rule`.
- Run 20 proved the strict numeric correction clears attachment readiness and
  reaches guest launch. Runs 20-22 also exposed fixture-only assumptions about
  BusyBox command paths and guest process privileges. The fixture now uses
  only authenticated BusyBox applets and needs no privileged guest signal.
- Run 23 starts the guest and records `first-ready` in its root filesystem.
  The exact runtime object is running, but `127.0.0.1:15992` has no listener.
  Its manifest and conmon argv authenticate `/usr/bin/crun` as the selected
  runtime even though the annotation requests the Krun handler and port map.
  Stock `/usr/bin/crun` reports no `+LIBKRUN`; the installed Nimbus runtime at
  `/usr/libexec/nimbus/crun` reports `+LIBKRUN`. The
  `KrunSandboxBackendConfig` default already selects the private runtime, but
  the new shared helper resolver replaces it with the first executable
  `crun` on `PATH`.
- The exact runtime-selection correction is live-proven in Run 24. Conmon
  invokes `/usr/libexec/nimbus/crun`, the runtime reports `+LIBKRUN`, and the
  guest records `first-ready`. The minicloud binary has SHA-256
  `422309df1f9fc3735d346f758808b457aec4360a68652b644a374dfcc84e1435`.
  The runtime process is in `net:[4026533989]`, where `libkrun VM` owns
  `[::ffff:127.0.0.1]:15992`; the host is in `net:[4026531840]` and has no
  listener on port 15992. This proves that the annotation is realized in the
  provider namespace. It must be a private bridge for the existing
  `nimbus-server` host listener, not a second host-publication authority.
- The exact Krun private-ingress assertions fail before (`0/2`): the map uses
  desired host addresses instead of the provider-namespace wildcard, and the
  upstream selector returns guest port 8080 instead of reserved bridge port
  18080. After the narrow correction, those assertions plus duplicate private
  wildcard-port rejection pass (`3/3`). The default-versus-provider target
  contract passes (`1/1`): Container/default routing keeps the guest port,
  while Krun selects the provider bridge port without changing the
  authenticated attachment address. All 28 Krun bundle tests and the sandbox
  crate check pass.
- Run 25 authenticates binary
  `1e6769284c3e07fa1a35834bc0bc551f303d59527122e2cc0058b6363d6a35b`,
  annotation `0.0.0.0:15992:8080`, and provider listener
  `[::ffff:0.0.0.0]:15992` in `net:[4026534049]`. The host route to assigned
  `10.0.0.2` is present, but an exact request to `10.0.0.2:15992` is refused
  and the guest remains `first-ready`. The reserved manifest has no assigned
  address; authenticated Netavark evidence supplies it after attach. The next
  correction therefore finalizes the bundle idempotently with that exact IPv4
  address after attachment and before creator intent/spawn. It does not change
  phase order or host-listener ownership.
- The exact assigned-address finalizer is now locally proven. Private-TSI
  coverage passes `5/5`, Krun provision phases pass `13/13`, all 28 Krun
  bundle tests pass, and sandbox check, Rustfmt, and diff checks pass. Run 26
  authenticates binary
  `d9651d670891f2eea9cf571dba0991daded379362c126455615e3a39e2a978ae`
  and annotation `10.0.0.2:15992:8080`. A request inside its exact provider
  namespace reaches the listener and returns `first-ready`; no host listener
  is published while foreground readiness remains pending.
- Run 27 isolates that remaining refusal as stale proof-host state. Twenty
  retained NNC9.2 namespaces map one-to-one to host `veth0` through `veth19`,
  are enslaved to `nb-0`, and all claim `10.0.0.2`. Host neighbor evidence
  selected an older run's MAC, while Run 27's exact namespace listener was
  healthy. The exact twenty run-owned attachments and empty `nb-0` bridge were
  removed without touching unrelated minicloud state.
- Run 28 then reached saga phase `Ready` and step `Publish`; private TCP
  readiness succeeded and the exact Krun object stopped because the fixture's
  one-shot listener treated that readiness connection as its public request.
  This was fixture behavior, not a product defect. The fixture now accepts one
  provider-private readiness connection before serving one published HTTP
  connection; POSIX shell syntax, ShellCheck, and the fixture diff check pass.
- Run 29 with that fixture reaches `Ready` and `Publish`, keeps the exact Krun
  object running with restart count zero, and waits for the published request.
  Publication records ambiguous evidence digest
  `7186202dc88ea5816d0803b583528dc4ba6a85293b85c77894f07b5ddf7bbf38`
  before any host-listener claim: lease `netportlease_3F32K6AQPR3E22E6FFM23ZA0KT`
  remains `Reserved` with its exact launch claim, no bind or adoption claim, no
  binding, no active lifetime, and lifetime generation zero. The complete
  planned-listener control is green (`1/1`), so `nimbus-network` is not the
  correction seam. A replay under GDB changed only the run-owned provider
  journal from `ambiguous` to `in_progress`; durable network authority stayed
  at revision 16.
- Runs 30-31 stop at the exact `nimbus-server` ambiguity constructor before the
  journal hashes its evidence. The backtrace places the failure in
  `KrunSandboxBackend::inspect_provision_server_ingress_targets`, and Run 31
  captures the exact text: `krun workload ... requires reserved launch
  authority before provider adoption, got ProviderOwned`. Activation correctly
  transfers the manifest state to `ProviderOwned`; the durable network config
  still carries the exact launch claim needed for the server listener handoff.
  The focused regression fails before (`0/1`) with that exact error and passes
  after (`1/1`) when inspection requires `ProviderOwned` and reads the claim
  from the authenticated network config. The provision-phase aggregate passes
  `32/32`, sandbox check passes, and Rustfmt and diff checks pass. No
  `nimbus-network` or server listener authority changed.
- The corrected minicloud build completes from the final local source and
  authenticates the 3,059,784,416-byte debug binary as SHA-256
  `1749d0f27b729e38a6cec112519a73089205f2eefabf11f153ebd481d8ac0c23`.
  Run 32 starts from a fresh durable root with the unchanged authenticated
  fixture, but it supplies a distinct pre-launch result: 600 bounded host
  probes receive no response, `attach_network` remains durably `in_progress`,
  IPAM remains `setup_prepared`, and the manifest retains `ProviderOwned` with
  creator handoff `not_spawned`. The foreground owner accepts cancellation and
  exits zero. Fresh startup inspection finds no provider status, process,
  listener, interface, mounted namespace, or nft table and converts the exact
  attachment to `CleanupPending`; it does not retry an ambiguous effect or
  release the retained leases. This does not reproduce the corrected Run 31
  server-ingress failure. Run 33 repeats the same invalid unprivileged runner
  contract and is interrupted by an intrusive GDB pause; neither run supplies
  K6 acceptance evidence.
- Run 34 uses the required passwordless-sudo boundary. The invoking identity is
  `uid=1001(nimbus)` and the candidate process is `uid=0(root)` with effective
  capability mask `000001ffffffffff`. Binary
  `1749d0f27b729e38a6cec112519a73089205f2eefabf11f153ebd481d8ac0c23`
  reaches truthful first readiness after 137 bounded probes, serves
  `first-ready`, and exits its foreground owner with status zero. The fixture
  records `first-exited` and creates its restart marker. During owner shutdown,
  `nimbus-server` reports that plan
  `netplan_5FW7KG0AEJZ6E4B5YFDQRGCCTK` requires its complete immutable durable
  member set while settling the workload-ingress listener. Two fresh,
  identically privileged Compose-down attempts both exit one with `workload
  provision settlement remains pending`. No host listener remains. The exact
  durable root, attachment authority, `nb-0`, and `veth0` remain retained, so
  cleanup failure does not permit reuse. This is an exact K8/K9/K11 convergence
  defect. The next action is a deterministic server regression and seam-local
  correction; no further live run is permitted before focused and affected
  gates pass.
- The exact owner-exit regression proves the complete-plan defect without a
  live provider. Before the correction it fails `0/1`: dropping one published
  batch leaves its planned listener `Active` instead of retained `Reserved`.
  The correction keeps the immutable plan witness in `RunningIngressBatch`,
  stops and joins its server-owned route subset, atomically retains that subset
  for exact-port rebind, and preserves an unrelated host-internal member. A
  route-level fallback never submits a scalar transition for a planned lease.
  The regression passes `1/1`, and the workload-ingress aggregate passes
  `21/21`. The first full-server attempt is invalid because 44 tests correctly
  reject concurrent process-global authority composition. The required
  serialized suite passes 659 tests before one unrelated listener-projection
  timing assertion, with 35 ignored; the exact failed test passes `1/1` on its
  immediate isolated rerun. Rustfmt and diff checks pass. A full serialized
  green run remains due in the final affected gate.
- The concept-owned lifecycle adapter and fail-closed evidence validator pass
  all eight frozen mutation cases locally and on minicloud. Run 35 rejected an
  incorrect Docker-style config assertion before lifecycle effects. Run 36
  preserved the unprivileged positive-control boundary and rejected a probe
  path that the dropped identity could not traverse. Both defects were
  corrected in the adapter, and both runs removed their exact outer resources.
- Run 37 passed config admission, positive controls, and exact counter reset,
  then retained `starting` with zero endpoints for the 300-second bound. Its
  OCI log reports `invalid file system type on /sys/fs/cgroup` for every exact
  start retry. A bounded diagnostic proves the host view is `cgroup2fs`, but
  `ip netns exec` remounts `/sys` and presents `sysfs` at that path. Entering
  only the same network namespace with `/usr/bin/nsenter --net=...` preserves
  `cgroup2fs` and the exact network namespace identity. This is a harness
  execution-environment defect, not a product defect. Product lifecycle
  commands now use the authenticated network-only entry path. Each attempt
  records and requires equal supported host/subject cgroup filesystem views
  before config or provider effects. Local Ruff, Python compile, Bash,
  ShellCheck, diff, and `8/8` mutation checks pass; remote Python compile,
  Bash, and `8/8` mutations pass. Run 37 cleaned both namespaces and root
  veths exactly.
- Run 38 crosses the corrected cgroup boundary and creates the exact Krun
  runtime. The guest then reports `Couldn't execute '/bin/busybox' inside the
  vm: Permission denied`. Source BusyBox is `0755`; root-owned staging had
  reduced executable inputs to `0500`, and the mapped guest identity could not
  execute them. Staged BusyBox and `lifecycle.sh` are now root-owned,
  immutable `0555`; every staged fixture mode is recorded and revalidated at
  PASS. An independent remote staging proof authenticates all eight modes.
  Run 38 also exposed failure-evidence collection trying to hash a retained
  mounted `nsfs` handle. Failed runs now record that exact unreadable path,
  size, and read error with no digest instead of losing the primary failure;
  PASS evidence still requires strict hashes for every artifact. A
  deterministic simulated `EINVAL` regression passes `1/1`. Run 38 cleaned
  both outer namespaces and root veths exactly.
- Run 39 confirms that the remaining `EACCES` is the dynamic ELF interpreter:
  BusyBox is `0555`, but staging changed source
  `ld-linux-x86-64.so.2` from executable to `0444`. Staging now derives each
  exact file mode from its source executable bits, removes all write bits, and
  authenticates the result. A recorded pre-effect smoke command invokes the
  staged interpreter, staged library path, and staged BusyBox. The independent
  remote proof passes all eight modes plus executable closure (`8/8 + 1/1`).
  Run 39's failed evidence preserves 1,463 artifact rows, including the one
  unreadable retained `nsfs` handle, and its outer cleanup passes.
- Run 40 uses the unchanged 800,502,392-byte candidate SHA-256
  `82e6483fff675c2881a9e088101d5bd039785c8db49f8d062f7be2eab018077a`.
  It crosses every earlier adapter defect: the real Krun VM runs, the guest
  records `first-ready`, the network is attached, the server ingress listener
  is active, and the durable provision saga reaches `Published`. The run then
  fails closed after the 300-second bound because the manifest remains
  `starting` with zero projected endpoints. Exact provider journals show
  `Publish` succeeded while `ObservePublication` remains `in_progress`.
  Source reduction isolates the cause: the server publication registry keys
  `PublicationKey.attempt_id` from each step-specific
  `WorkloadProvisionAttemptId`; publish and observation are separate steps and
  therefore have different IDs. Both commands retain the same stable sandbox
  execution-attempt identity. The next action is one deterministic fail-before
  regression, replacement of the step identity with that stable execution
  identity, the focused server gate, and one corrected complete run. Evidence
  is retained at `/var/lib/nimbus-nnc92-proof/run40/evidence.json`; its result
  is `FAIL`, phase `lifecycle`, finished `2026-08-15T01:17:38.949640+00:00`.
- The exact regression
  `publication_and_observation_commands_share_stable_registry_identity`
  fails before `0/1`: its `PublicationKey` values differ only by the Publish
  and ObservePublication `wpa_*` IDs. Both commands authenticate the same
  generation-scoped execution and sandbox execution attempt. The seam-local
  correction keys the existing server registry by that stable execution
  attempt. The exact regression passes `1/1`; the serialized
  `workload_ingress::` aggregate passes `22/22`; Rustfmt passes. No provider
  effect, policy, service-name, proxy, projection, network-crate, or cluster
  authority changes.
- The warm minicloud rebuild produces an authenticated 801,198,560-byte
  candidate with SHA-256
  `5526291c26a5b37630dc72ca9c2c776de9d1ca9ea2d9b4330b65583d64d0a226`.
  Retrying Run 40 cleanup with that exact binary reaches the durable provider
  inspection without binding or restarting. It also exposes the next direct
  K9/K11 condition: a process-bound publication that disappeared with its
  owner process remains recorded as successful, while publication observation
  reports `in_progress`; the queued stopped successor therefore fails closed
  with `workload provision settlement remains pending`. One exact regression
  must prove that provider-authenticated absence either schedules the existing
  publication at its next fenced epoch or, when a stopped successor is already
  durable, commits the absence directly into teardown. Inspection must remain
  effect-free in both cases.
- The exact fresh-owner settlement regressions now pass `3/3`. A running
  generation records provider-authenticated publication absence, republishes
  once at the adjacent epoch, and re-observes on the stable execution-attempt
  provider stream. A stopped successor does not wait on the inspection-only
  readiness observation; it enters final withdrawal, whose existing ingress
  owner still proves live or absent state. Same-phase evidence replacement is
  legal only on that authenticated republish-to-reobserve edge and can replace
  only the final `PublicationPresent` evidence for the same reference.
- Authenticated transient candidate
  `b336dc397bb243969517539f998615ad195d9e2fb4f2ee1166b7143fd3177ba2`
  (801,236,600 bytes, root-owned mode `0500`) advanced Run 40 through provision
  settlement and durable Drain. Stop then failed closed as
  `sandbox_teardown_command_crossed`. The durable Drain and Stop journals have
  the same saga, workload generation, execution subject, desired/source/network
  digests, and provider target, but correctly have distinct `wtd_*` step
  attempt IDs. Krun alone incorrectly required those command-local IDs to
  match; Container already compared the stable lifecycle fence. The corrected
  Krun regression passes `1/1`, while a crossed desired digest still fails
  before a signal or write. Run 40's terminal failed evidence remains retained;
  it is not acceptance evidence and must not be rewritten.
- Run 48 crossed restart settlement, drain, and stop, then exposed that an
  `AuthenticatedAbsent` publication observation had downgraded durable prior
  `PublicationPresent` authority to a `Ready` teardown origin. Final withdrawal
  was therefore treated as resource-free, the published port lease remained
  active, and exact detach failed closed. The pure reducer regression is exact
  `0/1 -> 1/1`; it now requires `Published`, emits a final-withdrawal claim, and
  rejects a forged `Ready` downgrade. The complete workloads crate passes
  `230/230`; compute settlement passes `1/1`; server final withdrawal passes
  `7/7`; the two restart-settlement server proofs pass `1/1` each; Rustfmt and
  diff checks pass. Run 48 already persisted the superseded origin and cannot
  be safely rewound, so its root remains negative evidence.
- The corrected minicloud build completed in `30m48s` with Rust/Cargo `1.97.1`.
  Its raw binary is 3,059,752,040 bytes with SHA-256
  `995f6a958d1d1e59ccbfae8b57d08ba130991f1eb38f67837dbda7f031c5df7e`.
  The authenticated debug-stripped candidate is 825,786,680 bytes, root-owned
  mode `0500`, and staged at
  `/var/lib/nimbus-nnc92-candidates/00d70227da9cae48cd22a14fe7912ddfcd5ee063fb50b364d521dd8f610091ae/nimbus`.
  Fresh Run 49 was the next acceptance action.
- Run 49 crossed every prior lifecycle defect. Initial readiness and HTTP,
  deliberate guest exit, automatic restart, restarted HTTP, and the exact
  durable restart-saga wait all passed. The K9 foreground handoff then timed
  out because parent-mode `strace -ff` continued to wait for the intentionally
  surviving conmon/guest descendants after the Nimbus tracee had stopped.
  Trace PID `678046` proves Nimbus received SIGINT at
  `1786803740.050908` and exited `0` at `1786803740.669061`; product shutdown
  took `0.62s`. A direct minicloud control measured the same parent tracer at
  `2039ms` for a two-second surviving child and detached `strace -D` at `7ms`.
  The deterministic harness invariant fails before `0/1` and passes `1/1`.
  Foreground product tracing now uses `strace -DDD -ff`: the tracer has its own
  session and can retain descendant network evidence while the subprocess
  handle follows only the Compose owner. The eight frozen evidence mutations
  remain `8/8`; Python compilation and diff checks pass. No product source or
  candidate changed. Evidence is retained at
  `/var/lib/nimbus-nnc92-proof/run49/evidence.json`.
- Run 50 proves the detached tracer correction and reaches the real K9 owner
  handoff. The first Compose owner stops; a fresh owner opens the exact same
  durable roots and provider realm. Its HTTP probe then times out after the
  bounded `300s` because public port `15992` is not restored. Durable source
  evidence remains the exact restarted attempt at phase `Observed`, completed
  restart epoch `1`, with all seven ordered provision observations, no active
  restart, and no nonterminal provider journal. Source reduction identifies
  the missing route: compute startup sends this state through normal provision
  resume, whose `Observed` decision is `Wait`, so it never schedules
  side-effect-free publication inspection.
  The server owner-exit seam has already retained exact confirmed-stop binding
  and complete plan authority; provider-authenticated absence can therefore
  authorize one fenced rebind without moving the socket effect from server.
  Run 50 cleanup returned one truthful `Waiting`, then one evidence-based retry
  reached `Recorded`. No run-owned process, listener, namespace, interface, or
  detached tracer remains. Evidence is retained at
  `/var/lib/nimbus-nnc92-proof/run50/evidence.json`.
- The Run 50 correction is an exact compute-owned startup mode, not a change to
  ordinary provision resume. The route regression is `0/1 -> 1/1`. The fresh
  owner first commits an explicit `owner_reopened_publication_inspection`
  authorization, retains all publication identity and `PublicationPresent`
  evidence, removes only the final process-local `PublicationObserved`
  observation, and invokes provider inspection. Exact absence then uses the
  existing fenced republish and reobserve protocol. The complete call order is
  `ObservePublication/Inspect`, `Publish/Execute`,
  `ObservePublication/Inspect`; ordinary `Observed` resume invokes zero
  provider calls. Ambiguous-after-apply adoption, ambiguous-before-apply retry,
  and a crash after the inspection claim all remain inspection-first. The pure
  wire rejects a forged ordinary authorization. Portable workloads pass
  `231/231`; compute passes `497/497` with one child-process ignore; startup
  routes pass `8/8`; workloads, compute, and server all-target checks pass.
  Rustfmt and diff checks pass.
- Run 51 used authenticated 825,819,408-byte candidate
  `c640eec3022a94eccb1f91e7a647c39bc744e6fcef00c868fdc62fa6a51a6113`.
  It crossed every earlier boundary but timed out for `300s` at the same K9
  fresh-owner HTTP probe. The durable saga stayed revision 40
  `Observed/Ready`, with completed restart epoch 1 and all seven observations;
  no owner-reopen transition was committed. The exact source fact is its
  indexed `recoveryEligible: false`: startup routing was correct, but the
  durable store did not select this record. The portable eligibility
  regression and fresh-Engine recovery-page regression each fail before
  `0/1` and pass `1/1`. `requires_recovery` now includes only the existing
  exact owner-reopened publication predicate, so withheld/quiescent records
  remain excluded and the current `by_recovery` index remains the sole query
  authority. Failed-run cleanup returned one truthful `Waiting`, then one
  evidence-based retry reached terminal `Recorded` at revision 61. No live
  Run 51 process, listener, outer namespace, interface, provider status, or
  retained netns remains. Its `stopping` manifest remains preserved as
  negative durable evidence. K1-K14 remain unclaimed; next is affected gates,
  one authenticated corrected candidate, and fresh Run 52.
- Run 52 used authenticated 825,819,408-byte candidate
  `24370a0f08e56754db94d812b2fa0869ec1dc3d18f1020d4082726ef393d22ff`.
  The fresh owner selected the exact recovery-eligible revision 40 `Observed`
  record and durably committed its inspection claim at revision 41. Exact live
  inspection found the process-bound ingress absent, but the provider journal
  rejected that result because `ObserveIngress` did not permit live-absence
  reconciliation. The sandbox journal and compute-adapter regressions each
  fail before `0/1` and pass `1/1` after that existing reconciliation contract
  includes `ObserveIngress`. Product cleanup removed every live effect and
  reached `Recorded` at revision 62, then reported the second exact defect:
  direct promotion from the recorded running generation to `Stopped` discarded
  retained terminal evidence. The workloads regression fails before `0/1`
  with `promoted generation must enter its exact initial phase` and passes
  `1/1` after direct promotion uses the existing evidence-preserving promotion
  constructor. Run 52 remains preserved as negative evidence. K1-K14 remain
  unclaimed. The three exact regressions pass `3/3`; the combined full
  workloads/sandbox/compute test command, their affected all-target checks,
  Rustfmt, and the diff check pass. The five corrected Rust paths match the
  minicloud proof worktree byte-for-byte. The corrected candidate was built,
  authenticated, and exercised as Run 53.
- Run 53 used authenticated 825,819,408-byte candidate
  `1b5b1017328de5d3cf30ce368b299fe0212513ffaefa01813fe794b7dee22058`.
  It crossed the complete initial lifecycle and automatic restart and passed
  every namespace control. The fresh-owner HTTP probe then timed out at the
  bounded `300s`. The fresh owner did commit the exact owner-reopened
  observation claim, but provider-journal admission rejected it before live
  inspection: the canonical `ObserveIngress` stream still held the resolved
  initial-publication attempt, while the restarted publication has a different
  effect subject and execution attempt. Thus Run 52's `ObserveIngress`
  live-absence permission was necessary but could not run. The exact compute
  regression now reaches this boundary and fails before `0/1`. The correction
  is limited to exact owner-reopened publication lineage; ordinary crossed
  attempts must remain rejected. Run 53 is preserved at
  `/var/lib/nimbus-nnc92-proof/run53/evidence.json`. K1-K14 remain unclaimed;
  blocker is none. Next is the deterministic correction, affected gates, and
  one fresh immutable Run 54 candidate.

The Run 53 correction is now candidate-green locally. Owner-reopened
inspection rotates only a resolved provider-journal stream with the same
stable authority, then authenticated live absence rotates only the matching
publication stream into its next exact epoch. The portable absence evidence
records its claim-derived origin in strict saga format v7. Old callbacks and
ordinary crossed attempts remain fenced. Exact compute, sandbox, and workloads
proofs pass. Full affected behavior passes at compute `499 + 1 ignored`,
sandbox `1,201 + 31 ignored` plus all integration targets, and workloads
`231`. The affected all-target check, durability contract `24/24`, mutation
self-test `10/10`, Rustfmt, and diff check pass. A deterministic K8 test-only
correction keeps the creator process alive through receipt capture and then
explicitly reaps it; all seven Krun restart-phase tests pass. Local and
minicloud product/harness paths match by checksum. The one Run 54 binary build
is active. K1-K14 remain unclaimed and structured review has not started.

Run 54 used authenticated 801,496,016-byte candidate
`94c10cc4c3d479c4ea724ecfd5bc4a73b877beb05988622da4abeac72531ccd5`.
It crossed initial canonical readiness and HTTP, the deliberate exit,
automatic restart, restarted HTTP, and the first owner handoff. The fresh
owner then timed out at the bounded `300s` HTTP probe. Exact durable evidence
shows that owner-reopened `ObserveIngress` correctly rotated to the restarted
execution and performed live inspection. Inspection returned a definite
failure before republish. The confirmed command's provider-journal subject is
the restarted execution, but its validation execution is still derived from
the initial active intent. The foreground submission then observed the
retained recovery entry and failed closed with `CrossedTrackedRequest`; that is
a secondary result, not a second authority defect. Run 54 remains immutable at
`/var/lib/nimbus-nnc92-proof/run54/evidence.json`. K1-K14 remain unclaimed;
blocker is none. Next is one deterministic fail-before for the crossed command
execution, the narrow correction, affected gates, and a fresh Run 55.

The Run 54 regression failed exactly `0/1`: the confirmed command named
restart epoch `0`, while its durable publication subject named epoch `1`.
Command construction now uses the record's canonical current execution, and
the regression passes `1/1`. Dispatch tests pass `21/21`, provider tests pass
`9/9`, full compute passes `499 + 1 ignored`, and server ingress passes
`30/30`; affected all-target check, strict Clippy, Rustfmt, and diff checks
pass. Product-owned Run 54 cleanup reached `Recorded`; no live listener or
namespace remains. The remote incremental build completed in `4m00s`. Its
801,496,040-byte binary has SHA-256
`f0dcef84fc159882baa7774efebcbdc537d451662c19fdeb7ca28b6a276550b4`
and is staged root-owned mode `0500` under the matching candidate directory.
It is the sole candidate for Run 55. Structured review has not started.

Run 55 used that authenticated 801,496,040-byte candidate. It crossed initial
canonical readiness and HTTP, deliberate exit, automatic restart, restarted
HTTP, restart inspection, and the first owner handoff. The corrected
owner-reopened command authenticates restart epoch `1`; its exact
`ObserveIngress` provider-journal attempt remains `in_progress` instead of
returning the Run 54 crossed-execution failure. The fresh-owner HTTP probe then
timed out at the bounded `300s`. Source evidence is
`server_ingress_target_waiting_for_private_attachment`: durable attachment and
IPAM state are Ready, while the process-bound PEP lease remains Active and the
fresh process has no registered PEP handle. The public lease is correctly
Reserved with confirmed-stop evidence. Run 55 is immutable at
`/var/lib/nimbus-nnc92-proof/run55/evidence.json`; its outer namespaces were
removed by the harness, but its exact product state remains preserved for
forensics. K1-K14 remain unclaimed; blocker is none. Next is one deterministic
fresh-owner regression for this exact PEP/private-readiness boundary, followed
by a correction confined to the existing sandbox/proxy effect owner. No
structured review has started.

The Run 55 correction is candidate-green locally. The exact compute
fail-before was `0/1`: owner reopen issued publication inspection without first
testing the process-bound private attachment. The exact Krun fail-before was
`0/1`: a fresh backend reported `InProgress` for an Active retained attachment
whose process-local PEP registration was absent. The correction keeps compute
as the sole saga coordinator and persists one two-stage owner-reopen protocol:
inspect and, only after authenticated absence, repair the existing attachment;
then enter the existing publication inspection, republish, and re-observation
protocol. Container and Krun keep all PEP effects. Their read-only inspections
remain effect-free, and repair accepts only the existing Active planned PEP
lease with missing process registration. No address becomes identity, no new
allocation occurs, and `nimbus-network` is unchanged.

The exact compute driver, compute provider, sandbox provider journal, Krun,
and Container proofs pass `5/5`. Portable workloads pass `231/231`; compute
passes `499` with one expected child-process ignore; sandbox passes `1,202`
with 31 expected child-process or environment ignores. The focused compute
driver is `17/17`, compute provider adapter `9/9`, sandbox provider journal
`51` with two expected child-process ignores, and Krun provision phases
`14/14`. K1-K14 remain frozen and unclaimed. Next is the remaining local
quality gates, one authenticated Run 56 candidate, and a fresh complete
lifecycle. Structured review has not started; blocker is none.

Run 55 product retirement returned one truthful `Waiting`. Read-only durable
inspection proved that the exact stop command was `in_progress`; one
evidence-based retry then reached terminal `Recorded`. No Run 55 process or
listener remains. The failed-run root retains a detached/released terminal
manifest and the superseded owner-reopened `ObserveIngress` journal entry. A
post-terminal startup scan quarantined that old root and rejected a crossed
quarantine generation before provider effects. No manual deletion or reuse
followed. Run 55 therefore remains negative evidence, not K11 proof. Run 56
must use its disjoint roots and prove complete terminal cleanup.

The remaining local candidate gates pass: affected all-target/all-feature
compilation, strict Clippy, Rustfmt, diff check, durability `24/24`, lifecycle
mutations `8/8`, Python compilation, shell syntax, and ShellCheck. Every owned
dirty path matches the isolated minicloud worktree by checksum. One warm
two-job x86_64 build completed in `4m56s`. Its root-owned mode-`0500`,
801,537,584-byte binary has SHA-256
`a5372091625de741f4aca80d8873ffe235d40d78847e13d2eb992f008fee459d`.
It is the sole candidate for Run 56. K1-K14 remain unclaimed until the frozen
two-attempt lifecycle and evidence validator pass.

Run 56 crossed the complete initial lifecycle, automatic restart, first owner
handoff, and the former Run 55 boundary: fresh-owner HTTP returned
`restarted-ready` without a timeout. The restart and fresh manifests name the
same sandbox `wex_5718e1a25db771b9a14c5f10b412536d957fac2f5fa7e8c062e7da72c6c44488`
and the same restarted execution attempt
`wea_5fe12120d47bd5f36d74aae35747782db0488a9f3cf874984f69e9491a4cb9e3`.
Read-only host evidence shows one exact current conmon with one live child VM.
The harness then failed `fresh owner duplicated or crossed the running
attempt`. Its census admits every host command that mentions
`/usr/libexec/nimbus/crun`, including retained historical conmon processes,
while its exact test accepts only that path as `argv[0]`. The real child
correctly presents `[libcrun:krun]`. This is a harness-only K5/K9 evidence
defect. Product code and candidate identity remain frozen. K1-K14 stay
unclaimed; next is one deterministic exact-process-census regression, Run 56
product cleanup, and Run 57 with the unchanged binary.

The exact run-owned conmon-to-`libkrun VM` census now rejects historical and
crossed owners while accepting the real child argv `[libcrun:krun]`; its
deterministic invariant passes. Product cleanup then exposed a separate
durable defect. The valid revision-48 owner-reopened attachment repair could
not accept stopped successor generation 2 because validation compared its
artificial `Prepared -> NetworkAttached` attempt phases with the durable
`Observed` lifecycle phase. The exact fail-before is `0/1` with `provision
attempt is crossed with the durable workload generation`.

The correction preserves that exact claim as inspection-required under the
successor fence. Only fully matched attachment success or authenticated
absence can enter withdrawal; generic queued teardown remains rejected before
inspection, and the superseded provision effect is never retried. Workloads
pass `231/231`; compute passes `501/502` with one existing ignore. Affected
all-target/all-feature compilation, strict Clippy, Rustfmt, diff check,
durability `24/24`, and lifecycle mutations `8/8` pass. `nimbus-network` and
provider-effect ownership are unchanged. K1-K14 remain frozen and unclaimed.
Next is one replacement minicloud candidate, exact Run 56 product cleanup,
and fresh Run 57.

Every owned dirty path is byte-identical in the isolated minicloud worktree.
One two-job debug-stripped build completed in `4m05s`. Its root-owned
mode-`0500`, 801,548,128-byte binary is
`/var/lib/nimbus-nnc92-candidates/26eda1c3e6189ddcba2097229894e6f0302f336a6f378f874f9adfc2e0dbc57f/nimbus`;
the directory name is its SHA-256. This is the sole candidate for exact Run 56
cleanup and fresh Run 57.

Exact Run 56 cleanup with that candidate returned one truthful `Waiting`, then
one evidence-based retry reached workload saga `Recorded`. The durable network
store reports the attachment, tenant segment/IPAM, public port, and PEP lease
released, and the exact conmon and VM are absent. The Krun manifest is not
terminal, however: it remains `stopping`, retains its exact rootfs launch
artifact, and does not satisfy `has_terminal_network_finality()`. K11 is
therefore correctly red even though the coordinator recorded teardown. The
bounded correction is to make sandbox-owned release finalization crash-safe:
persist release authorization, idempotently remove only the authenticated
owned launch artifact, publish terminal manifest state, and make inspection
retry rather than report success until those facts hold. Krun and Container
must prove the same provider contract. No manual cleanup, fresh lifecycle,
broad gate, or structured review starts before that focused proof is green.

The bounded provider-finality correction is focused-green. Krun and Container
each failed before with a successful Release that left the manifest
`Stopping` (`0/1` each). Release now treats durable `Released` network
authority as authorization to remove only the exact provider-owned launch
artifact, then publishes `Stopped` manifest finality. Inspection authorizes a
retry until both facts hold. Startup applies the same idempotent transition
only to an exact released, execution-stopped, process-absent manifest. The two
fresh-process released-checkpoint proofs each pass (`1/1`) with a real rootfs
artifact sentinel and prove startup finality before command replay. The full
focused network-teardown set passes (`24 + 2 ignored`). The corrected
two-thread Krun proof permits only the intended terminal manifest fields and
still proves zero repeated runtime effects. K1-K14 remain unclaimed. Next is
one authenticated minicloud candidate, product-owned Run 56 convergence, and
one fresh two-attempt lifecycle; broad gates and structured review remain
deferred.

The first live provider-finality candidate then proved the remaining K11 edge:
network release and process absence were exact, but a durable runtime PID
receipt remained, so startup refused to remove the rootfs. The correction now
uses each sandbox adapter's existing provider delete/absence and creator
quiescence seams before it retires only the exact PID, conmon, and exit
receipts. Rootfs removal and terminal `Stopped` publication follow those
durable facts. Direct Krun and Container release proofs pass `2/2`; the two
released-checkpoint fresh-process proofs pass `2/2` with stale receipts and
real rootfs sentinels; the serialized focused set passes `24` with two expected
subprocess-entry ignores. The parallel focused run had one harness-only
15-second contention timeout; the exact contender passed `1/1`, and the
serialized complete set is green. No manual provider cleanup occurred.

Compute provisioning now retains the new foreground wait through durable saga resume until
an exact services projection exists or the caller cancels. The one-shot method
remains unchanged for asynchronous callers. Deterministic tests prove
waiting-to-observed convergence and cancellation without a busy retry owner
(`2/2`). Direct Compose now shares the canonical CLI provider-helper discovery
contract, and the exact fail-before passes (`1/1`). The nft state-set matcher
now parses symbolic and double-numeric nft encodings structurally, accepts only
exact established/related pairs in either order, and rejects missing, extra,
mixed, or substituted states. The complete focused suite passes (`13/13`).
Diagnostic warnings outside this owning seam were removed. The authenticated
Run 24 through Run 28 runtime and attachment effects were removed exactly.
Every retained run root and all unrelated historical stopped objects remain.
Runs 29-31 exact runtime and attachment effects were removed after their
evidence was captured. Runs 32-33 retain only their durable roots and no host
effects. Run 34 retains the exact failed-settlement state described above.

The exact caller-level regression
`direct_compose_krun_preserves_private_runtime_when_path_has_stock_crun`
failed before the correction (`0/1`): the selected path was the fixture's
stock `crun`, while the expected path was `/usr/libexec/nimbus/crun`. The
correction removes only generic `crun` discovery from the Krun helper adapter;
the owning `KrunSandboxBackendConfig` keeps its capability-specific runtime.
The regression now passes (`1/1`), both direct Compose selection tests pass
(`2/2`), both unchanged machine helper consumers pass (`2/2`), `cargo check -p
nimbus-cli` passes, and Rustfmt passes. Run 24 supplies the live proof.

Authenticated candidate
`07654f0635c9cbf54342ff54240a64414d67b13c35cc1e330920ec348be64ddb`
product-converged preserved Run 56 without manual provider effects. Its exact
manifest is `Stopped`, launch authority is `Released`, launch artifact is
absent, creator handoff is quiesced with dead-contained proof, provider cleanup
is inactive, execution is stopped, and network detach/release are terminal.
The canonical network store has one released attachment, two released port
leases, empty tenant segment/IPAM ownership, and terminal provider journals.
Read-only inspection found no run-owned process, listener, netns, interface, or
nft table. This closes the bounded provider-finality defect.

Fresh Run 57 then crossed initial readiness and HTTP, deliberate exit and
automatic restart, restarted HTTP, first-owner handoff, fresh-owner
reconciliation, and fresh HTTP. Its first Compose-down call exited `1` because
compute returned the truthful durable `Waiting` disposition and the foreground
caller treated it as terminal. The run remains product-owned and preserved;
no manual effect cleanup is authorized. The bounded correction keeps the
existing one-shot retirement API unchanged and adds a cancellable foreground
method that retries only safe pending settlement and `Waiting` states through
durable truth. `CleanupPending` and definite failures remain fail-closed. Exact
tests must prove waiting-to-terminal convergence without duplicate effects and
cancellation with later replay before a replacement candidate is built.

That bounded correction is focused-green. The compute retirement module passes
`35/35`, including waiting-to-terminal convergence with no duplicate provider
effect and cancellation followed by durable replay. Its failure set passes
`7/7`, including pre-cancel zero source/store/provider mutation and explicit
non-retry of `CleanupPending`. Teardown runtime passes `4/4`. The serialized
CLI retirement module passes `10/10`, including the exact foreground sequence
Execute, Inspect `InProgress`, Inspect `Satisfied`, followed by terminal
`Recorded`. Compute and CLI all-target checks pass, Rustfmt and the diff check
pass, and the final seven correction paths are byte-identical in the isolated
minicloud worktree. The authenticated replacement candidate is recorded below.
K1-K14 remain frozen and unclaimed; structured review and broad gates remain
deferred.

The 801,559,040-byte replacement candidate has SHA-256
`52246ff67e0759cec95f75bf967b06495d5c1752617e850ef6622ac3964a471a`
and is staged root-owned mode `0500`. It product-converged preserved Run 57 in
one foreground call to terminal `Recorded`. The manifest is `Stopped`, release
and detach are terminal, the launch artifact is absent, creator handoff is
quiesced, provider journals are terminal, and exact process, status, and netns
counts are zero. The network store has one released attachment, two released
port leases, and empty segment/IPAM ownership.

Run 58 used that candidate and crossed initial serving, automatic restart,
fresh-owner reconciliation, terminal Compose down, and endpoint refusal. It
then failed on a harness-only assertion: `compose ps` returned the exact one
persisted terminal observation (`stopped`, shutdown requested, no endpoints),
while the harness required `[]`. The CLI contract intentionally lists
persisted sandbox state, and K11 requires stopped/recorded truth rather than
deleting observed history. The corrected classifier requires exactly one
tenant/service/sandbox-qualified stopped row and rejects missing, duplicate,
crossed, active, published, or restartable projections. Evidence derivation now
binds that result into K11. Local and minicloud deterministic mutation
tests remain `8/8`; all four harness invariants pass. Run 58 post-state has a
terminal manifest, terminal provider journals, zero exact processes, and zero
status/netns residue. It remains immutable FAIL evidence. K1-K14 remain
unclaimed pending fresh Run 59 with the unchanged product candidate.

Run 59 crossed every product lifecycle condition through one-call terminal
Compose down, endpoint refusal, and the exact stopped projection. Its manifest,
provider journals, process census, status paths, namespace paths, attachment,
leases, segment, and IPAM state were terminal. The harness then rejected the
retained `inet netavark` table. Netavark 1.14 source and its own teardown tests
show that the provider intentionally retains ten global chains and seven global
rules while deleting each per-network `nv_*` chain and subnet, bridge, and
host-port rule. The corrected full-table classifier accepts only that exact
provider-global scaffold. It fails closed on a dynamic chain, bridge rule,
subnet or container address, published or guest port, unknown executable nft
object, duplicate or missing global chain, and missing or extra rule. Local
mutation tests remain `8/8`; all five lifecycle invariants pass. Run 59 remains
immutable FAIL evidence. K1-K14 remain unclaimed pending fresh Run 60 with the
unchanged product candidate.

Run 60 again crossed the full product lifecycle and terminal state. Evidence
collection then failed because the command recorder authenticates every token
named `netavark`; this correctly rewrote tools but also rewrote the nft table
name in `nft list table inet netavark`. The correction collects the complete
JSON ruleset without a table-name argument and structurally selects only
objects owned by `inet netavark`. An unrelated proof table and counter are now
part of the passing deterministic fixture, so cross-table state cannot satisfy
or fail the provider-scaffold proof. Local mutation tests remain `8/8`; all
five lifecycle invariants pass. Run 60 remains immutable FAIL evidence.
K1-K14 remain unclaimed pending fresh Run 61 with the unchanged product
candidate.

Run 61 again crossed the full product lifecycle and terminal state. Structural
classification found exactly Netavark's ten global chains and seven global
rules, but the marker scan still traversed the separate sovereignty proof
table, whose deliberate `10.0.0.0/24` controls overlap the provider subnet.
The corrected scan traverses only structurally selected `inet netavark`
objects. Its deterministic fixture now contains an unrelated table rule with
both the provider subnet and published port. All `8/8` mutations and five
invariants pass. Replay against Run 61's captured full ruleset and exact
terminal manifest returns `passed=true` with zero run-owned markers. Run 61
remains immutable FAIL evidence. K1-K14 remain unclaimed pending fresh Run 62
with the unchanged product candidate.

Run 63 passed product lifecycle, exact terminal cleanup, DNS capture, and
control-exclusive syscall-address classification. The only remaining evidence
was exactly two 40-byte IPv4 packets. A disposable namespace applied the same
rules and created only the Netavark-style bridge/veth/guest topology; it
reproduced the same `denied_ipv4=2`, `bytes=80` result. nft trace identified the
packets as TTL-1 IGMPv3 membership reports from bridge address `10.0.0.1` to
IPv4 link-local multicast `224.0.0.22`. They are kernel bridge-control traffic,
not workload or control-plane egress. The profile now allows only IGMP output to
`224.0.0.0/24`, symmetric with its existing IPv6 link-local control allowance;
it does not allow `224.0.0.0/4` or forwarding. The identical isolated
reproduction then returned zero for all six named counters. Local mutation
tests remain `8/8`; all six invariants pass. Run 63 remains immutable FAIL
evidence. K1-K14 remain unclaimed pending fresh Run 64 with the unchanged
product candidate.

Run 64 is the candidate-green lifecycle. Both required attempts passed the
same authenticated 801,559,040-byte candidate with SHA-256
`52246ff67e0759cec95f75bf967b06495d5c1752617e850ef6622ac3964a471a`.
Every K1-K14 assertion is derived and green. Each attempt has the exact 19
ordered transitions, one automatic restart, exact fresh-owner recovery,
one-call recorded retirement, an unreachable retired endpoint, one terminal
stopped projection, terminal provider journals and manifest, no run-owned
process/status/netns/interface/nft residue, zero values for all six named
network counters, empty DNS capture, no forbidden trace address, and complete
outer cleanup. The evidence result is `PASS`, exit `0`; all `8/8` mutation
cases and six deterministic invariants pass. NNC9.2 is candidate-frozen for
its one broad affected-gate pass and one Sol/xhigh/fast item review.

Run 62 passed the corrected Netavark classifier and exact terminal product
cleanup, then failed only its network-profile scope. Source evidence identified
four measurement defects: the address census included `control.strace*`; it did
not normalize IPv4-mapped provider peers; tcpdump's signal-only newline was
treated as DNS; and the fresh ruleset was installed immediately after the
negative nonblocking-connect controls, allowing two delayed IPv4
retransmissions into the measured counters. No lifecycle trace contained a
forbidden connect, the DNS capture contained no packet, and all named product
addresses were local or provider-private. The corrected profile excludes
control traces, normalizes mapped addresses, preserves only non-whitespace DNS
evidence, and waits two seconds for closed control sockets to quiesce before it
installs and proves a fresh zeroed ruleset. The exact scope invariant includes
control-only public addresses, mapped provider peers, and empty/nonempty capture
cases. Local mutation tests remain `8/8`; all six invariants pass. Run 62
remains immutable FAIL evidence. K1-K14 remain unclaimed pending fresh Run 63
with the unchanged product candidate.
