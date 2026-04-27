# gVisor as a Lightweight Isolation Tier

Evaluation of gVisor (`runsc`) as an alternative or complement to hardware VM
isolation via libkrun.

**Date:** 2026-04-10
**Status:** Evaluated, deferred

---

## What gVisor Is

gVisor is a user-space kernel that intercepts application syscalls and
re-implements them in Go. It provides strong isolation without KVM or hardware
virtualization. `runsc` is a drop-in OCI runtime — compatible with Docker,
containerd, Podman, CRI-O.

**Production use:** Google trusts gVisor for Cloud Run, Cloud Functions, App
Engine, and GKE Sandbox at massive scale.

---

## Why It Was Evaluated

libkrun provides hardware VM isolation but requires KVM, libkrunfw, and a
~100ms cold-boot path. gVisor could provide a lighter isolation tier for
workloads that don't need full hardware VM boundaries.

---

## Advantages

- **No KVM required.** Works on any Linux host, including platforms without
  nested virtualization.
- **Lower memory overhead.** No guest kernel or guest userspace duplication.
  ~20-30MB baseline for the Go runtime.
- **Simpler operational model.** Single OCI runtime binary, no libkrun/libkrunfw
  dependency chain.
- **Comparable startup.** ~150-200ms boot, similar to or faster than libkrun
  cold boot.
- **Drop-in OCI runtime.** Can be used alongside crun — select per-container
  via `--runtime runsc`.

---

## Why Not for v1

1. **Syscall compatibility is not 100%.** gVisor implements ~70% of Linux
   syscalls. Standard workloads (Node.js, Python, Go) work well, but low-level
   systems code and some database internals may hit unimplemented syscalls.
   neovex's target workloads (postgres, redis) benefit from full kernel
   compatibility that hardware VMs provide.

2. **I/O overhead.** Every syscall goes through gVisor's user-space kernel,
   adding latency. I/O-heavy workloads see 2-10x overhead. CPU-bound workloads
   see minimal impact.

3. **No hardware isolation boundary.** gVisor dramatically reduces kernel attack
   surface but shares the host kernel at the ptrace/KVM boundary. Some
   compliance models require hardware-level VM isolation.

4. **No GPU or device passthrough.** If workloads need hardware access, gVisor
   cannot provide it.

---

## Potential Future Use

gVisor could serve as a **lightweight isolation tier** in a multi-tier model:

```
Tier 1: V8 isolates (sub-ms startup, trusted first-party code)
Tier 2: gVisor (150ms startup, untrusted code without hardware isolation)
Tier 3: libkrun microVM (100ms startup, full hardware isolation)
```

This would let neovex select isolation level based on trust level and workload
requirements, without the cost of a hardware VM for every workload.

---

## References

- [gVisor documentation](https://gvisor.dev/docs/)
- [GKE Sandbox](https://cloud.google.com/kubernetes-engine/docs/concepts/sandbox-pods)
- [gVisor syscall compatibility](https://gvisor.dev/docs/user_guide/compatibility/)
