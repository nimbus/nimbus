# RRC5 Workload Hosts

Status: `RRC5_WORKLOAD_HOSTS_PASS`

Date: 2026-09-06

## Candidate under test

The final candidate is Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

The Linux lane uses the isolated worktree
`/home/nimbus/rrc5-release-candidate` on `nimbus@minicloud`. Its Deno path
dependencies point to `/home/nimbus/rrc5-deno`. No branch, tag, package, or
image left the isolated proof environment. The final provisional executable is
3,082,795,272 bytes with SHA-256
`283295f9d80558ec55e5c0523b40e3d04b0b5d29a803c2a504ed932ccac6285d`.

The terminal release-tuple VMM proof uses Nimbus
`c565e89fd8b089c6b11159c35274e8d8e74c7bf9`. No RRC5-owned source, verifier,
or live-helper path changes between that commit and the final candidate. The
final candidate also passes the native server lifecycle, nine-application
lane, hosted container-egress lane, hosted krun drill, and Node D-Bus lane.

## Fail-before ledger

| ID | Severity | Fail-before evidence | Terminal verdict |
|---|---|---|---|
| RRC5-001 | P1 | Nimbus 0.1.45 defaulted to `machine-os:v0.1.30`, whose embedded Nimbus 0.1.30 could not satisfy the current host contract. | Fixed in `e03a010a4`. The host now pins published `machine-os:v0.1.45@sha256:e313a09b481b86de8cfe99cefdc1e9b631d65e96b3971eb300660f8ae92e1e9b`, with an independent exact-value regression. |
| RRC5-002 | P1 | Standalone `nimbus machine stop` always passed no persistence configuration. It rejected before execution with `physical-machine stop requires canonical Engine workload authority`; `machine rm` then could not release the active SSH lease. | Fixed in `e03a010a4`. The top-level CLI resolves the canonical persistence contract only for stop and restart effects. A real stop of the orphaned failed state succeeded, `machine rm` succeeded, and isolated `machine list --format json` returned `[]`. |
| RRC5-003 | P1 | The bootc guest applied the mounted authority and started its baked service, but the host then ran the legacy SSH stop/install/restart sequence. The released image's passwordless `nimbus` account had no noninteractive sudo rule, so first boot failed. | Fixed in `e0a68be28`. Bootc now waits for its baked forwarded API. Legacy images retain SSH convergence through the existing root management channel; user-facing SSH and copy keep the configured guest user. |
| RRC5-004 | P2 | The machine-os recipe called `nimbus` an administrator and added it to `wheel`, but the account had no password and no passwordless sudo rule. Interactive administration could not use sudo. | Fixed locally in machine-os commit `d5752a4`. The recipe installs `sudo`, writes `nimbus ALL=(ALL) NOPASSWD: ALL` as mode 0440, records the package, and verifies the contract. A replacement image was not published. |
| RRC5-005 | P1 | Live libkrun resource checks stayed at the provider defaults because Nimbus wrote `/.krun_vm.json`; current crun consumes the authenticated `krun.cpus` and `krun.ram_mib` OCI annotations. An image-supplied sidecar could also select untrusted VMM settings. | Fixed in `6ca8eb981`. Nimbus emits CPU and rounded-RAM annotations, keeps the generic OCI memory limit, and removes an image-supplied sidecar before launch. Direct-rootfs and image-backed KVM tests observe the requested limits. |
| RRC5-006 | P1 | Applying OCI `USER` to the crun bundle prevented the VMM from starting, while leaving the bundle as root skipped the image user inside the guest. The scratch image fixtures also omitted dynamically linked BusyBox libraries. | Fixed in `6ca8eb981`. The VMM bundle stays root, the injected guest helper applies the numeric image UID/GID, and the fixture copies its runtime libraries. A live guest reports UID/GID 33 while the bundle remains UID/GID 0. |
| RRC5-007 | P1 | A failed launch at `Reserved + NotSpawned` entered exact Detach without first persisting attachment-adoption intent. Retry could exhaust while the original reservation remained fenced. | Fixed in `6ca8eb981`. Detach durably records the cleanup intent before interrupted-adoption reconciliation. Same-process and fresh-process tests cover both launch-reserved and adoption-intent-reserved cuts. |
| RRC5-008 | P1 | Provider-assigned ingress sent external port `0` into the private TSI map and then exposed a placeholder endpoint. Terminal cleanup also rejected this valid no-effect publication authority. | Fixed in `6ca8eb981`. The private bridge uses the guest port until the ingress owner binds an OS-assigned external port, the backend hides the placeholder, and terminal reconciliation accepts the exact provider-assigned no-effect case. Three concurrent guests received distinct reachable ports and released them exactly. |
| RRC5-009 | P2 | The live smoke lane used fixed `10.0.0.0/16` addresses and fixed host ports. An unrelated active Nimbus route captured the third guest address, and an unrelated listener occupied port 18081. | Fixed in `6ca8eb981`. The smoke configuration accepts an explicit node supernet and a checked host-port offset. The final run used isolated `10.247.0.0/16` and ports 21081 through 21089. |
| RRC5-010 | P2 | The liveness smoke tried to control a guest with a host marker and direct crun signal. It also expected the backend's raw durable projection to remember a prior `Ready` state, although the compute saga owns that state. | Fixed in `6ca8eb981`. The guest now performs its own 30-second ready, 10-second unavailable, then recovery cycle. The test accepts the backend's exact `Starting` or `NotReady` unavailability report, requires endpoints to withdraw, and proves the runtime PID does not change. |
| RRC5-011 | P2 | The container-only egress target failed to compile with warnings denied because shared provision support exposed `TestIngressSet::addresses` only to KVM smoke callers. | Fixed in the current working tree with one method-local, reasoned `dead_code` allowance. The container target now compiles on macOS and Linux, while the KVM callers still execute the method. |
| RRC5-012 | P2 | The archived SDK resource-model verifier reported 17 passed and 6 failed because it stopped at obsolete pre-native-workload file names, state maps, test names, and one removed compute file. | Fixed in the current working tree. Its checks now follow durable source retirement, desired/observed sandbox state, tenant retirement, and multiline SDK signatures; the verifier passes 23/23. |
| RRC5-013 | P1 | The compiler-authority refresh put generated output under the host's 3.9 GiB `/tmp` tmpfs and reused the shared Cargo target for MIR. The first run exhausted temporary storage; the disk-backed retry then correctly rejected a feature-off V8 artifact in a feature-on scan. An external `CARGO_TARGET_DIR` workaround would also enter the baseline as a host-specific identity. | Fixed in the current working tree. The verifier now requires 16 GiB free, supports `NIMBUS_NETWORK_COMPILER_TEMP_ROOT`, and owns one private Cargo target for MIR, generated output, and structural scans. Its 18 mutation self-tests pass, and a real 3 GiB preflight rejects before creating output. The final portable baseline refresh remains blocked by RRC1 because the integrated Linux candidate uses local Deno path overrides and therefore a different lock resolution. |
| RRC5-014 | P1 | A public sandbox or service create could return a truthful pending receipt, then discard the only task that was authorized to continue provider inspection. Repeated GET requests correctly had no side effects, so the resource could not converge without a second lifecycle POST. | Fixed in the current working tree. A keyed retained supervisor publishes bounded pending receipts, continues exact read-only inspection with bounded backoff, and separates caller progress from final settlement. Regression tests prove GET-only convergence and retirement handoff. |
| RRC5-015 | P1 | Public sandbox and service stop drove only one teardown receipt and could return while durable teardown still reported a safe pending state. Retirement could also join the first pending provision receipt instead of its terminal settlement. | Fixed in the current working tree. Foreground stop retains ownership until terminal teardown or cancellation, and retirement joins the settlement channel. Sandbox stop boxes the recursive async future explicitly. |
| RRC5-016 | P1 | Startup or explicit recovery rejected a missing process-local source even when an exact durable stopped successor already authenticated retirement. This could strand cleanup after the source registry was removed. | Fixed in the current working tree. Missing-source authorization is accepted only for an exact durable stopped successor; all other missing or crossed sources still fail closed. |
| RRC5-017 | P1 | Final ingress withdrawal rejected an exact empty endpoint set. A standalone sandbox or unpublished workload could therefore fail teardown even though the compiled plan authenticated that no public listener existed. | Fixed in the current working tree. Empty final membership is accepted as an authenticated no-publication decision; attachment and internal PEP cleanup remain owned by later teardown steps. |
| RRC5-018 | P1 | KVM provision replay accepted reserved or adopted attachment authority but rejected the durable intermediate adopting state. Attachment readiness also kept one pre-activation absence shape in progress. An interruption after the adoption intent could not converge. | Fixed in the current working tree. Attachment adoption is one resumable transition across Reserved, Adopting, and Adopted, with exact claim checks and missing-pre-activation authority reported as absence. Focused restart and provision-phase regressions pass. |
| RRC5-019 | P1 | Definite failure after PrepareWorkload but before creator admission planned StopExecution without DrainExecution. Both Container and KVM backends required a durable drain first and returned `sandbox_teardown_command_crossed`, so compensation could not complete. | Fixed in the current working tree. Exact Execute + prepared + NotSpawned state persists `ExecutionNeverAdmitted` under the StopExecution fence, closes later activation admission, and never fabricates a Drain command. Focused Container and KVM teardown suites and the full sandbox library pass. |
| RRC5-020 | P1 | `nimbus start --compose-file` constructed and logged the desired workload and placement plan but never submitted it. On a fresh root, ten GET-only polls stayed pending, port 21980 remained unbound, and no sandbox existed; one manual service-start POST proved the backend itself was healthy. | Fixed in the current working tree. The CLI validates one-to-one desired and local placement evidence, the server applies the ordered plan after recovery and before serving, and synchronous router construction refuses to silently skip a plan. A fresh Linux root reached ready automatically with no lifecycle POST. |

## macOS machine lifecycle

The final isolated proof used the repository helper with dedicated HOME, XDG,
runtime, network, data, and control roots:

```text
NIMBUS_MACHINE_HELPER_BINARY_DIR=/opt/homebrew/Cellar/podman/6.1.0/libexec/podman \
bash scripts/collect-nimbus-machine-cli-proof.sh \
  --machine rrc5-release-fixed \
  --root /private/tmp/nimbus-rrc5-machine-proof-fixed \
  --output-dir /private/tmp/nimbus-rrc5-machine-proof-fixed/output \
  --nimbus /private/tmp/nimbus-ws-test.0rXOFY/worktree/target/debug/nimbus
```

The host verified that the default image embeds Nimbus 0.1.45, names the
machine-os source repository, and has a build attestation. The proof then
passed these observable steps:

- Image materialization, isolated SSH identity, networking, and AppleHV boot.
- Guest boot and SSH readiness.
- Forwarded machine API readiness.
- Human, JSON, and YAML status.
- Canonical stop and SSH-lease release.
- Machine removal and process cleanup.

The running status reported `manager=ready`, machine API protocol `v1alpha2`,
systemd socket activation, and `service_execution_ready=true`. Bootc, conmon,
crun, netavark, and aardvark-dns were present. Every reported bootc and service
sandbox operation was available with no blocker. Stop and remove both returned
success, and no krunkit or gvproxy process remained.

The successful proof output remains under
`/private/tmp/nimbus-rrc5-machine-proof-fixed/output`. Cleanup deleted the 1.1
GiB downloaded image and 10 GiB materialized disk. The retained proof root is
about 3.8 MiB.

## Automated evidence

These focused checks pass against the integrated candidate:

```text
cargo test -p nimbus-cli parses_machine_init_defaults_to_version_pinned_release_image --lib
cargo test -p nimbus-cli direct_machine_dispatch_requests_engine_authority_only_for_fenced_effects --lib
cargo test -p nimbus-cli machine::tests::records_state --lib
cargo test -p nimbus-cli machine::manager::tests::provider_bootstrap --lib
cargo test -p nimbus-cli machine::manager::tests::ssh_scp --lib
cargo clippy -p nimbus-cli --all-targets -- -D warnings
```

The selections passed 1, 1, 19, 19, and 6 tests. Clippy passed with warnings
denied. The proof helper passes shell syntax, Rust formatting passes, and all
changed diffs pass whitespace checks.

All six machine-os repository checks pass, as does Nimbus's
`verify-machine-os-release-ref-contract.sh` cross-repository gate.

## Linux KVM evidence

`minicloud.local` resolves through the user's SSH configuration and connects
without a host-key prompt. The Debian 13.4 x86_64 host has KVM, Podman, Buildah,
Rust 1.97.1, Node 24.19.0, about 133 GiB free disk, and passwordless sudo. The
installed libkrun tuple is older than the repository contract, so the lane used
the current tuple staged under `/home/nimbus/rrc5-vmm-tuple/staged`. It did not
replace the host installation.

The final serial run used the integrated test binary with SHA-256
`7646528ac54761946212460c1a54a9eebdb854975f1c9925b16ded62c69d42dd`.
It used the isolated supernet `10.247.0.0/16` and a host-port offset of 3000.
It also used the staged crun/libkrun tuple, rootfs fixture, and guest-user
helper:

```text
NIMBUS_KRUN_SMOKE_NODE_NETWORK_SUPERNET=10.247.0.0/16 \
NIMBUS_KRUN_SMOKE_HOST_PORT_OFFSET=3000 \
NIMBUS_KRUN_SMOKE_WORKDIR=/home/nimbus/rrc5-linux-proof/full-suite-pass-20260827-3 \
NIMBUS_KRUN_SMOKE_RUNTIME=/home/nimbus/rrc5-vmm-tuple/staged/crun \
NIMBUS_KRUN_GUEST_USER_HELPER_ROOT=/home/nimbus/rrc5-guest-helper-root \
cargo test -p nimbus-sandbox --test krun_linux_smoke -- --ignored --test-threads=1
```

Result: 8 passed, 0 failed, 0 ignored, 1 filtered out, in 185.05 seconds. The
eight live cases proved:

- Image `USER` and `STOPSIGNAL` lowering with bundle UID/GID 0 and guest UID/GID
  33.
- A SIGQUIT stop that completed gracefully in 10.02 seconds without a fabricated
  exit code.
- A separate numeric guest-user case that reported UID/GID 33.
- Image-backed pull, build, boot, HTTP response, non-loopback refusal, and
  cleanup.
- Direct-rootfs and image-backed CPU/RAM enforcement.
- Three concurrent provider-assigned ingress bindings, reachable HTTP, exact
  teardown, and later OS rebind.
- Liveness withdrawal and recovery without a VM restart.
- Readiness gating before endpoint publication.

All ten terminal manifests report `stopped` and no synthetic exit code. After
the suite, no `10.247` route, run-owned network namespace, or run-owned process
remained. The successful 38 MiB proof root stays at
`/home/nimbus/rrc5-linux-proof/full-suite-pass-20260827-3`. Cleanup unmounted
and deleted the Buildah fixture, both test images, and all superseded proof
roots.

## Current release-tuple lifecycle drill

Commit `fb56b7816bc29e67b1973370feefdbfae03d860a` updates the release contract.
The immutable tuple uses crun `v1.29.1-nimbus.2`, libkrun
`v1.19.4-nimbus.3`, and libkrunfw 5.5.0. The downloaded amd64 artifacts passed
their published SHA-256 checks:

- crun: `eb136ebad6516238e8967bfff8651be67ad569772f9ea2d4b5f3d77242d7639a`.
- libkrun: `a4a462b284f212731ac548c2f5d4039e38f825101ae59c5eb56ad5b140c473c1`.

The isolated Debian 13 run at
`/home/nimbus/tmp/rrc8-vmm-live.EXop7D` completed LH1 through LH6. The run
installed only the private Nimbus runtime tuple and preserved the host's system
crun. It then repeated the strict host check. Both the direct and conmon paths
booted a copied rootfs through KVM and returned `nimbus` over HTTP. Each path
tried graceful TERM and used the bounded KILL fallback when the guest did not
exit. Both paths reported exit 137.

The conmon process tree proved
`conmon -> libcrun VM`. Normal-user entry re-executed through passwordless sudo
at the system-service boundary. A second conmon run passed with the same
container ID. This result proves stale state, log, PID, exit-file, and
persistence cleanup.

Final cleanup found no drill crun state, matching process, listener on port
28080, or Buildah container. The unrelated `nimbus-libsql-local` Podman
container remained running. All four generated command scripts passed
`bash -n` before execution.

Local regression evidence for commit `6ca8eb981` is also green:

```text
cargo fmt --all --check
cargo test -p nimbus-sandbox --lib
cargo test -p nimbus-sandbox --test krun_linux_smoke --no-run
```

The library result was 1,215 passed, 0 failed, and 31 intentional ignored
subprocess or characterization entries. The live smoke target compiled.

## Linux container egress evidence

The root-only container target first exposed RRC5-011 at compile time. After
the narrow shared-fixture correction, the target compiled on both macOS and
Linux. The live Linux run used the candidate's existing build target, the
official BusyBox image, exact Podman helper paths, and isolated proof root
`/home/nimbus/rrc5-linux-proof/container-egress-20260827-2`:

```text
NIMBUS_NETAVARK=/usr/lib/podman/netavark \
NIMBUS_AARDVARK_DNS=/usr/lib/podman/aardvark-dns \
NIMBUS_CONTAINER_EGRESS_WORKDIR=/home/nimbus/rrc5-linux-proof/container-egress-20260827-2 \
NIMBUS_CONTAINER_EGRESS_IMAGE=docker.io/library/busybox:latest \
NIMBUS_CONTAINER_EGRESS_TARGET=http://1.1.1.1 \
sudo --preserve-env=NIMBUS_NETAVARK,NIMBUS_AARDVARK_DNS,NIMBUS_CONTAINER_EGRESS_WORKDIR,NIMBUS_CONTAINER_EGRESS_IMAGE,NIMBUS_CONTAINER_EGRESS_TARGET \
  target/debug/deps/container_linux_egress-84e797b34825f20b \
  --ignored --nocapture --test-threads=1
```

Result: 2 passed, 0 failed, in 49.61 seconds. Nimbus denied direct external
egress. The policy case admitted the exact allowed HTTP path. It denied the
blocked path, default loopback, and direct bypass. After a live reload, Nimbus
denied the old endpoint and admitted the new endpoint. Internal DNS remained
denied.

No run-owned process, nsfs mount, or network namespace remained. The inspection
found two pre-existing namespaces named `kme-test` and `tsitest` and did not
modify them.

An earlier invocation supplied empty Netavark and Aardvark environment values
because the host installs those helpers outside `PATH`. Nimbus failed closed
before launch. This was a test-driver error, not a Nimbus defect. Cleanup
deleted its exact 84 KiB root after process and mount inspection.

## Repository verifier evidence

The source and contract verifiers now report:

```text
bash scripts/verify-nimbus-sdk-resource-model.sh
  23 passed, 0 failed
bash scripts/verify-service-sandbox-node-reconciliation.sh
  21/21 conditions green
bash scripts/verify-nimbus-sandbox-egress-launch-hardening.sh
  66 passed, 0 failed
bash scripts/verify-multi-tenant-network.sh
  16 passed, 0 failed
node scripts/nimbus-network-control-plane/compiler-authority-contract.mjs --self-test
  18 passed, 0 failed
```

The terminal proof ran the isolated compiler scan after the immutable Deno
references became available. Its compiler-authority self-tests passed 18/18.
No RRC5-owned source, baseline, or verifier path changed after that proof.

## Public resource and lifecycle evidence

The final provisional Linux binary ran from a fresh isolated data, control,
network, HOME, XDG data, XDG state, and runtime root. One standalone sandbox
create returned `201 ready`. Two later GET requests preserved the same resource
version and did not submit lifecycle work. The filtered list returned exactly
one item. Session open, get, list, and close returned 201, 200, 200, and 200.
The close retained the exact reason. One sandbox stop returned 200 and reached
`stopped` with no endpoints.

The terminal KVM manifest retained the exact stopped execution as durable
evidence. The stop quiesced creator handoff and released launch authority. The
network attachment was absent, and the runtime registry was empty. No process,
bundle payload, rootfs payload, workload network file, or bound port remained.
Nimbus retained the tenant firewall metadata and terminal manifest.

The retained-provision and foreground-retirement regressions pass. Focused
Container teardown passed 46 tests with zero failures and two intentional
ignored cases. Focused KVM teardown passed 38 tests with zero failures and two
intentional ignored cases. After the no-execution fence repair, the complete
`nimbus-sandbox` library passed 1,218 tests with zero failures and 31
intentional ignored subprocess or characterization cases.

Two controlled Netavark fault injections validated the ambiguous-outcome
contract. A nonzero provider exit and a missing executable both left the exact
AttachNetwork command `in_progress`, moved the attachment to
`cleanup_pending`, retained `creator_handoff=not_spawned`, and admitted no
execution. Nimbus did not guess that the provider had no effect. These cases
are safety evidence, not definite-failure evidence. Focused backend regressions
cover the definite pre-dispatch compensation branch. They do not edit live
durable state.

## Automatic Compose service evidence

The fail-before binary parsed the Compose file and logged a boot plan but never
submitted it. On a fresh root, ten GET-only polls stayed pending, port 21980
remained free, and the sandbox collection was empty. One explicit service start
returned 200 ready and bound the port. The empty BusyBox document root then
returned the expected HTTP 404. Explicit stop returned 200 and released the
port. This isolated the missing startup submission from the KVM backend.

The repaired binary crossed the validated plan into server startup. On a
second fresh root, health returned 200 without a lifecycle POST. The `web`
service reported ready and healthy.

The KVM sandbox was present, and port 21980 was bound. The guest returned the
expected 404.

One explicit stop returned 200 stopped and released port 21980. Server shutdown
released port 21880. Both roots retained exact evidence. No test-owned process
remained.

Matching-source checks on `minicloud` passed:

```text
cargo fmt --all --check
cargo check -p nimbus-server -p nimbus-cli
cargo test -p nimbus-server workload_boot::tests::boot_plan_preserves_order_and_rejects_duplicate_services --lib
  1 passed, 0 failed, 701 filtered out
cargo test -p nimbus-cli workload_boot::tests::schedules_compose_services_onto_embedded_node --lib
  1 passed, 0 failed, 1,076 filtered out
```

## Live node and systemd evidence

The final Linux lane used the warm integrated target and the host's session
systemd instance:

```text
cargo test -p nimbus-node \
  --features systemd-dbus,systemd-dbus-integration-tests \
  --test zbus_systemd_live --no-fail-fast
```

Result: 2 passed, 0 failed, in 5.23 seconds after the integration target
finished building. `start_inspect_stop_roundtrip_against_session_systemd`
proved the complete unit lifecycle, and `failed_unit_is_observable_via_inspect`
proved failed-unit diagnostics. The lane did not modify unrelated system
units.

## Final exact-candidate evidence

The direct KVM, container/egress, public resource, automatic Compose, and live
node/systemd lanes are complete. Exact-head container-egress run `34025638164`
passes. Exact-head krun run `34025639860` passes its bundle, direct drill,
runtime separation, Podman-machine diagnostics, and conmon helper gates. CI
run `34025631263` passes Root-only container PEP egress and Node D-Bus
Integration before the unrelated server stack overflow stopped the aggregate
gate. The resource-model, service-reconciliation, sandbox-egress, and
multi-tenant network verifiers are green.

## Current verdict

RRC5 passes with twenty confirmed findings fixed and covered by automated or
live evidence. Every supported macOS and Linux workload-host lane is terminal.
The immutable Deno and VMM references are reachable, all final-delta checks are
green, and no RRC5 product defect remains open.

Candidate binding: Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.
