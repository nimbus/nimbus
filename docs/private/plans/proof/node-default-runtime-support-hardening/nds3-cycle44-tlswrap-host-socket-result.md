# NDS3 cycle 44 - TLSWRAP host socket reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/async-hooks/test-tlswrap.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `host_owned_network_socket_surface`.

Gate movement:

- node22: 58 -> 57 gaps, 97.60% pass rate
- node24: 66 -> 65 gaps, 97.30% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical. The fixture:

- imports `tls`, `../common/fixtures`, `./init-hooks`, and `./hook-checks`;
- creates a real TLS server with `tls.createServer({...}).listen(0)`;
- reads host TLS certificate/key fixture files;
- connects a TLS client to `server.address().port` with `tls.connect(...)`;
- asserts `hooks.activitiesOfTypes('TLSWRAP')` and exact `TLSWRAP`
  `init`/`before`/`after` lifecycle counts for the server and client sockets.

That is the same structural class as the existing host-owned async-hooks socket
bucket in `scripts/runtime/node/default_support_posture.py`: real host
TCP/UDP/TLS sockets plus libuv async-resource handle graph assertions. The
default multi-tenant V8 isolate must not own ambient host TLS sockets or host
libuv handle topology, so the fixture is not required Application API support.

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
node22 57 97.6
node24 65 97.3
```
