# NDS3 Node26 Cycle 15: QUIC host-network reclassification

## Scope

This checkpoint removes the remaining Node26 Current
`test/parallel/test-quic-*` fixtures from the V8-isolate-required denominator
with source-confirmed structural evidence. The fixtures exercise Node's
experimental native QUIC stack: they are gated by `--experimental-quic`, import
`node:quic`, use `../common/quic.mjs` listen/connect helpers, or check
`hasQuic` before driving the native surface.

No V8 or rusty_v8 changes, fixture edits, checker edits, Deno fork edits, local
Deno pins, or generated false-green JSON hand edits were made in this cycle.
Nimbus remained pinned to the published immutable Deno tag
`v2.8.3-nimbus.49`.

Before this wave, Node26 `v8_isolate_required` posture was `427` gaps /
`80.58%`.

## Source Census

Command:

```bash
node -e 'const fs=require("fs");const posture=JSON.parse(fs.readFileSync("docs/private/architecture/runtime/node-default-support-posture.json","utf8"));const paths=posture.lanes.node26.entries.filter(e=>e.support_denominator==="v8_isolate_required"&&e.owner==="node-compat/unpromoted-surface"&&e.test_path.startsWith("test/parallel/test-quic-")).map(e=>e.test_path).sort();const misses=[];for(const f of paths){const text=fs.readFileSync(`crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node26/${f}`,"utf8").slice(0,3000);if(!/(--experimental-quic|node:quic|common\/quic|hasQuic)/.test(text))misses.push(f);}console.log(JSON.stringify({count:paths.length, misses},null,2));'
```

Result:

```json
{
  "count": 94,
  "misses": []
}
```

Representative source facts:

- `test/parallel/test-quic-module-exports.mjs` carries
  `--experimental-quic --no-warnings`, checks `hasQuic`, imports `node:quic`,
  and imports `listen` / `connect` from `../common/quic.mjs`.
- `test/common/quic.mjs` imports `node:quic`, provisions TLS credentials from
  Node's fixture certificates, and wraps `quic.listen()` / `quic.connect()`.
- TLS fixtures such as `test-quic-tls-verify-client.mjs` create server/client
  keys and certificates, call QUIC listen/connect APIs, and assert native
  handshake outcomes.

That is host-owned UDP+TLS networking, diagnostics, H3, stream lifecycle, and
socket-level behavior. The default multi-tenant V8 isolate must deny ambient
host network access; these fixtures can only be supported by a host-capable
backend such as a sandbox-backed service or microVM.

## Generator Change

The reclassification lives in
`scripts/runtime/node/default_support_posture.py` under
`NDS3_WAVE2_PREFIXES`:

- prefix: `test/parallel/test-quic-`
- denominator: `diagnostic_only_non_isolate`
- reason: `host_owned_network_socket_surface`
- shim: `diagnostic_stub`

The official fixtures remain unchanged. The checked-in classification catalogs
remain source evidence; the denominator move is computed by the posture
generator.

## Generated Evidence

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Results:

- `scripts/runtime/node/watchpoints.py validate`:
  `validated node-compat watchpoint catalog: 147 entries`
- `scripts/runtime/node/required_surface_blockers.py`:
  `node22 required gaps: 0`, `node24 required gaps: 0`
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  Node26 `v8_isolate_required` is `333` gaps / `84.18%`
- Node26 remaining `test/parallel/test-quic-*` entries in
  `v8_isolate_required`: `0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `333` gaps, `84.18%`

The Node26 count moved from `427` gaps / `80.58%` to `333` gaps /
`84.18%`, burning 94 required-surface gaps in this wave. Node26 official
manifested green count stayed `1772 / 5578`; this wave does not promote new
green fixtures, it narrows the required denominator honestly.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1772` official passes and `333` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Next Node26 Work

Node26 remains at `333` required gaps. The largest remaining buckets after this
cycle are:

- `149` `node-compat/unpromoted-surface` residual required paths
- `34` `node26_current_required_residual`
- `33` `process-and-timing/process-host`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `15` `streams-local-io/fs-host-io`

Continue with a broad coherent implementation or source-confirmed structural
wave, not singleton cleanup, unless a singleton is the last member of its
cluster.
