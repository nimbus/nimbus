---
title: Node lifecycle
description: Install, inspect, and remove the systemd or Podman Quadlet service that runs a Nimbus node — generated units, socket activation, diagnostics, and Compose export.
sidebar:
  label: Node lifecycle
  order: 9
---

Nimbus does not self-daemonize. A node is either a foreground process or a
service owned by the host's service manager. On Linux, `nimbus node`
generates, installs, and manages those service-manager artifacts for you —
the `nimbus node` commands are Linux-only and refuse to mutate other
hosts.

Pick the mode that matches your deployment:

| Mode | Command | Lifecycle owner |
| --- | --- | --- |
| Local development | `nimbus dev`, `nimbus start`, `nimbus compose up` | Your terminal (foreground) |
| Native Linux service | `nimbus node install --systemd` | systemd, via generated `nimbus.service` (+ optional `nimbus.socket`) |
| Containerized service | `nimbus node install --container --image ...` | systemd + Podman, via generated `nimbus.container` Quadlet |
| Compose-derived artifacts | `nimbus compose export quadlet` | You — review, then install manually |

If you prefer to write the unit file yourself, the
[Linux deployment tutorial](/operators/deploy-linux/) walks through a
hand-written `nimbus.service` step by step.

## Native systemd service

### 1. Review the generated units

Always dry-run first. It prints every artifact in full without touching
the host:

```bash
nimbus node install --systemd --dry-run
```

The generated `nimbus.service` runs the binary in the foreground with
`Restart=on-failure` and sandboxing directives (`NoNewPrivileges`,
`PrivateTmp`, `ProtectHome`, `ProtectSystem=full`). Defaults:

| Setting | Default | Override |
| --- | --- | --- |
| Binary | `/usr/local/bin/nimbus` | `--binary <path>` |
| Data directory | `/var/lib/nimbus/data` | — |
| Control directory | `/var/lib/nimbus/control` | — |
| Bind | `127.0.0.1:8080` | — |
| Unit location (system) | `/etc/systemd/system/` | `--user` writes to `~/.config/systemd/user/` |

The rendered unit owns `ExecStart` — there is no flag for raw unit text,
arbitrary systemd sections, or a custom `ExecStart` line.

### 2. Install, enable, start

```bash
sudo nimbus node install --systemd --system --enable --now
```

`--system` is the default scope; pass `--user` for a user service
instead. Re-running against existing artifacts requires `--overwrite`.

### 3. Check status and logs

```bash
nimbus node status --systemd --system
nimbus node logs --systemd --system --follow
```

These wrap `systemctl status nimbus.service` and
`journalctl -u nimbus.service`, which work just as well directly:

```bash
systemctl status nimbus.service
journalctl -u nimbus.service -f
```

### Socket activation (optional)

To let systemd own the TCP listener and start Nimbus on first
connection, render a paired `nimbus.socket`:

```bash
nimbus node install --systemd --socket-activation --dry-run
sudo nimbus node install --systemd --system --socket-activation --enable --now
```

The socket listens on `127.0.0.1:8080` and the service starts with
`--systemd-socket-activation` to inherit it.

## Containerized service (Podman Quadlet)

The published image runs `nimbus` directly — it does not run systemd
inside the container. Host systemd and Podman own the lifecycle through a
generated Quadlet `nimbus.container` unit.

Rotate the admin token against the state volume first — the
containerized service binds a published port, and Nimbus refuses
non-loopback binds with a stale admin token (see
[Container image](/operators/container-image/)):

```bash
podman volume create nimbus-data
podman run --rm -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> auth rotate-admin
```

Then review, diagnose, and install:

```bash
nimbus node install --container \
  --image ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --user --dry-run
nimbus node doctor --container --user
nimbus node install --container \
  --image ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --user --enable --now
```

The Quadlet unit publishes `127.0.0.1:8080:8080`, mounts
`nimbus-data:/var/lib/nimbus`, and configures a `/health` health check.
It lands in `~/.config/containers/systemd/` (`--user`) or
`/etc/containers/systemd/` (`--system`).

Pin the image by version and digest from the release — `latest` is not a
production pin.

## Diagnose and uninstall

`nimbus node doctor` probes host support for the selected mode without
mutating anything:

```bash
nimbus node doctor --systemd --system
nimbus node doctor --container --user
```

Uninstall removes the generated artifacts and reloads systemd; dry-run it
first to see exactly which files go:

```bash
nimbus node uninstall --systemd --system --dry-run
sudo nimbus node uninstall --systemd --system
```

## Regenerate, don't hand-edit

Every generated unit carries provenance comments (template version,
generating command, content hash). If a generated file changed outside
Nimbus, don't patch it — rerun `nimbus node install` with `--overwrite`
so the artifact matches a command you can review and repeat.

Tenant workloads scheduled by a running Nimbus server are a separate
path that never produces unit files for you to install or edit. The
node-side machinery for supervising them as short-lived systemd
transient units exists in the binary but is not yet wired into the
running server — see
[Node lifecycle](/concepts/architecture/node-lifecycle/) for the
current state of that seam.

## Export Quadlet files from Compose

`nimbus compose export quadlet` turns an admitted Compose plan into
Quadlet artifacts for review — useful for migrations and small static
deployments. Nothing is installed; you review and place the files
yourself:

```bash
nimbus compose export quadlet --strict
nimbus compose export quadlet --mode pod --output-dir ./quadlet --overwrite
nimbus compose export quadlet --mode kube --podman-version 5.6.0
```

`--mode` selects `containers` (default), `pod`, or `kube` output. The
exporter warns when Compose features are omitted or need review;
`--strict` fails on any warning. It does not emit raw Podman arguments,
host networking, privileged mode, or arbitrary systemd sections.

## Machine OS images

If you run Nimbus's managed machine VM (the macOS developer-machine
flow), the matching binary and systemd units are baked into the
`ghcr.io/nimbus/machine-os` image — don't run `nimbus node install`
inside the guest. Manage the OS image from the host:

```bash
nimbus machine status
nimbus machine os upgrade --dry-run
nimbus machine os apply docker://ghcr.io/nimbus/machine-os:vX.Y.Z@sha256:<digest>
```

## Next steps

- [Deploy to Linux](/operators/deploy-linux/) — the full tutorial,
  including the dedicated service user and admin token.
- [Container image](/operators/container-image/) — image contract,
  digest pinning, Compose and Kubernetes.
- [Hardening](/operators/hardening/) — additional systemd sandboxing for
  the service unit.
- [Updates](/operators/updates/) — moving an installed node to a new
  release.
