# macOS Host-vs-Guest Control-Plane Rationale

Why Neovex originally preferred a guest-resident authoritative server on macOS
developer machines, why the hybrid alternative stayed viable, how both map to
the landed Linux baseline, and why a DX-first evaluation ultimately switched
the active macOS plan to the hybrid direction.

This document captures the evaluated tradeoff space as of 2026-04-13 so future
contributors do not re-derive the same architecture fork from scratch. It is a
research rationale, not an execution plan. The active execution control plane
remains `docs/plans/macos-machine-support-plan.md`.

---

## The Question

On Linux production today, Neovex runs on a Linux node and activates workload
services that run inside isolated microVMs. On macOS developer machines, we
already know the workload services themselves must run inside one Linux machine
VM.

The open design question is:

- should Neovex itself keep running on the macOS host and remotely manage guest
  containers?
- or should the macOS host be only a thin launcher/proxy while the
  authoritative Neovex server runs inside the Linux guest next to the guest
  container runtime?

Both are viable. They are not equally close to the Linux baseline.

---

## Current Recommendation Status

The active macOS execution plan now prefers:

- a **host-resident authoritative `neovex serve`**
- a **narrow guest machine-API seam**
- **guest-resident standard containers** for services

In short:

```text
macOS host
  -> neovex serve
  -> V8 runtime / storage / API
  -> machine lifecycle
  -> forwarded host `<machine>-api.sock`
  -> local port exposure / proxying

Linux guest VM
  -> neovex.socket / neovex.service
  -> buildah + conmon + crun
  -> service containers
```

This is the former "Option A" hybrid model:

- host-resident Neovex runtime/storage/server
- guest-resident service containers only

The earlier guest-authoritative model remains worth preserving in this document
because it is still the simpler and more Linux-topology-faithful alternative.
But after the Podman source review and the explicit DX prioritization, the
active macOS control plane switched to the hybrid path for `MAC5` and `MAC6`.

---

## The Two Options

### Option A: Hybrid host-resident Neovex, guest-resident containers

```text
macOS host
  -> neovex serve
      -> V8 runtime
      -> engine / service registry
      -> storage access
      -> remote service control into guest

Linux guest VM
  -> buildah + conmon + crun
  -> service containers
```

### Option B: Guest-resident authoritative Neovex

```text
macOS host
  -> neovex machine ...
  -> neovex serve    (thin wrapper / proxy)
  -> neovex service ...

Linux guest VM
  -> neovex serve
      -> V8 runtime
      -> engine / service registry
      -> storage access
      -> buildah + conmon + crun
      -> service containers
```

---

## Why Option B Is Closer To Linux

This depends on what we mean by "host."

There are two useful meanings:

- **physical host**: the actual machine a developer owns
- **platform host**: the OS environment that directly owns the workload runtime

On Linux production, these are the same machine:

```text
physical host = platform host = Linux node
  -> neovex serve
  -> buildah + conmon + crun + libkrun
  -> service microVMs
```

On macOS, they are not:

```text
physical host = macOS
platform host = Linux guest VM
```

The landed Linux baseline is not merely "Neovex runs on the same physical
machine as the workloads." The deeper invariant is:

- Neovex runs on the same **platform host** that owns the container / sandbox
  runtime it manages.

Option B preserves that invariant on macOS:

- Neovex runs inside the Linux guest
- the Linux guest also owns buildah/conmon/crun and service state

Option A breaks it:

- Neovex would run on macOS
- but the actual service runtime would live on a remote Linux node from
  Neovex's perspective

So the hybrid is closer only if we optimize for "same physical laptop."
Option B is closer if we optimize for "same control-plane placement relative to
the workload runtime."

That second meaning is the more important one for architecture.

---

## Where The Linux Baseline Actually Draws The Boundary

The current baseline says:

- `neovex-server` owns service activation and `ctx.services.*`
- Linux request-time activation runs through the local server-owned service
  manager
- macOS v1 should not add a second host-side orchestration path

See:

- `docs/reference/microvm-service-baseline.md`

The Linux request path today is:

```text
compose.yaml / image / build context
  -> neovex-bin validates and lowers service intent
  -> neovex-server owns declared services and activation
  -> ctx.services.<name> triggers ensure_service_binding(...)
  -> neovex-sandbox krun backend materializes OCI bundle + state
  -> conmon -> patched crun -> libkrun VM
  -> guest service answers via host-side binding
```

That is a **local control plane** relative to the sandbox runtime.

Option A would turn that into a **remote control plane** for macOS:

- host Neovex would have to manage a guest Linux container runtime remotely

Option B keeps the same local-control-plane property:

- guest Neovex manages guest containers locally

---

## Why The Podman Analogy Still Matters

Podman's macOS model is:

- thin host CLI
- one Linux machine VM
- guest-resident Podman engine
- guest-resident containers

Neovex is not Podman, but Podman's topology is still the right reference.

The important distinction is product scope:

- **Podman** productizes a generic remote container engine
- **Neovex** productizes a database/runtime/server that also manages service
  containers

That means Neovex should copy:

- the host/guest topology
- the machine lifecycle layering
- the local-wrapper to guest-authority model

But Neovex does **not** need:

- a generic user-facing remote-engine abstraction
- a registry of arbitrary remote engines/connections
- Docker-compat connection semantics as the primary model

So the host is still talking to something remote in both products.
The difference is:

- Podman host config is centered on **remote engine connection management**
- Neovex host config should be centered on **booting and reaching one
  guest-resident Neovex control surface**

---

## What Podman Actually Does For DX

Podman's macOS developer experience is strong because it makes a guest-resident
engine feel local enough for ordinary workflows.

The source-backed pieces are:

- `gvproxy` forwards a guest unix socket to a host-local unix socket
- machine readiness waits for both machine state and localhost SSH reachability
- optional helper logic can claim `/var/run/docker.sock` through a stable link
  structure, but that is additive rather than the core machine model

Relevant source anchors:

- `pkg/machine/shim/networking.go`
- `pkg/machine/shim/networking_unix.go`
- `pkg/machine/ssh.go`

The important product lesson is not "copy Podman's engine API." It is:

- local DX improves dramatically when the host can talk to a guest-owned
  control socket as if it were local

That pattern can support either Neovex option:

- guest-resident authoritative Neovex server
- host-resident Neovex with a narrower guest `neovex.sock` / machine-API
  socket

---

## DX Lens: Why Option A Reopens The Question

The strongest reason to keep exploring the hybrid model is developer feedback
speed, especially for engineers and AI agents iterating on application code.

With Option A:

- V8/runtime execution stays on the Mac
- storage access stays on the Mac
- request/response debugging stays on the Mac
- file watching and source edits stay on the Mac
- agents can run and inspect the authoritative app/server process without first
  crossing a guest shell/proxy boundary

For service-backed app development, this can feel materially better:

- runtime-only changes can feedback immediately
- `ctx.db.*` and local persistence introspection stay host-native
- only service activation crosses into the guest
- guest containers become a backing dependency rather than the home of the
  whole Neovex stack

This is the best argument for Option A, and it is stronger for AI agents than
for many human workflows because agents often optimize for:

- short edit-run-observe loops
- direct local process/log access
- minimal indirection between source edits and observed behavior

---

## Podman-Reusable Seams If We Choose Option A

If Neovex adopts the hybrid model, Podman's source and helper topology are
still highly reusable.

### 1. Machine lifecycle and image/materialization

Keep reusing the same Podman-aligned pieces:

- `krunkit`
- `gvproxy`
- Fedora CoreOS machine image
- Linux-side image build and publish flow
- virtiofs host-path sharing

### 2. Host-local control transport

Podman's `gvproxy` + forwarded guest socket pattern is directly useful.

Neovex hybrid could expose a guest socket such as:

```text
/run/neovex/neovex.sock
```

and forward it to a host-local socket such as:

```text
~/.local/state/neovex/machine/<name>-api.sock
```

The host Neovex process would then talk to that socket through a local client,
just as Podman makes the guest engine feel local.

### 3. Readiness and recovery

Podman's readiness model is also reusable:

- machine process state
- SSH reachability
- forwarded socket reachability

For Neovex hybrid, the layered readiness model would become:

- machine ready
- guest machine-API socket reachable
- guest service container reachable

### 4. Published ports

Keep using `gvproxy` and machine port-forwarding for published guest service
ports.

The hybrid model does not require inventing a new data-plane technology. It
only changes where Neovex's authoritative application/runtime process lives.

---

## If We Choose Option A, Keep The Guest API Narrow

The biggest architectural risk in the hybrid model is accidentally rebuilding a
generic remote engine like Podman.

Avoid that by keeping the guest API narrow and Neovex-specific.

The guest should not become "a Podman replacement API." It should expose only
the runtime operations the host Neovex server needs:

- ensure service
- stop service
- inspect service state
- stream logs
- report published bindings
- report readiness / liveness
- build image from shared context when needed

That keeps the product seam as:

- Neovex host server
- Neovex guest machine API

rather than:

- generic host engine client
- generic guest engine server

---

## DX-First Rewrite Of MAC5 And MAC6

If developer and AI-agent adoption become the dominant goal, the active macOS
plan should be rewritten along these lines:

### Revised MAC5: Remote guest machine-API control seam

Repo outputs:

- guest `neovex.socket` / `neovex.service` inside the Linux machine
- host client for that guest machine API
- forwarded host-local socket path
- typed remote lifecycle protocol for service operations only
- focused integration tests for socket forwarding and remote service-control

Acceptance criteria:

- host Neovex can ensure, stop, and inspect guest containers through the
  forwarded guest socket
- guest logs and published port bindings are visible to host Neovex without
  SSH-driven ad hoc shelling
- the protocol remains Neovex-specific and does not expand into a generic
  container-engine surface

### Revised MAC6: Host-resident `neovex serve`

Repo outputs:

- mac-aware host-resident `neovex serve`
- host runtime/storage path remains authoritative on macOS
- `ctx.services.*` activation routes through the guest machine-API client
- docs for hybrid host-runtime plus guest-services workflow

Acceptance criteria:

- `neovex serve` on macOS runs the authoritative Neovex API on the host Mac
- service activation reaches guest containers through the remote guest
  machine-API seam
- ordinary runtime/storage edits do not require restarting or re-entering the
  guest
- developers and AI agents can get fast local feedback while still using real
  Linux guest services

---

## Updated DX Tradeoff

When the scorecard is optimized for operational simplicity and Linux-topology
parity, Option B remains stronger.

When the scorecard is optimized for developer and AI-agent feedback speed,
Option A becomes much more competitive:

- local runtime loop improves
- local storage/debug loop improves
- guest service realism is preserved
- Podman's forwarded-socket pattern gives us a credible implementation path

So the right framing is no longer:

- "Option A is just worse"

It is:

- "Option B is simpler and closer to the current Linux control-plane shape"
- "Option A may be the better macOS DX architecture if we value local feedback
  speed over strict control-plane parity"

---

## Tradeoff Matrix

| Concern | Option A: host Neovex, guest containers | Option B: guest Neovex, guest containers |
| --- | --- | --- |
| Linux control-plane parity | weaker | stronger |
| Podman topology alignment | weaker | stronger |
| V8/debugging on macOS host | stronger | weaker |
| Storage directly on macOS host | stronger | weaker |
| Remote path translation complexity | higher | lower |
| Service lifecycle/log/restart ownership | split across host+guest | local to guest |
| Risk of host/guest state divergence | higher | lower |
| Need for custom remote service-control protocol | higher | lower |
| Local iteration speed for pure runtime work | potentially stronger | potentially weaker |
| Developer / AI-agent feedback loop | stronger | weaker |
| Architectural simplicity for v1 | weaker | stronger |

---

## The Strongest Arguments For Option A

These are real advantages and should not be dismissed:

- V8 runtime and database access stay native to the developer's Mac process.
- Storage can stay directly on host paths without crossing a guest process
  boundary.
- macOS-local debugging for runtime code may be easier.
- For requests that only touch storage/runtime and never activate services,
  the path can be shorter.

If Neovex's product value on macOS were dominated by host-native runtime
development and only lightly touched guest-managed services, this option would
deserve stronger consideration.

---

## The Strongest Arguments Against Option A

These are the reasons it is not the default v1 recommendation:

### 1. It creates a new remote control-plane problem

The host Neovex process would need to manage:

- guest container create/start/stop
- guest logs
- guest readiness/liveness
- guest restart outcomes
- guest port publication
- guest build contexts
- guest recovery/reconciliation after partial failure

That is not the same problem Neovex solves on Linux today.

### 2. It introduces split-brain state

With Option A:

- the Neovex service registry and runtime state live on macOS
- the actual container runtime state lives in the Linux guest

That invites disagreement during failures:

- host thinks a service is running, guest disagrees
- guest logs exist, host has stale manifest state
- path mappings differ between host and guest

Option B keeps service-control authority and container-runtime state in the
same environment.

### 3. It makes path and artifact ownership harder

Compose files, build contexts, mounted paths, logs, and image state become a
cross-OS ownership problem:

- host paths are Darwin paths
- guest runtime needs Linux paths
- buildah/conmon/crun run in the guest

Option B still has file sharing, but the control plane that reasons about
those paths lives in the guest where the runtime executes.

### 4. It pushes Neovex toward becoming a generic remote engine client

Even if we do not expose that product seam publicly, Option A nudges the
implementation toward:

- remote service-control transport
- host-side state cache
- guest reconciliation protocol

That is essentially a mini remote-engine problem.

---

## Storage Nuance

One of the strongest objections to Option B is storage.

Important nuance:

- **guest-resident Neovex does not require guest-only data**

The guest Neovex process can still read/write host-backed project paths or
state roots through `virtiofs`.

So the choice is not:

- Option A: host-backed data
- Option B: guest-only data

The real choice is:

- where the **authoritative process** runs
- not whether data ultimately lives on host-backed storage

This weakens one of the main objections to guest-resident Neovex.

---

## Recommendation Status

The repo should now treat Option A as the active macOS execution direction.

That does **not** mean the guest-authoritative Option B analysis was wrong.
Option B is still:

- simpler
- closer to the landed Linux control-plane shape
- closer to Podman's own guest-authoritative engine placement

What changed is the product priority for macOS:

- local developer and AI-agent feedback speed became important enough to accept
  the extra remote-runtime seam

So the practical recommendation is now:

1. keep the active macOS plan on Option A
2. preserve Podman's machine topology, forwarded-socket pattern, and readiness
   layering
3. keep the guest protocol narrow and Neovex-specific
4. do not let the hybrid seam expand into a generic remote container engine

Option B remains the strongest fallback if the hybrid seam proves too complex
or too failure-prone in practice.

---

## What Could Reopen This Decision Later

Re-evaluate the choice back toward the guest-authoritative model if one of
these becomes dominant:

- the remote guest machine-API seam proves too complex or too failure-prone
- host↔guest path translation and reconciliation bugs dominate the macOS story
- the hybrid design starts expanding toward a generic remote-engine protocol
- guest-resident V8/runtime on macOS turns out to be fast and debuggable
  enough that the extra host-local DX benefit is no longer worth the split
- service management becomes dominant enough that keeping control and execution
  co-located in the guest is more valuable than the host-local runtime loop

If those risks dominate in practice, Option B may again become the better
follow-on design even after the current DX-first switch.
