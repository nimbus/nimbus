# BPD7 — Offline + integrity end-to-end

Proof that the supported Convex scaffold flow runs **with the registry
unreachable**, that provisioned bytes are checksum-verified against the binary's
embedded manifest, and that tampering is detected. Covers completion-gate
conditions 6, 11, 17, 19, 21, 23.

Network was disabled by pointing npm at a dead registry
(`NPM_CONFIG_REGISTRY=http://127.0.0.1:1`) and passing `npm install --offline
--no-audit --no-fund --fetch-retries=0`. `--offline` makes npm fail rather than
reach the network; the dead registry is belt-and-suspenders. Binary:
`target/debug/nimbus`.

## 1. No-network init → provision → install → codegen → serve → query

```
$ nimbus init convex "$APP"          # scaffold (sole dep: convex: file:./.nimbus/packages/convex)
$ nimbus packages provision all --app-dir "$APP"
Provisioned 8 package(s) into …/.nimbus/packages: convex, nimbus, firebase, mongodb, dynamodb, @bufbuild/protobuf, @connectrpc/connect, @connectrpc/connect-web
$ (cd "$APP" && NPM_CONFIG_REGISTRY=http://127.0.0.1:1 npm install --offline --no-audit --no-fund --fetch-retries=0)
added 2 packages in 252ms                       # exit 0; node_modules/{convex,nimbus} → ../.nimbus/packages/*
$ (cd "$APP" && NPM_CONFIG_REGISTRY=http://127.0.0.1:1 nimbus codegen --app "$APP")   # in-binary, exit 0
# generated: convex/_generated/{api.ts,server.ts,dataModel.d.ts,scheduled_functions.ts}
#            .nimbus/convex/{functions.json,schema.json,http_routes.json,auth.config.json,bundle.mjs,bundle.sha256,…}
# no node_modules/@nimbus/codegen present (in-binary codegen sources the embedded tooling closure)
$ shasum -a 256 .nimbus/convex/bundle.mjs == cat .nimbus/convex/bundle.sha256   → MATCH ✓
```

Serve + query (registry unreachable for the whole run): `nimbus dev --no-open
--port 39217` started; codegen preflight completed **before registry load**; tenant
`demo` auto-created; `GET /health → 200 {"ok":true}`.

```
POST /convex/demo/query    {"name":"messages:list","args":{}}                       → [] (200)
POST /convex/demo/mutation {"name":"messages:send","args":{"author":"bpd7","body":"offline-proof"}}
                                                                                    → "messages:01KT…" (200)
POST /convex/demo/query    {"name":"messages:list","args":{}}
                                    → [{"_id":"messages:01KT…","author":"bpd7","body":"offline-proof",…}] (200)
```

The runtime bundle is SHA-256-verified before every invocation (architecture
invariant); the `bundle.sha256` match above is the codegen-side record of the
same bytes the runtime loads.

## 2. Provisioned-byte integrity + tamper detection (condition 21)

`nimbus packages verify` re-hashes every file under `.nimbus/packages/` against
the binary's embedded manifest (`provision::verify_provisioned` →
`embedded_packages::sha256_hex`):

```
$ nimbus packages verify --app-dir "$APP"
Verified 717 provisioned file(s) … against embedded checksums          # exit 0
$ printf 'X' >> "$APP/.nimbus/packages/mongodb/uri.js"                  # tamper one byte
$ nimbus packages verify --app-dir "$APP"
Error: checksum mismatch for provisioned mongodb/uri.js: expected c7dc…, got 45d0…   # exit 1
$ # restore → verify passes again (exit 0)
```

Unit coverage: `provision::tests::provisioned_bytes_verify_and_tamper_is_detected`
and `verify_provisioned_skips_unprovisioned_packages` (a `provision firebase` app
verifies its 678-file subset and is not failed by the packages it never
provisioned).

## 3. Adapter packages offline into an existing app (condition 19)

Each adapter provisioned into a bare app, then offline-installed + verified:

```
firebase   → provision 4 (firebase + @connectrpc/* + @bufbuild/protobuf); offline install exit 0;
             require.resolve("@nimbus/firebase") → yes; verify 678 files ✓
mongodb    → offline install exit 0; verify 5 files ✓   (official `mongodb` driver developer-supplied)
dynamodb   → offline install exit 0; verify 5 files ✓   (`@aws-sdk/client-dynamodb` optional dev-supplied peer)
```

Closure regression guard: `@bufbuild/protobuf` shipped a `devDependencies`
(`upstream-protobuf`) that npm installs for `file:` links and would fetch
offline; staging now strips `devDependencies`/`scripts` from third-party
manifests and `scripts/check-package-closure.mjs` fails if any survive.

## 4. Clone-then-install ordering + lockfile policy (condition 25)

The scaffold gitignores `.nimbus/` and `node_modules/` and commits no
`package-lock.json`, so a fresh **clone-then-install** has no `file:` targets and
**provision must run before `npm install`/`npm ci`**:

```
# fresh clone (.nimbus + node_modules absent):
npm install (before provision) → exit 0 BUT node_modules/convex is a DANGLING symlink;
                                 require.resolve("convex") → MODULE_NOT_FOUND (app broken)
nimbus packages provision all  → then npm install → node_modules/convex/package.json readable ✓;
                                 require.resolve("convex/server") → resolves ✓
```

The supported flow needs no manual step: `nimbus init` provisions right after
scaffolding, and `nimbus dev` provisions (`provision::ensure`) before its install
loop. Proven end-to-end: on a simulated fresh clone (`.nimbus`/`node_modules`
removed), `nimbus dev --once` (registry unreachable) provisioned the convex
closure, installed valid symlinks, and generated `bundle.mjs` with no manual
`provision`. A binary upgrade (stamp drift) re-provisions **and** forces a Node
dependency reinstall (`provision::force_node_reinstall` clears the install
fingerprint + the stale `node_modules` copies) — condition 26,
`provision::tests::ensure_on_drift_forces_node_reinstall`.

## 5. Convex auth.config is in-contract (in-binary, offline)

The whole default Convex authoring surface — schema, server, http, and
`auth.config.{ts,js}` — runs in-binary with the registry unreachable. esbuild
bundling does **not** run in the in-binary V8 tooling runtime (both IPC paths
fail: async-service `child_process` `unref`; `buildSync` worker message-port
deserialization), so rather than bundle auth.config with esbuild it is evaluated
by the compile-time TypeScript AST interpreter (`evaluateModuleDefaultExport`) —
the same path as schema/server extraction. Proof: a default-runner
(`NIMBUS_CODEGEN_RUNNER` unset) `nimbus codegen` on a Convex app whose
`auth.config.ts` uses `process.env`, a hoisted `const`, an OIDC provider, and a
`customJwt` provider, with `NPM_CONFIG_REGISTRY=http://127.0.0.1:1`:

```
exit=0; no "external Node.js runner" routing notice (stayed in-binary)
.nimbus/convex/auth.config.json contains both providers (OIDC domain/applicationID
  + customJwt issuer/jwks/algorithm)
```

Unit coverage: `@nimbus/codegen` selftest 219/219 pass, including the auth-config
suite. The default Convex surface is never routed to external Node.

## 6. Out-of-contract surface: Cloud Functions (proven only via external Node)

Cloud Functions is the **one** authoring surface outside the in-binary/offline
contract: its runtime bundling needs esbuild plugins (dynamic virtual modules),
and its Firebase server SDKs are developer-supplied. A detected CF app runs
codegen on the external Node.js runner as its **supported** path (not a
fallback). `start_codegen_preflight_generates_cloud_functions_artifacts` and the
`@google-cloud/functions-framework` variant both pass via the external-node
runner; `embedded_pilot_rejects_cloud_functions_layout_with_clear_message` proves
an *explicit* in-binary CF request is rejected with a clear message. CF is not
counted as a no-network success. Lifting CF in-binary needs a `nimbus/deno`
`child_process`/`worker_threads` IPC fix (separate follow-up).

## Verification

- `cargo test -p nimbus-bin` → **568 passed, 0 failed** (+2 in the integration
  binary), incl. `provision::tests::*` (9), `codegen::tests::*`, and both CF
  `start_codegen_preflight_*` tests.
- `node scripts/check-package-closure.mjs` → OK (5 Nimbus + 3 third-party roots).
- Offline end-to-end commands above run with `NPM_CONFIG_REGISTRY=http://127.0.0.1:1`
  + `npm install --offline`.
