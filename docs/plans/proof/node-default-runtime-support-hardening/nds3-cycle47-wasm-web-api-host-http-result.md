# NDS3 cycle 47 - wasm web API host HTTP reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/es-module/test-wasm-web-api.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `host_owned_network_socket_surface`.

Gate movement:

- node22: 55 -> 54 gaps, 97.72% pass rate
- node24: 63 -> 62 gaps, 97.42% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies both route their streaming WebAssembly
success and rejection cases through a loopback HTTP helper. The fixture:

- imports `http.createServer`, `events`, `fetch`, and fixture wasm bytes;
- defines `testRequest(handler)`, which creates a real
  `createServer((_, res) => handler(res)).unref().listen(0)`;
- waits for the server's `listening` event;
- reads `server.address().port`;
- fetches `http://127.0.0.1:${port}/foo.wasm` from that live host listener;
- uses that response to drive multiple `WebAssembly.compileStreaming()` and
  `WebAssembly.instantiateStreaming()` assertions.

The WebAssembly API assertions are inseparable from a host-owned loopback HTTP
server/client topology for many official subcases. The default multi-tenant V8
isolate must not own ambient host network listeners or clients, so this fixture
is not required Application API support in the default isolate lane.

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
node22 54 97.72
node24 62 97.42
```
