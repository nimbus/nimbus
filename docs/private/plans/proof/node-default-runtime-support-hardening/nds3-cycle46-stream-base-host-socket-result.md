# NDS3 cycle 46 - stream-base host socket reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-stream-base-typechecking.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `host_owned_network_socket_surface`.

Gate movement:

- node22: 56 -> 55 gaps, 97.68% pass rate
- node24: 64 -> 63 gaps, 97.38% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical. The fixture:

- imports `net` and `assert`;
- creates a real TCP server with `net.createServer().listen(0, ...)`;
- connects a real TCP client to `server.address().port` with `net.connect(...)`;
- asserts the `client.write('broken', 'buffer')` type error while that live
  host socket exists;
- destroys the client socket and closes the server.

Although the asserted error is a stream-base typecheck, the fixture's execution
topology is a live host TCP listener plus host TCP client. That belongs to the
same host-owned socket surface already used for TCPWRAP, async-hooks TCP graph,
and TLS/UDP socket fixtures. The default multi-tenant V8 isolate must not own
ambient host TCP sockets, so this is not required Application API support.

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
node22 55 97.68
node24 63 97.38
```
