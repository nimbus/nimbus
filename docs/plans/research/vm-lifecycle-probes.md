# VM Lifecycle Probes and Graceful Shutdown

Research into canonical lifecycle probe patterns, health check models, and
graceful shutdown mechanisms for neovex's microVM runtime.

**Date:** 2026-04-10
**Status:** Research complete

---

## Cross-Platform Probe Model Comparison

### The Kubernetes Three-Probe Model (Gold Standard)

| Probe | Purpose | Initial State | On Failure |
|-------|---------|---------------|------------|
| **startupProbe** | App has finished booting | Unknown | Kill + restart |
| **livenessProbe** | App is not deadlocked | Success (assumed) | Kill + restart |
| **readinessProbe** | App can serve traffic | Failure (no traffic until proven ready) | Remove from endpoints (no kill) |

Key: startup probe disables liveness/readiness until it passes. This prevents
slow-starting apps (JVM, ML models) from being killed during boot.

**Configurable fields (shared by all three probes):**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `initialDelaySeconds` | i32 | 0 | Wait before first probe |
| `periodSeconds` | i32 | 10 | Interval between probes (min 1) |
| `timeoutSeconds` | i32 | 1 | Per-probe timeout (min 1) |
| `failureThreshold` | i32 | 3 | Consecutive failures before action |
| `successThreshold` | i32 | 1 | Consecutive successes to mark healthy (must be 1 for liveness/startup) |

**Probe mechanisms (mutually exclusive per probe):**
- `httpGet`: path, port, scheme, headers → success = 2xx status
- `tcpSocket`: port → success = connection established
- `exec`: command → success = exit code 0
- `grpc`: port, service → gRPC Health Checking Protocol

**Source:** [`kubernetes/kubernetes` `pkg/kubelet/prober/worker.go`](https://github.com/kubernetes/kubernetes/blob/master/pkg/kubelet/prober/worker.go),
[`prober.go`](https://github.com/kubernetes/kubernetes/blob/master/pkg/kubelet/prober/prober.go),
[`prober_manager.go`](https://github.com/kubernetes/kubernetes/blob/master/pkg/kubelet/prober/prober_manager.go)

### Docker HEALTHCHECK

Single probe model with a `start_period` grace window.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `interval` | 30s | Time between probes |
| `timeout` | 30s | Max probe duration |
| `retries` | 3 | Consecutive failures → unhealthy |
| `start_period` | 0s | Grace period; failures don't count |
| `start_interval` | 5s | Probe frequency during start_period |

**States:** `starting` → `healthy` → `unhealthy` (and back to `healthy` on success)

**Source:** [`moby/moby` `daemon/health.go`](https://github.com/moby/moby/blob/master/daemon/health.go)

### Fly.io Checks

```toml
[checks.my_check]
  type = "http"          # "http" or "tcp"
  port = 8080
  path = "/healthz"      # HTTP only
  interval = "15s"
  timeout = "10s"
  grace_period = "30s"   # wait after start before first check
```

Top-level checks are monitoring-only. Service-level checks (`[[http_service.checks]]`)
affect traffic routing.

### systemd sd_notify

Push-based readiness (process declares "I'm ready") vs pull-based probes (prober asks "are you ready?").

| Message | Purpose |
|---------|---------|
| `READY=1` | Startup complete |
| `WATCHDOG=1` | Periodic liveness heartbeat |
| `STOPPING=1` | Beginning graceful shutdown |
| `STATUS=<text>` | Free-form status for `systemctl status` |

**Watchdog pattern:** If `WATCHDOG=1` is not received within `WatchdogSec`,
systemd sends SIGABRT and restarts the service. Catches internal deadlocks
that would still respond to TCP/HTTP probes.

### Nomad check_restart

```hcl
check_restart {
  limit           = 3      # failures before restart
  grace           = "90s"  # startup grace period
  ignore_warnings = false
}
```

### AWS ECS

Single `healthCheck` block, similar to Docker: `command`, `interval` (5-300s),
`timeout` (2-60s), `retries` (1-10), `startPeriod` (0-300s).

---

## Graceful Shutdown Patterns

### The Universal Pattern

Every platform follows the same sequence:

```
Shutdown requested
    │
    ├── [Optional: PreStop hook / drain / deregister]
    │
    ├── Send configurable signal to PID 1
    │
    ├── Wait grace period
    │
    └── SIGKILL (unconditional)
```

### Platform Defaults

| Platform | Signal | Default Grace | Configurable |
|----------|--------|---------------|-------------|
| Kubernetes | SIGTERM | 30s | `terminationGracePeriodSeconds` |
| Docker | SIGTERM | 10s | `docker stop -t` |
| Fly.io | SIGINT | 5s | `kill_signal`, `kill_timeout` |
| systemd | SIGTERM | 90s | `KillSignal`, `TimeoutStopSec` |
| AWS ECS | SIGTERM | 30s | `stopTimeout` |

### PID 1 Signal Problem

Linux kernel ignores signals sent to PID 1 unless PID 1 explicitly registers
handlers. Two standard solutions:

**[tini](https://github.com/krallin/tini):** Built into Docker (`--init`).
Spawns workload as child, forwards all signals, reaps zombies.

**[dumb-init](https://github.com/Yelp/dumb-init):** Same pattern + signal
rewriting support (`--rewrite 15:3`). Creates a new session via `setsid()`,
sends signals to the entire process group.

Both are ~300 lines of C. The pattern is: PID 1 registers signal handlers →
forks child for workload → forwards signals to child → reaps zombies → exits
with child's exit code.

---

## Graceful Shutdown in libkrun (Neovex-Specific)

### What libkrun provides (and doesn't)

| Mechanism | Available | Notes |
|-----------|-----------|-------|
| SIGTERM forwarding to guest | **No** | SIGTERM has no handler; default behavior kills VMM |
| `krun_get_shutdown_eventfd()` | **macOS only** (aarch64) | Returns -EINVAL on Linux |
| ACPI power button | **No** | No ACPI PM device emulated |
| i8042 Ctrl+Alt+Del | Exists internally | **Not exposed** via public API |
| vsock | **Yes** | `krun_add_vsock_port()` available |
| Built-in init signal handling | **No** | `init.c` does not handle SIGTERM |

**Conclusion:** The only viable graceful shutdown path on Linux is **vsock**.

### The vsock shutdown protocol (recommended)

Modeled on Kata Containers (ttrpc over vsock) and libkrun's own AWS Nitro
signal proxy (`src/aws_nitro/src/enclave/proxy/proxies/signal_handler.rs`).

```
neovex (host)                              Guest VM
     │                                          │
     │  1. Connect to vsock port 10000          │
     ├─────────────────────────────────────────►│
     │                                          │
     │  2. Send: SHUTDOWN {grace_period: 30s}   │
     ├─────────────────────────────────────────►│
     │                                          │
     │                        3. neovex-init receives message
     │                        4. SIGTERM → workload process group
     │                        5. waitpid with timeout (grace_period)
     │                        6. If timeout: SIGKILL remaining
     │                        7. set_exit_code via virtiofs ioctl
     │                        8. exit(0) → kernel panic → VM exit
     │                                          │
     │  9. child.wait() returns exit code       │
     │◄─────────────────────────────────────────┤
     │                                          │
     │  [Fallback: if no response within        │
     │   total timeout, SIGKILL the helper]     │
```

### Why this requires a custom guest init (neovex-init)

libkrun's built-in `init.c` (at `init/init.c:1230-1275`):
- Forks and execs the workload
- Waits for the workload to exit naturally
- Reports exit code via virtiofs ioctl
- **Does NOT listen on vsock**
- **Does NOT handle any signals**
- **Has no grace period logic**

neovex needs a custom init that adds: vsock listener, signal forwarding
(tini/dumb-init style), grace period, and zombie reaping.

### neovex-init design (~200 lines Rust, musl static binary)

```rust
// Simplified structure
fn main() {
    mount_filesystems();           // /proc, /sys, /dev, etc.
    let config = read_oci_config(); // .krun_config.json
    
    // Start vsock shutdown listener (background thread)
    let shutdown_rx = start_vsock_listener(SHUTDOWN_PORT);
    
    // Fork and exec workload
    let child_pid = fork_exec(&config.entrypoint, &config.cmd, &config.env);
    
    // Main loop: reap zombies + wait for shutdown signal
    loop {
        match waitpid(-1, WNOHANG) {
            child exited → set_exit_code(), exit
            _ → {}
        }
        match shutdown_rx.try_recv() {
            SHUTDOWN(grace) → {
                kill(-child_pid, SIGTERM);     // signal process group
                wait_with_timeout(grace);
                kill(-child_pid, SIGKILL);     // force kill remaining
                set_exit_code(), exit
            }
            _ → sleep(100ms)
        }
    }
}
```

Build: `cargo build --release --target x86_64-unknown-linux-musl`
Result: ~1-2MB static binary, zero runtime dependencies.

---

## Prior Art: Implementation References

| Project | Pattern | Source |
|---------|---------|-------|
| **K8s kubelet prober** | Three-probe model, state machine, threshold-based transitions | [`pkg/kubelet/prober/`](https://github.com/kubernetes/kubernetes/tree/master/pkg/kubelet/prober) |
| **Docker healthcheck** | Single probe, start_period grace, state machine | [`daemon/health.go`](https://github.com/moby/moby/blob/master/daemon/health.go) |
| **tini** | PID 1 signal forwarding, zombie reaping | [`src/tini.c`](https://github.com/krallin/tini/blob/master/src/tini.c) |
| **dumb-init** | PID 1 signal forwarding, session leader, signal rewriting | [`dumb-init.c`](https://github.com/Yelp/dumb-init/blob/master/dumb-init.c) |
| **Kata agent** | ttrpc over vsock, SignalProcess RPC, DestroySandbox | [`src/agent/src/rpc.rs`](https://github.com/kata-containers/kata-containers/blob/main/src/agent/src/rpc.rs) |
| **libkrun AWS Nitro proxy** | Signal forwarding over vsock (4-byte int) | `src/aws_nitro/src/enclave/proxy/proxies/signal_handler.rs` (in libkrun repo) |
| **Fly.io init-snapshot** | Rust init for Firecracker, vsock API, OCI config | [`superfly/init-snapshot`](https://github.com/superfly/init-snapshot) |
| **systemd sd_notify** | Push-based readiness, watchdog heartbeat | [`sd_notify(3)`](https://man7.org/linux/man-pages/man3/sd_notify.3.html) |

---

## Recommended Probe Model for Neovex

### VM States

```
                    ┌───────────┐
    spawn helper    │           │
    ───────────────►│  Spawning │
                    │           │
                    └─────┬─────┘
                          │ READY on stdout
                          ▼
                    ┌───────────┐
                    │           │   startup probe fails
                    │  Starting │──────────────────────┐
                    │           │                      │
                    └─────┬─────┘                      │
                          │ startup probe passes       │
                          ▼                            ▼
                    ┌───────────┐              ┌───────────┐
                    │           │  readiness   │           │
                    │   Ready   │◄────────────►│ Not Ready │
                    │           │  fails/passes│           │
                    └─────┬─────┘              └─────┬─────┘
                          │ liveness fails           │
                          │ (N times)                │
                          ▼                          │
                    ┌───────────────┐                │
   shutdown req     │               │◄───────────────┘
   ────────────────►│ ShuttingDown  │  liveness fails (N times)
                    │               │
                    └───────┬───────┘
                            │ exited or killed
                            ▼
                    ┌───────────┐
                    │           │
                    │  Exited   │  exit_code, signal
                    │           │
                    └───────────┘
```

### Probe Configuration

```rust
/// Per-service probe configuration
struct ProbeConfig {
    /// How to check the service
    check: HealthCheck,

    /// Startup: don't check until this period elapses after READY signal
    startup_grace: Duration,          // default: 10s

    /// How often to probe once running
    interval: Duration,               // default: 10s

    /// Per-probe timeout
    timeout: Duration,                // default: 5s

    /// Consecutive failures before marking NotReady
    failure_threshold: u32,           // default: 3

    /// Consecutive successes before marking Ready (after failure)
    success_threshold: u32,           // default: 1

    /// Graceful shutdown timeout before SIGKILL
    shutdown_grace: Duration,         // default: 30s
}

enum HealthCheck {
    /// TCP connection test (is the port open?)
    Tcp { port: u16 },

    /// HTTP GET, expect 2xx status
    Http { port: u16, path: String },
}

enum RestartPolicy {
    /// Never restart (let it stay dead)
    Never,
    /// Restart on non-zero exit or crash
    OnFailure {
        max_restarts: u32,            // default: 5
        backoff: BackoffConfig,
    },
    /// Always restart, even on clean exit
    Always {
        max_restarts: u32,
        backoff: BackoffConfig,
    },
}

struct BackoffConfig {
    initial: Duration,                // default: 1s
    max: Duration,                    // default: 60s
    multiplier: f64,                  // default: 2.0
    reset_after: Duration,            // default: 300s (reset backoff after stable)
}
```

### Graceful Shutdown Sequence

```
handle.shutdown(grace_period: 30s)
    │
    ├── 1. Connect to guest vsock port 10000
    ├── 2. Send SHUTDOWN message with grace_period
    ├── 3. Wait for child.wait() with timeout = grace_period + 5s
    │       │
    │       ├── Child exits normally → read exit code → Exited(code)
    │       │
    │       └── Timeout → SIGKILL helper process → Exited(killed)
    │
    └── 4. Clean up: remove temp rootfs if ephemeral
```

Inside the guest (neovex-init):
```
Receive SHUTDOWN on vsock port 10000
    │
    ├── SIGTERM → workload process group
    ├── Wait grace_period
    ├── If still alive: SIGKILL → workload process group
    ├── Collect exit code
    ├── ioctl(KRUN_EXIT_CODE_IOCTL, code)
    └── exit(0) → kernel panic → VM exit → _exit(code) in VMM
```
