# EIH5 Minicloud Cgroup Memory Proof

Date: 2026-05-22

Host:

```text
user=nimbus
host=minicloud
kernel=6.12.88+deb13-amd64
os=Debian 13
```

Command from the Nimbus worktree:

```bash
ssh nimbus@192.168.4.29 'bash -s' < scripts/prove-linux-cgroup-memory-limit.sh
```

Nimbus hard-quota enforcement paths identified in code:

- `crates/nimbus-sandbox/src/backends/container/bundle.rs` lowers
  `SandboxResourceLimits.memory_limit_bytes` to OCI
  `linux.resources.memory.limit` and `cpu_count` to OCI CPU quota.
- `crates/nimbus-sandbox/src/backends/krun/bundle.rs` lowers krun memory
  limits into OCI `linux.resources.memory.limit`.
- `crates/nimbus-sandbox/src/backends/krun/vm/launch.rs` materializes
  `/.krun_vm.json` with explicit `cpus` and `ram_mib` for libkrun-backed
  microVM launches when both CPU and memory are requested.
- `crates/nimbus-sandbox/src/backends/oci/conmon.rs` passes
  `SandboxResourceLimits.log_limit_bytes` through conmon
  `--log-size-max`.
- `crates/nimbus-sandbox/src/backends/oci/resource_quota.rs` remains the
  per-tenant reservation/admission layer; it is not the hard enforcement
  layer.

Proof output:

```text
host=minicloud
kernel=6.12.88+deb13-amd64
cgroup_root=/sys/fs/cgroup
cgroup_path=/sys/fs/cgroup/nimbus-eih5-memory-117796
memory.max=33554432
memory.high=max
memory.swap.max=0
memory.events.before:
low 0
high 0
max 0
oom 0
oom_kill 0
oom_group_kill 0
bash: line 88: 117843 Killed                  timeout --kill-after=2s "${PROOF_TIMEOUT_SECONDS}s" sudo -n env NIMBUS_EIH5_CGROUP_PATH="${CGROUP_PATH}" NIMBUS_EIH5_ALLOC_CHUNK_BYTES="${ALLOC_CHUNK_BYTES}" python3 - <<'PY'
import os
import time

cgroup_path = os.environ["NIMBUS_EIH5_CGROUP_PATH"]
chunk_bytes = int(os.environ["NIMBUS_EIH5_ALLOC_CHUNK_BYTES"])

with open(os.path.join(cgroup_path, "cgroup.procs"), "w", encoding="utf-8") as procs:
    procs.write(str(os.getpid()))

chunks = []
while True:
    chunk = bytearray(chunk_bytes)
    for index in range(0, len(chunk), 4096):
        chunk[index] = 1
    chunks.append(chunk)
    time.sleep(0.005)
PY

allocation_exit_status=137
memory.events.after:
low 0
high 0
max 35
oom 1
oom_kill 1
oom_group_kill 0
result=pass
reason=cgroup-v2-memory-limit-fired
```

Conclusion:

- Linux cgroup v2 memory enforcement is available on the current proof host.
- The hard substrate fires independently of Nimbus reservation bookkeeping:
  the child process is killed under `memory.max`, and `memory.events` records
  both `oom` and `oom_kill`.
- Nimbus should keep launch-time reservation as the policy admission layer and
  continue lowering admitted limits into OCI/conmon/libkrun hard controls at
  the sandbox seam.
