# krun Sandbox Backend Smoke Test

Manual Linux-host smoke path for the first Rust `nimbus-sandbox` krun backend
slice.

Use this after the VMM foundation recorded in
`docs/plans/archive/vmm-infrastructure-plan.md` is complete on a supported
Linux host, or when rerunning the historical krun smoke lane for regression
comparison.

## Purpose

This smoke path proves the Rust backend can:

1. lower a generic `SandboxSpec` into the backend-owned krun implementation
2. boot a real VM through `conmon -> /usr/libexec/nimbus/crun`
3. reach the guest service over a TSI-mapped host port
4. recover the running sandbox from manifest-backed state with a fresh backend
   instance
5. stop the sandbox and preserve conmon/runtime logs on disk

## Host prerequisites

- Linux host with `/dev/kvm`
- `conmon`, `buildah`, and `/usr/libexec/nimbus/crun` installed
- VMM foundation validation complete (`LH1` through `LH6`)
- mounted rootfs for the guest workload

The easiest way to get a known-good rootfs is to reuse the VMM foundation
bundle flow:

```bash
buildah from --name nimbus-http docker://busybox:latest
ROOTFS="$(buildah mount nimbus-http)"
echo "${ROOTFS}"
```

## Command

Run the ignored Linux-only integration test:

```bash
export NIMBUS_KRUN_SMOKE_ROOTFS="${ROOTFS}"
export NIMBUS_KRUN_SMOKE_WORKDIR="/tmp/nimbus-sandbox-smoke"
export NIMBUS_KRUN_SMOKE_RUNTIME="/usr/libexec/nimbus/crun"
export NIMBUS_KRUN_SMOKE_CONMON="$(command -v conmon)"
export NIMBUS_KRUN_SMOKE_BUILDAH="$(command -v buildah)"
export NIMBUS_KRUN_SMOKE_HOST_PORT="18080"
export NIMBUS_KRUN_SMOKE_GUEST_PORT="8080"

cargo test -p nimbus-sandbox krun_backend_smoke_boots_http_service_and_survives_backend_restart -- --ignored --nocapture
```

## Expected outcomes

- The test reaches `SandboxStatus::Ready`
- A fresh `KrunSandboxBackend` instance can `inspect(...)` the running sandbox
- The guest HTTP service answers on `127.0.0.1:${NIMBUS_KRUN_SMOKE_HOST_PORT}`
- Logs persist under
  `${NIMBUS_KRUN_SMOKE_WORKDIR}/state/containers/<sandbox-id>/ctr.log` and
  `oci.log`
- `stop(...)` leaves the sandbox in `SandboxStatus::Stopped`

## Write-back contract

When this succeeds, record the following alongside the current task and compare
against the original closeout evidence in
`docs/plans/archive/vmm-infrastructure-plan.md`:

- exact `cargo test` command
- concrete `NIMBUS_KRUN_SMOKE_WORKDIR` path
- rootfs source and path
- observed sandbox id
- log file paths
- HTTP connectivity proof
- restart-survival proof (`inspect(...)` from a fresh backend instance)
- stop outcome and exit-status evidence
