# NDS3 cycle 48 - process beforeExit host lifecycle reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-process-beforeexit.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `host_owned_network_socket_surface`.

Gate movement:

- node22: 54 -> 53 gaps, 97.76% pass rate
- node24: 62 -> 61 gaps, 97.46% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical. The fixture:

- installs a `process.once('beforeExit', ...)` chain;
- uses `setImmediate()`, timers, and `process.nextTick()` to keep re-entering
  and extending host process before-exit lifecycle handling;
- requires the `tryListen()` phase to call
  `net.createServer().listen(0).on('listening', ...)`;
- closes that live host TCP listener before continuing the beforeExit/timer
  lifecycle assertions.

This is not just timer API parity. It asserts exact host process beforeExit
liveness semantics, and one mandatory phase opens a real host TCP listener from
inside that lifecycle chain. The default multi-tenant V8 isolate must not own
host process exit/liveness or ambient host TCP listeners, so this fixture is not
required Application API support in the default isolate lane.

## Verification

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries
```

Generated counts:

```text
node22 53 97.76
node24 61 97.46
```
