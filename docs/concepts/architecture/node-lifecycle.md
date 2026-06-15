---
title: Node lifecycle
description: How a Nimbus node is installed and supervised, and how the node-side machinery drives systemd transient units over D-Bus with signal-correlated job completion.
sidebar:
  label: Node lifecycle
  order: 10
---

A Nimbus node is one host running the `nimbus` binary as a long-lived
service. Nimbus never self-daemonizes: the host's service manager owns the
process, and Nimbus's job is to make that ownership explicit, reviewable,
and reproducible. This page explains the machinery — the unit generation
surface, the D-Bus client seam, and the workload reconciler. For the
step-by-step install commands, see the
[operator guide](/operators/node-lifecycle/).

There are two distinct lifecycle layers, and they are deliberately
separate:

1. **The node service itself.** The CLI renders systemd or Podman Quadlet
   artifacts that the host service manager installs and supervises.
2. **Workloads on the node.** The `crates/nimbus-node` library drives
   short-lived systemd *transient* units over D-Bus — no unit files on
   disk, completion confirmed by signal rather than polling.

## Installing the node service

The `nimbus node` surface lives in `crates/nimbus-bin/src/node_service.rs`
and only ever mutates Linux hosts; on other platforms the mutating
subcommands refuse to run. It renders two families of artifacts:

- **Native systemd**: a `nimbus.service` unit that runs the binary in the
  foreground, plus an optional paired `nimbus.socket` when socket
  activation is requested.
- **Podman Quadlet**: a `nimbus.container` unit for the published
  container image, supervised by host systemd through Podman.

Scope selects the unit directory: system scope writes under the
system-wide systemd (or Quadlet) directories, user scope under the
per-user equivalents. Every render path supports a dry run that prints
the full artifact without touching the host, and `nimbus node doctor`
probes host support without mutating anything.

Generated units are treated as build outputs, not config files:

- Each artifact carries a provenance header — template version, the exact
  generating command, and a SHA-256 hash of the rendered content — so
  out-of-band edits are detectable and the fix is always "regenerate".
- Inputs are validated before rendering: unit paths are checked, and
  container image references must point at the published Nimbus registry
  path with a real version pin (a `latest` tag is rejected).
- `ExecStart` is always composed by Nimbus. There is no pass-through for
  raw unit text or arbitrary systemd sections.

With socket activation, systemd owns the TCP listener and the rendered
service starts the binary with a flag telling it to inherit the socket.
On boot, `crates/nimbus-bin/src/start/boot.rs` verifies the systemd
activation contract — exactly one passed file descriptor, addressed to
this process — before adopting the inherited listener instead of binding
its own.

## The D-Bus client seam

Workload supervision is built on a small trait in
`crates/nimbus-node/src/systemd_transient.rs`: `SystemdDbusClient`, with
four operations — report capabilities, start a transient unit, stop a
unit, and inspect a unit. The seam is fail-closed by design:

- The default implementation is an *unavailable* client. Anything built
  on top observes "systemd D-Bus is not available" until a real client
  is explicitly constructed, and the backend refuses to proceed when
  required capabilities are missing.
- The production client, `ZbusSystemdClient` in
  `crates/nimbus-node/src/systemd_transient/zbus_client/`, talks to
  `org.freedesktop.systemd1` over zbus with generated proxies. It probes
  capabilities once at construction time using a harmless manager query;
  if the bus or the manager interface is unreachable, the client reports
  degraded capabilities rather than failing later mid-operation.
- The client can attach to the system bus (production, where the node
  service runs) or the session bus (tests against a user systemd
  instance). The trait surface is identical on both.

A second implementation of the node's host-lifecycle seam,
`crates/nimbus-node/src/direct_process.rs`, runs workloads as in-memory
records with captured logs and evidence — it exists so the reconciler
and its callers can be tested deterministically without systemd.

## Transient units, not unit files

Tenant workloads become systemd *transient* units: created over D-Bus,
named `nimbus-<component>.service`, and gone when they stop. Nothing is
written under the systemd unit directories, so there is no drift between
files on disk and what is actually running.

The properties Nimbus will set on a transient unit are allowlisted —
description, slice, restart policy, restart delay, memory ceiling, CPU
weight, and task count. `ExecStart` is composed only from a
Nimbus-validated absolute executable path; a request carrying a raw
`ExecStart` or any property outside the allowlist is rejected before it
reaches the bus.

Because the unit is a real systemd unit, observability comes for free:
logs are queryable by the systemd unit field and by a Nimbus workload-id
journal field, and resource accounting lives at the unit's cgroup path
under the system slice.

## Signal-correlated job completion

Starting or stopping a unit over D-Bus returns a *job*, and the only
trustworthy way to learn the job's outcome is systemd's `JobRemoved`
signal. The ordering in
`crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs` is the
trust-critical part:

```text
Subscribe to manager signals
  -> open the JobRemoved signal stream
    -> call StartTransientUnit / StopUnit (returns a job path)
      -> wait for the JobRemoved whose job path matches
        -> classify the result string
```

The signal stream is established *before* the start or stop call is
issued. If it were opened afterward, a fast job could complete in the
gap and its `JobRemoved` would be lost — the classic race that polling
or subscribe-after-call designs hit under load.

Classification is conservative: `done` and `skipped` count as success,
and every other result — including result strings the client has never
seen — counts as failure. Waiting is bounded by a timeout (30 seconds by
default), so a lost signal degrades into a reported error rather than a
hang.

Inspection maps systemd's state pairs onto Nimbus workload states:
activating units are *submitted*, active-and-running units are
*running*, active units in other sub-states are *ready*, inactive units
are *stopped*, failed units are *failed*, and a unit systemd does not
know about is reported as stopped rather than as an error.

## The workload reconciler

`crates/nimbus-node/src/reconciler.rs` provides
`NodeWorkloadReconciler`, which converges observed unit state toward the
desired state derived from a tenant workload spec: an active spec should
be running, a deleting spec should be stopped.

Reconciliation is inspect-first. For a workload that should be running,
the reconciler inspects the unit; if it is already submitted, running,
or ready, the outcome is "observed running" and nothing is touched —
otherwise it issues a start and re-inspects. The stopped path is
symmetric. Each pass writes status evidence through a writer seam so the
observation that justified the outcome is recorded alongside it.

The live implementation entry point is the hidden node workload executor
(`crates/nimbus-bin/src/node_workload_executor.rs`), which constructs a
`NodeWorkloadReconciler` over the systemd transient backend and runs the
reconcile loop for one assigned workload per invocation. It is not part
of the public workload CLI. User-facing commands declare workload intent
through higher-level run, deploy, service, or sandbox resources; the node
executor is the mechanism that converges an assigned spec on a host.
What stays out of scope here is the control plane that decides *which*
workloads land on a node: multi-node scheduling and assignment are not
part of a single Nimbus process. The reconciler converges a node toward
the spec it has been given; it does not distribute workloads across a
cluster.

## Related pages

- [Run Nimbus as a service](/operators/node-lifecycle/) — installing,
  inspecting, and removing the node service.
- [Deploy to Linux](/operators/deploy-linux/) — a hand-written service
  unit, step by step.
- [CLI and codegen](/concepts/architecture/cli-codegen/) — how the
  binary that all of this ships in is put together.
- [Sandboxes and machines](/concepts/architecture/sandbox-machines/) —
  the isolation layers that workloads run inside.
