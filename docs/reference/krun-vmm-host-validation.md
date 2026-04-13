# krun VMM Host Validation

This runbook is the operator-facing historical baseline for the krun-backed
VMM foundation. It defines the reproducible Linux-side commands and evidence
used to build the patched private `neovex-crun` binary, stage it at
`/usr/libexec/neovex/crun`, prepare the first OCI bundle recipe with
`krun.port_map` in `"host:guest"` form, and lay out the repeatable conmon
lifecycle drill.

On macOS, run this runbook inside the Linux machine guest described by
`docs/plans/distribution-plan.md` Channel 4, not on the macOS host. The VMM
stack itself remains Linux-only even when the developer entrypoint is a Mac.

Repo-owned helper entrypoints:

- `make check-vmm-host`
- `make collect-vmm-package-versions`
- `make prepare-linux-vmm-validation-bundle CRUN_SRC=~/src/github.com/containers/crun OUTPUT_ROOT=/tmp/neovex-linux-vmm-validation`
- `make collect-podman-machine-diagnostics MACHINE=neovex-libkrun-validation PROVIDER=libkrun OUTPUT_DIR=/tmp/neovex-libkrun-diagnostics`
- `make recreate-podman-machine MACHINE=neovex-libkrun-users-only PROVIDER=libkrun TMP_ROOT=/tmp/podman OUTPUT_DIR=/tmp/neovex-libkrun-users-only-recreate VOLUME=/Users:/Users`
- `make verify-crun-patch CRUN_SRC=~/src/github.com/containers/crun`
- `make build-neovex-crun CRUN_SRC=~/src/github.com/containers/crun OUTPUT=/tmp/neovex-crun-stage/crun`
- `make verify-neovex-crun-fedora-userspace CRUN_SRC=~/src/github.com/containers/crun OUTPUT_DIR=/tmp/neovex-crun-fedora-userspace-output WORK_DIR=/tmp/neovex-crun-fedora-userspace-build`
- `make prepare-krun-bundle BUNDLE_DIR=/tmp/neovex-krun-probe ROOTFS=/absolute/path/to/rootfs HOST_PORT=18080 GUEST_PORT=8080`
- `make verify-krun-bundle-helper`
- `make prepare-direct-krun-drill BUNDLE_DIR=/tmp/neovex-krun-probe STATE_ROOT=/tmp/neovex-direct-krun-drill`
- `make verify-direct-krun-drill-helper`
- `make verify-runtime-separation SYSTEM_RUNTIME=/usr/bin/crun PRIVATE_RUNTIME=/usr/libexec/neovex/crun`
- `make verify-runtime-separation-helper`
- `make prepare-conmon-krun-drill BUNDLE_DIR=/tmp/neovex-krun-probe STATE_ROOT=/tmp/neovex-conmon-drill`
- `make verify-conmon-krun-drill-helper`
- `make verify-linux-vmm-validation-bundle-helper`
- `make verify-podman-machine-recreate-helper`

## Supported Host Baseline

- Linux only
- If starting from macOS, execute these steps inside the machine VM guest
- Debian 13 or Fedora 40+ are the supported first targets
- `/dev/kvm` must exist
- the current user should be `root` or in the `kvm` group
- the pinned upstream source checkout is expected at
  `~/src/github.com/containers/crun`

## Generate The Linux Command Bundle

To minimize judgment on the Linux host, generate the numbered `LH1`-`LH6`
execution bundle first:

```bash
bash scripts/prepare-linux-vmm-validation-bundle.sh \
  --crun-source ~/src/github.com/containers/crun \
  --output-root /tmp/neovex-linux-vmm-validation
```

Or through `make`:

```bash
make prepare-linux-vmm-validation-bundle \
  CRUN_SRC=~/src/github.com/containers/crun \
  OUTPUT_ROOT=/tmp/neovex-linux-vmm-validation
```

This emits:

- `session.env` with the fixed paths and parameters for the Linux run
- `commands/00-run-through-lh6.sh` for the full sequence
- numbered `commands/01...11...` scripts for each queue step
- `99-writeback-checklist.txt` listing the artifact files to record alongside
  the current task or compare against the archived VMM foundation evidence

Recommended Linux-host entrypoint:

```bash
bash /tmp/neovex-linux-vmm-validation/commands/00-run-through-lh6.sh
```

If a queue item fails and needs focused reruns, execute the numbered scripts
individually from the same bundle instead of rebuilding the command sequence by
hand.

Optional macOS-only preflight before entering a Linux guest:

```bash
bash scripts/verify-neovex-crun-fedora-userspace.sh \
  --crun-source ~/src/github.com/containers/crun \
  --output-dir /tmp/neovex-crun-fedora-userspace-output \
  --work-dir /tmp/neovex-crun-fedora-userspace-build

file /tmp/neovex-crun-fedora-userspace-output/crun
```

This proves the patch and Linux userspace build helper on a Mac through Docker
Desktop, but it does **not** replace the Linux `/dev/kvm` validation required
by `LH1` through `LH6`.

## Optional macOS Machine Diagnostics

When the Podman-managed macOS guest lane is blocked, capture deterministic
artifacts before changing providers, deleting machines, or overwriting logs:

```bash
bash scripts/collect-podman-machine-diagnostics.sh \
  --machine neovex-libkrun-validation \
  --provider libkrun \
  --output-dir /tmp/neovex-libkrun-diagnostics
```

Or through `make`:

```bash
make collect-podman-machine-diagnostics \
  MACHINE=neovex-libkrun-validation \
  PROVIDER=libkrun \
  OUTPUT_DIR=/tmp/neovex-libkrun-diagnostics
```

The helper captures:

- Podman version, `podman info --debug`, `podman machine list`, and
  `podman machine inspect`
- the machine config JSON, discovered disk image path, and the standard
  Podman tmp-root listing
- the machine log tail plus any API, ready, and gvproxy socket paths
- matching `krunkit` / `gvproxy` / machine process output
- `system_profiler` hardware and software snapshots on macOS

Record the emitted `summary.txt` path, the log-tail path, and any failing
Podman command outputs in the VMM plan `Execution Log`. This helper is
best-effort and is meant to preserve macOS evidence; it does not replace the
Linux `LH1` through `LH6` closeout lane.

If the machine is already on the short-root path but still looks stale, use the
checked-in recreate helper instead of continuing ad hoc restart loops:

```bash
bash scripts/recreate-podman-machine.sh \
  --machine neovex-libkrun-users-only \
  --connection neovex-libkrun-users-only \
  --provider libkrun \
  --tmp-root /tmp/podman \
  --output-dir /tmp/neovex-libkrun-users-only-recreate \
  --cpus 2 \
  --memory 2048 \
  --disk-size 20 \
  --volume /Users:/Users
```

Or through `make`:

```bash
make recreate-podman-machine \
  MACHINE=neovex-libkrun-users-only \
  CONNECTION=neovex-libkrun-users-only \
  PROVIDER=libkrun \
  TMP_ROOT=/tmp/podman \
  OUTPUT_DIR=/tmp/neovex-libkrun-users-only-recreate \
  CPUS=2 \
  MEMORY=2048 \
  DISK_SIZE=20 \
  VOLUME=/Users:/Users
```

This helper:

- records the short-root socket-budget report
- captures pre-recreate diagnostics before deleting the machine
- force-removes the named machine
- reinitializes it with the known-good short-root `/tmp/podman` recipe
- starts it and captures a post-start readiness bundle

On the current host, the bundle at `/tmp/neovex-libkrun-users-only-recreate`
preserved the old missing API/gvproxy socket failure in
`pre-diagnostics/summary.txt`, then returned
`result ready info=ok ssh=ok` in `readiness/summary.txt` after recreate. Record
those artifact paths and the exact init/start command files in the VMM plan if
you repeat this repair.

## 1. Preflight The Host

Run the checked-in host probe first:

```bash
bash scripts/check-vmm-host.sh
```

Record the output in the VMM plan `Execution Log` before moving on. The command
returns non-zero if the host is not ready for Linux krun/conmon validation.

Then collect the package/version evidence the plan expects for `LH1` and `V2`:

```bash
bash scripts/collect-vmm-package-versions.sh
```

Record the exact output in the VMM plan alongside the host probe result.

## 2. Verify The Patch Against The Pinned Source

```bash
bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun
```

Record the exact source path that was verified.

## 3. Build And Stage The Patched Binary

Stage a private patched binary without touching the system runtime:

```bash
bash scripts/build-neovex-crun.sh \
  --source ~/src/github.com/containers/crun \
  --output /tmp/neovex-crun-stage/crun

/tmp/neovex-crun-stage/crun --version
```

The build helper copies the upstream checkout into a separate Linux build
directory, applies the checked-in patch there, and stages the resulting binary
without mutating the source checkout.

## 4. Install The Private Runtime Path

Install the private runtime path expected by the plan:

```bash
bash scripts/build-neovex-crun.sh \
  --source ~/src/github.com/containers/crun \
  --output /tmp/neovex-crun-stage/crun \
  --install-path /usr/libexec/neovex/crun \
  --sudo-install

/usr/libexec/neovex/crun --version
```

This path is private to neovex. It must not replace the distro `crun` binary.

## 5. Prepare The First Port-Mapping Bundle

```bash
bundle_dir=/tmp/neovex-krun-probe
rootfs=/absolute/path/to/buildah-mounted-rootfs

bash scripts/prepare-krun-bundle.sh \
  --bundle-dir "${bundle_dir}" \
  --rootfs "${rootfs}" \
  --host-port 18080 \
  --guest-port 8080
```

This helper runs `crun spec` inside the bundle directory unless `--skip-spec`
is provided, then updates `config.json` so these fields are present:

```json
{
  "root": {
    "path": "/absolute/path/to/buildah-mounted-rootfs",
    "readonly": false
  },
  "process": {
    "args": ["/bin/busybox", "httpd", "-f", "-p", "8080"],
    "cwd": "/"
  },
  "annotations": {
    "run.oci.handler": "krun",
    "krun.port_map": "18080:8080"
  }
}
```

Notes:

- The helper defaults `process.args` to `/bin/busybox httpd -f -p <guest-port>`
  for the first service-connectivity probe.
- Use repeated `--process-arg` flags to replace that with a service-specific
  command such as `postgres`.
- `run.oci.handler=krun` selects the libkrun handler when invoking the `crun`
  binary directly.
- `krun.port_map` must stay in `"host:guest"` form.
- `root.path` may be absolute or relative; crun `1.22` resolves either form.

For the repo-owned config transformation proof, run:

```bash
bash scripts/verify-krun-bundle-helper.sh
```

## 6. Prepare The Direct Private-Runtime Drill

Once the bundle exists and the private runtime is installed, create the
operator-visible state layout for the first direct
`/usr/libexec/neovex/crun run --bundle ...` drill:

```bash
bundle_dir=/tmp/neovex-krun-probe
state_root=/tmp/neovex-direct-krun-drill

bash scripts/prepare-direct-krun-drill.sh \
  --bundle-dir "${bundle_dir}" \
  --state-root "${state_root}" \
  --container-id neovex-http \
  --runtime /usr/libexec/neovex/crun
```

For the repo-owned preparation proof, run:

```bash
bash scripts/verify-direct-krun-drill-helper.sh
```

The helper generates these operator-facing scripts and files under
`${state_root}/containers/neovex-http/`:

- `run-runtime.sh`
- `start-runtime.sh`
- `probe-http.sh`
- `wait-for-http.sh`
- `wait-for-exit.sh`
- `show-exit-status.sh`
- `graceful-stop.sh`
- `force-stop.sh`
- `drill.env`

Suggested Linux-host sequence after preparation:

```bash
bash "${state_root}/containers/neovex-http/start-runtime.sh"
bash "${state_root}/containers/neovex-http/wait-for-http.sh"
bash "${state_root}/containers/neovex-http/probe-http.sh"
bash "${state_root}/containers/neovex-http/graceful-stop.sh"
bash "${state_root}/containers/neovex-http/show-exit-status.sh"
```

Notes:

- The helper derives the default probe port from the bundle's
  `krun.port_map` annotation, so the first connectivity command stays aligned
  with the OCI config being tested.
- The generated `probe-http.sh` assumes the first guest-service proof is the
  default BusyBox HTTP server from `scripts/prepare-krun-bundle.sh`. For a
  different service, keep the generated runtime start/stop bookkeeping but
  replace the probe command with a service-appropriate check.
- `run-runtime.sh` writes deterministic stdout, stderr, pid, launcher-pid, and
  exit-status files so `LH5` can record exact paths in the active VMM plan.

## 7. Prove The System Runtime Remains Untouched

Run the checked-in helper after the private install:

```bash
bash scripts/verify-runtime-separation.sh \
  --system-runtime /usr/bin/crun \
  --private-runtime /usr/libexec/neovex/crun
```

Or through `make`:

```bash
make verify-runtime-separation \
  SYSTEM_RUNTIME=/usr/bin/crun \
  PRIVATE_RUNTIME=/usr/libexec/neovex/crun
```

The helper records the system-runtime path and version, the private-runtime
path and version, the Podman runtime path, the realpaths used for comparison,
and the final separation result. Capture the full output in the VMM plan.

If you need to prove the helper behavior itself before running it on a host,
use:

```bash
bash scripts/verify-runtime-separation-helper.sh
```

The expected outcome is that system Podman continues to point at the distro
runtime path, while neovex uses `/usr/libexec/neovex/crun`.

## 8. Prepare The Conmon Lifecycle Drill

Once the bundle exists and the private runtime is staged, create the
operator-visible state layout for the first `conmon -> /usr/libexec/neovex/crun
-> guest` run:

```bash
bundle_dir=/tmp/neovex-krun-probe
state_root=/tmp/neovex-conmon-drill

bash scripts/prepare-conmon-krun-drill.sh \
  --bundle-dir "${bundle_dir}" \
  --state-root "${state_root}" \
  --container-id neovex-http \
  --name neovex-http \
  --conmon /usr/bin/conmon \
  --runtime /usr/libexec/neovex/crun
```

For the repo-owned preparation proof, run:

```bash
bash scripts/verify-conmon-krun-drill-helper.sh
```

The helper generates these operator-facing scripts and files under
`${state_root}/containers/neovex-http/`:

- `run-conmon.sh`
- `find-attach-sockets.sh`
- `capture-process-tree.sh`
- `wait-for-exit.sh`
- `show-exit-status.sh`
- `graceful-stop.sh`
- `force-stop.sh`
- `drill.env`

Suggested Linux-host sequence after preparation:

```bash
bash "${state_root}/containers/neovex-http/run-conmon.sh"
bash "${state_root}/containers/neovex-http/find-attach-sockets.sh"
bash "${state_root}/containers/neovex-http/capture-process-tree.sh"
curl -fsS http://127.0.0.1:18080/
bash "${state_root}/containers/neovex-http/graceful-stop.sh"
bash "${state_root}/containers/neovex-http/show-exit-status.sh"
```

Notes:

- The generated `run-conmon.sh` uses Podman-style conmon arguments, including
  `--persist-dir`, `--conmon-pidfile`, and runtime log redirection through
  `--runtime-arg --log`.
- The helper intentionally does not guess the attach-socket basename. Use the
  generated `find-attach-sockets.sh` script after the VM is running and record
  the concrete absolute socket path that appears on the Linux host.
- The generated `graceful-stop.sh` targets the runtime pid from `pidfile`
  because the long-lived `crun` process is the VMM in the krun model.

## 9. Evidence To Record In The Plan

When a Linux host completes this runbook, record all of the following in the
current task notes or issue, and compare against the original closeout record
in `docs/plans/archive/vmm-infrastructure-plan.md`:

- host OS and version
- upstream source path
- staged binary path
- install path
- patched binary identity output
- exact build/install commands
- bundle path
- `krun.port_map` value used
- process args used for the first guest-service probe
- exact generated direct-runtime command or the `run-runtime.sh` path used
- absolute stdout, stderr, pid, launcher-pid, and exit-status paths for the
  direct private-runtime drill
- host-to-guest connectivity probe command and observed outcome
- system `crun` and Podman runtime-path proof
- exact generated conmon command or the `run-conmon.sh` path used
- absolute log, exit, pid, conmon-pid, persist, and attach-socket paths
- process-tree capture command and observed output
- graceful-stop command and final exit-status output
