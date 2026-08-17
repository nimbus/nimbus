# NNC0.2 Host-Port Allocation Race Baselines

Status: `expected-red predicates reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `aa57ad91608b661beb7f55894bc7ddbd2276c31d`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, macOS loopback sockets, Rust test profile

## Result

Two explicitly ignored expected-red parent tests now preserve the NNC0.2
failure predicates without making an unsafe result a green CI invariant.
Their child-role entrypoints are also ignored for ordinary discovery and are
invoked only by the exact parent test through real subprocesses.

The tests change no production behavior. They establish the executable
baseline that NNC3 must turn green when the host-global `PortLease` authority
replaces scan/probe-and-drop allocation.

## Sandbox and PEP collision

`two_real_allocator_processes_expose_sandbox_pep_port_collision` starts
distinct `sandbox` and `pep` OS processes over one state root. Both acknowledge
a semantic `ready` barrier before the parent sends `release`. The sandbox child
then calls the real
`PortManager::allocate_missing_bindings_for_tenant`; the PEP child calls the
real `PortManager::allocate_internal_host_port`.

The available range is `41337..=41338`. A safe shared authority could give the
two acquisitions distinct leases. Instead, both scan the same empty manifest
state and select `41337`. Each child persists and syncs its result before
acknowledging `selected:<port>`. The expected-red parent fails only at:

```text
assertion `left != right` failed:
sandbox and PEP allocations must hold distinct host-port leases
  left: 41337
 right: 41337
```

The provider-local coordinator has bounded `recv_timeout` waits, captured
stdout/stderr/status diagnostics, flushed pipe messages, and kill/wait/join
cleanup. It contains no polling sleep. It remains beside the private sandbox
allocator because adding `nimbus-sandbox -> nimbus-testing` even as a dev edge
would create the forbidden cycle
`nimbus-sandbox -> nimbus-testing -> nimbus-tenant -> nimbus-sandbox`.
No Cargo manifest or dependency graph changed.

## Machine probe/drop and external binder

`external_binder_after_probe_blocks_provider_while_machine_state_claims_port`
calls the real `allocate_machine_ssh_port`. That function probes a loopback
port, drops the probe listener, and durably records the selected port. The
parent then starts a separate external-owner process. The child binds that
exact port and flushes an acknowledgement while retaining the listener.

Only after the acknowledgement does the parent perform the
provider-equivalent bind. The kernel returns exactly
`io::ErrorKind::AddrInUse`. The expected-red safety assertion then reloads the
machine allocation file and fails because the recorded claim still names the
externally owned port:

```text
assertion `left != right` failed:
machine state must not retain a port claim after an external owner wins the bind
  left: Some(10001)
 right: Some(10001)
```

The concrete port is intentionally allocator-selected and may vary with host
use; identity comes from the machine record, not the numeric address. The
external child wait is semantic and bounded. Its guard closes stdin, kills and
reaps the process, and joins stdout/stderr readers on every return or panic.

## Commands and results

The two fail-before commands exited `101` for their exact named safety
assertions:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::port_manager::tests::two_real_allocator_processes_expose_sandbox_pep_port_collision \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; failure is 41337 == 41337 at the distinct-lease assertion.

timeout 180 cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state::external_binder_after_probe_blocks_provider_while_machine_state_claims_port \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; provider bind was AddrInUse and the final failure is
# Some(10001) == Some(10001) at the durable-claim safety assertion.
```

The ordinary focused suites and static gates remained green:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::port_manager::tests::
# 7 passed; 0 failed; 2 ignored expected-red/child entrypoints.

timeout 180 cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state::
# 4 passed; 0 failed; 2 ignored expected-red/child entrypoints.

timeout 240 cargo clippy -p nimbus-sandbox -p nimbus-cli \
  --all-targets -- -D warnings
# Exit 0; no warning from either changed crate.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
# All exit 0.
```

The first `nimbus-cli` compile identified the documented generated-asset
prerequisites. `npm ci`, `npm run build -w nimbus-ui`, and
`npm run build:embedded-packages` completed in the owner worktree; the latter
two emitted their existing route/chunk and Node-engine warnings. Generated
outputs were ignored or byte-identical, and `git status` retained only the
owned NNC0.2 test, plan, and proof paths.

No Netavark, gvproxy, KVM, cloud provider, cross-target, or
sovereignty-denial lane applies. The proof uses real child processes and a real
kernel loopback bind. No random seed is used.

## Independent closeout review

The frozen test-only diff was reviewed with:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local \
  --engine claude \
  --model claude-opus-4-8 \
  --thinking max
```

The first pass reported one actionable P2: the sandbox parent had encoded the
collision as a passing ordinary test, so the correct NNC3 fix would have looked
like a regression. The finding was accepted. Both parent tests were converted
to ignored expected-red tests that assert the safe invariant, and their exact
nonzero failure predicates plus the ordinary green suites and Clippy were
rerun.

The required second review pass exited `0` with:

```text
autoreview clean: no accepted/actionable findings reported
```

No finding was rejected. The reviewer traced both process protocols,
timeouts, channel capacities, exact child filters, cleanup paths, ignored-test
polarity, and crate dependency direction.
