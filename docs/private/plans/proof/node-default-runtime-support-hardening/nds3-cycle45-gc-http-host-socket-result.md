# NDS3 cycle 45 - GC HTTP host socket reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-gc-http-client-connaborted.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `host_owned_network_socket_surface`.

Gate movement:

- node22: 57 -> 56 gaps, 97.64% pass rate
- node24: 65 -> 64 gaps, 97.34% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical. The fixture:

- runs with `// Flags: --expose-gc`;
- imports `../common/gc`, whose `onGC()` helper tracks objects through an
  `async_hooks.AsyncResource` GC-tracker destroy event;
- creates a real HTTP server with `http.createServer(serverHandler)` and
  `server.listen(0, ...)`;
- repeatedly opens real localhost clients with `http.get({ hostname:
  'localhost', port: server.address().port }, ...)`;
- forcibly aborts every accepted socket with `res.connection.destroy()`;
- calls `globalThis.gc()` and loops until every ClientRequest has produced the
  expected GC-tracker destroy callback before closing the server.

That is not a portable multi-tenant V8-isolate Application API claim. The core
assertion depends on host-owned HTTP server/client sockets, connection abort
teardown, exposed-GC scheduling, and async-resource destroy topology for native
request objects. It belongs with the existing host-owned socket bucket in
`scripts/runtime/node/default_support_posture.py`, alongside the TLSWRAP and
TCP/UDP/TLS socket graph fixtures.

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
node22 56 97.64
node24 64 97.34
```
