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
- `/usr/libexec/nimbus/lib` populated by the matching `nimbus-libkrun`
  release archive
- the smoke command that launches the VM runs as root, matching the current
  root-owned Nimbus service path
- VMM foundation validation complete (`LH1` through `LH6`)

## Command

Build the ignored Linux-only integration test as the operator user, then run
the produced test binary under `sudo` so Cargo does not leave root-owned build
artifacts in the workspace:

```bash
cargo test -p nimbus-sandbox --test krun_linux_smoke \
  krun_backend_image_backed_smoke_pulls_and_boots_busybox \
  -- --ignored --list

test_bin="$(find target/debug/deps -maxdepth 1 -type f -perm -111 \
  -name 'krun_linux_smoke-*' | head -1)"

sudo env \
  NIMBUS_KRUN_SMOKE_WORKDIR="/tmp/nimbus-sandbox-smoke" \
  NIMBUS_KRUN_SMOKE_RUNTIME="/usr/libexec/nimbus/crun" \
  NIMBUS_KRUN_SMOKE_CONMON="$(command -v conmon)" \
  NIMBUS_KRUN_SMOKE_BUILDAH="$(command -v buildah)" \
  NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST="<host-lan-ip>" \
  "${test_bin}" \
  krun_backend_image_backed_smoke_pulls_and_boots_busybox \
  --ignored --nocapture
```

## Expected outcomes

- The test reaches `SandboxStatus::Ready`
- A fresh `KrunSandboxBackend` instance can `inspect(...)` the running sandbox
- The guest HTTP service answers on `127.0.0.1:18081` unless
  `NIMBUS_KRUN_IMAGE_SMOKE_HOST_PORT` overrides the default
- The non-loopback probe refuses `<host-lan-ip>:18081`, proving the patched
  bind-address hook kept TSI loopback-only
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
- observed sandbox id
- log file paths
- HTTP connectivity proof
- non-loopback refusal proof
- restart-survival proof (`inspect(...)` from a fresh backend instance)
- stop outcome and exit-status evidence
