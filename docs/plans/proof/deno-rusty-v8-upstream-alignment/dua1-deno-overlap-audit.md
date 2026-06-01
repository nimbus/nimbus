# DUA1 Deno Overlap Audit

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA1 is done. DUA0 is done and draft PR #11
   is the remote review/CI control surface for this row.
2. **Input baseline.** The Deno fork patch stack, upstream `v2.8.1` target,
   fetched tags, and current dirty-file state are recorded below.
3. **Disposition table.** Every local Deno commit through
   `v2.8.0-nimbus.15` has a disposition below, plus hunk-level replay
   instructions for every upstream-overlapping area.
4. **Implementation evidence.** This row is audit-only so far; no fork patch
   has been replayed or dropped.
5. **Focused verification.** Local fetch, range-diff, patch-id, overlap, and
   targeted diff evidence are recorded below.
6. **Broad verification.** DUA1 does not run broad compatibility groups; DUA6
   owns the post-repin broad rebaseline.
7. **Residual risks.** DUA1 is closed as an audit row. DUA3/DUA4 still own
   source changes and runtime proof before any compatibility claim moves.

## Row And Status

DUA1 is done. The local upstream Deno checkout was stale at row start
and did not have `v2.8.1`; fetching tags made `v2.8.1` available in both the
upstream checkout and the Nimbus fork checkout for local comparison.

This proof intentionally separates two facts:

- No Nimbus fork commit has an exact patch-id match in upstream `v2.8.1`.
- Several upstream `v2.8.1` commits overlap the same logical areas and must
  replace or shrink Nimbus patches during DUA3/DUA4.

## Input Baseline

Local Deno fork:

| Field | Value |
| --- | --- |
| Path | `/Users/jack/src/github.com/nimbus/deno` |
| Branch | `nimbus/v2.8.0` |
| Current tag | `v2.8.0-nimbus.15` |
| Current SHA | `1f101bf0032a223463507f500ddd236afebd9fcc` |
| Upstream base | `v2.8.0` |
| Target upstream tag | `v2.8.1` |
| Dirty files | none observed by `git status --short --branch` |

Fetched upstream target:

```console
git -C /Users/jack/src/github.com/denoland/deno fetch --tags origin
git -C /Users/jack/src/github.com/nimbus/deno fetch --tags local-denoland
```

Observed: `v2.8.1` is now present locally for comparisons.

Version-lock correction:

```console
git -C /Users/jack/src/github.com/denoland/deno show v2.8.1:Cargo.toml
```

Observed upstream `v2.8.1` workspace pins include
`v8 = { version = "149.2.0", default-features = false, features = ["simdutf"] }`.
That makes `rusty_v8` substrate alignment a prerequisite for Deno replay rather
than a later cleanup row. DUA2 now owns the `v149.2.0` rebase or an exact
build/safety/runtime hold decision before DUA3 rebuilds the Deno fork.

Current local patch stack:

```console
git -C /Users/jack/src/github.com/nimbus/deno log --oneline --reverse v2.8.0..v2.8.0-nimbus.15
```

Observed commits:

| Commit | Subject |
| --- | --- |
| `7530d3c1a1` | `build: pin rusty_v8 to nimbus v149` |
| `363de88e0d` | `runtime: restore nimbus locker lifecycle seam` |
| `c0d5302324` | `runtime: harden embedded node vm and zlib` |
| `9225357ba8` | `runtime: return sqlite config errors` |
| `37b6333a1f` | `runtime: update DNS and TLS security dependencies` |
| `21015e0bb9` | `node: preserve queued MessagePort data without listeners` |
| `70dba91f18` | `Fix Node constants binding shape` |
| `fc5ead421f` | `Fix Node dgram compatibility` |
| `e6cf99d4a3` | `Improve Node compatibility surfaces` |
| `ae79fb3e4b` | `Fix Node addAbortListener stop propagation semantics` |
| `5099d87414` | `Align legacy Node URL parsing` |
| `843e485fb9` | `Align ArrayBuffer inspect byteLength label` |
| `663306f565` | `Improve Node fs and stream compatibility` |
| `d1c53e4315` | `Improve Node networking compatibility` |
| `1f101bf003` | `Improve Node loader crypto and V8 compatibility` |

Relevant upstream `v2.8.1` overlap commits:

```console
git -C /Users/jack/src/github.com/denoland/deno log --oneline --reverse v2.8.0..v2.8.1 -- ext/node libs/node_resolver libs/core ext/node_crypto ext/net ext/fetch ext/tls ext/web
```

Observed overlap set:

| Upstream commit | Subject | Overlap area |
| --- | --- | --- |
| `1fab34851` | `fix(ext/node): attach register as static on Module (#34305)` | module API |
| `54179d8b7` | `Revert "fix(ext/node): polyfill module.enableCompileCache and companions" (#34190) (#34348)` | compile-cache removal |
| `728239f30` | `fix(ext/node): drop extra positional args in promisified fs.promises.* (#34347)` | `fs.promises` |
| `96aeaf285` | `fix(core): keep lazy_loaded_esm sources across concurrent loads (#34353)` | lazy ESM loading |
| `4e8c17423` | `fix(ext/node): add missing node:util APIs getSystemErrorMap, transferableAbortSignal, transferableAbortController (#34372)` | `node:util` |
| `41d7773ae` | `fix(node/util): don't invoke Proxy traps in util.inspect (#34373)` | `util.inspect` |
| `8ab97008c` | `fix(ext/node): TLSSocket.authorized=false when client presents no cert (#34381)` | TLS authorization |
| `c2c91051c` | `fix(ext/node): support PKCS#12 MACs other than SHA-1 (#34342)` | PFX/PKCS#12 |
| `7aadfe88f` | `fix(ext/node): accept array forms of cert/key/pfx in createSecureContext (#34379)` | TLS secure context inputs |
| `5ca12ed9c` | `fix(ext/node): extract cert/key from pfx in tls SecureContext (#34383)` | PFX extraction |
| `c10cf515c` | `fix(ext/node): do not throw NotFound for fs.exists (#34244)` | `fs.exists` |
| `bd343a7d5` | `fix(core): allow host objects to round-trip through core.deserialize (#34380)` | host object deserialization |
| `88d31a2ec` | `perf(ext/node): reuse keep-alive timer in node:http server (#34302)` | Node HTTP runtime wakeups |
| `044bed848` | `fix(ext/node): require env permission for process.loadEnvFile (#34350)` | process/env permission |
| `f746b764b` | `fix(ext/node): tolerate non-AsyncWrap handles in _getNewAsyncId (#34413)` | async-wrap tolerance |
| `e805fcd6e` | `fix(ext/node): emit 'error' event for fs.watch open failures (#34398)` | `fs.watch` errors |

## Disposition Table

Initial row-level disposition for each carried Deno fork commit:

| Commit | Subject | Initial disposition | Reason | DUA3/DUA4 requirement |
| --- | --- | --- | --- | --- |
| `7530d3c1a1` | `build: pin rusty_v8 to nimbus v149` | `nimbus-embedding-specific` | Nimbus must pin Deno to the matching `nimbus/rusty_v8` locker fork until DUA2 proves a safe rebase or hold. | Replay only if the post-DUA2 rusty_v8 decision still needs fork pin wiring. |
| `363de88e0d` | `runtime: restore nimbus locker lifecycle seam` | `nimbus-embedding-specific` | The lifecycle seam is tied to Nimbus' locker-based embedding model, not general Deno behavior. | Replay only with DUA2 locker safety proof. |
| `c0d5302324` | `runtime: harden embedded node vm and zlib` | `upstream-adjacent` | Broad mixed patch; upstream 2.8.1 contains nearby core lazy-load and runtime wakeup work but not an exact patch-id match. | Split hunks into upstream-replaced, embedding-specific, or still-needed pieces before replay. |
| `9225357ba8` | `runtime: return sqlite config errors` | `upstream-adjacent` | Upstream 2.8.1 includes Node runtime compatibility work but this exact sqlite error patch is not patch-id replaced. | Compare against upstream sqlite behavior before replay. |
| `37b6333a1f` | `runtime: update DNS and TLS security dependencies` | `upstream-adjacent` | Upstream 2.8.1 updates network/TLS-facing crates and code, but exact dependency hygiene needs lockfile comparison. | Keep only dependency changes still newer or security-relevant after 2.8.1. |
| `21015e0bb9` | `node: preserve queued MessagePort data without listeners` | `still-needed-node-gap` | No exact patch-id match; upstream 2.8.1 has adjacent web/core timer/runtime changes but not this MessagePort listener-late delivery fix in the observed overlap set. | Re-test worker/message-port fixtures on the 2.8.1 candidate. |
| `70dba91f18` | `Fix Node constants binding shape` | `still-needed-node-gap` | No exact patch-id match and no observed `constants` overlap in the upstream 2.8.1 commit list. | Replay only if constants fixtures fail on raw upstream 2.8.1. |
| `fc5ead421f` | `Fix Node dgram compatibility` | `still-needed-node-gap` | No exact patch-id match and no direct `dgram` upstream 2.8.1 overlap was observed. | Re-test dgram fixtures before replay. |
| `e6cf99d4a3` | `Improve Node compatibility surfaces` | `upstream-adjacent` | Broad patch overlaps multiple upstream 2.8.1 Node improvements and must be split. | Classify hunks by owner before replay. |
| `ae79fb3e4b` | `Fix Node addAbortListener stop propagation semantics` | `still-needed-node-gap` | No exact patch-id match and no direct upstream abort-listener fix was observed in `v2.8.1`. | Re-test abort-controller/events fixtures on upstream 2.8.1. |
| `5099d87414` | `Align legacy Node URL parsing` | `still-needed-node-gap` | No exact patch-id match and no direct upstream legacy URL parser overlap was observed. | Re-test URL fixtures before replay. |
| `843e485fb9` | `Align ArrayBuffer inspect byteLength label` | `upstream-adjacent` | Upstream 2.8.1 has `util.inspect` work, but its observed commit targets Proxy traps rather than this exact byteLength label. | Compare `internal/util/inspect.mjs` hunks before replay. |
| `663306f565` | `Improve Node fs and stream compatibility` | `upstream-adjacent` | Upstream 2.8.1 overlaps `fs.promises`, `fs.exists`, and `fs.watch` error behavior. | Drop upstream-replaced FS/watch hunks; carry only distinct behavior with tests. |
| `d1c53e4315` | `Improve Node networking compatibility` | `upstream-adjacent` | Upstream 2.8.1 overlaps TLS PFX extraction, array cert/key/pfx forms, authorization, and HTTP wakeups. | Drop overlapping TLS/PFX hunks if upstream passes the focused fixtures; carry distinct SNI/keylog/http2/globalAgent behavior only if still needed. |
| `1f101bf003` | `Improve Node loader crypto and V8 compatibility` | `upstream-adjacent` | Upstream 2.8.1 removed compile-cache, added host-object deserialization support, and contains JS stream socket work; local `.15` also changes CommonJS paths, crypto, and `node:v8`. | Drop compile-cache API surface, use upstream host-object support where equivalent, and re-test loader/crypto/V8 focused fixtures before replay. |

Compile-cache disposition:

| Surface | Disposition | Reason | Required action |
| --- | --- | --- | --- |
| `module.enableCompileCache`, `module.flushCompileCache`, `module.getCompileCacheDir`, `module.constants.compileCacheStatus`, and `ext/node/polyfills/internal/compile_cache.js` | `upstream-replaced` / `drop-no-longer-needed` | Upstream `v2.8.1` explicitly reverts the polyfill because Deno already has V8 code caching and the polyfill adds unwanted environment-permission reads. | DUA3 must not replay this API surface unless a later product-specific exception names the permission behavior and tests. |

Dirty Deno files:

| Path | Status |
| --- | --- |
| `/Users/jack/src/github.com/nimbus/deno` | clean at DUA1 start |

## Hunk-Level Source Mapping

DUA1 compared the local Nimbus patch stack with upstream `v2.8.1` at commit
and file level. The result is not "replay everything": it is "start from
upstream `v2.8.1`, preserve its fixes, and replay only the local hunks that
remain distinct after focused tests." No local commit is an exact upstream
patch-id match, but many local commits touch files that upstream also changed.

Commit matching evidence:

```console
git -C /Users/jack/src/github.com/nimbus/deno range-diff --creation-factor=80 v2.8.0..v2.8.0-nimbus.15 v2.8.0..v2.8.1
git -C /Users/jack/src/github.com/nimbus/deno cherry -v v2.8.1 v2.8.0-nimbus.15
```

Observed: `range-diff` reports all 15 Nimbus commits only on the local side
and all 45 upstream `v2.8.1` commits only on the upstream side. `git cherry`
reports all 15 Nimbus commits with `+`, so DUA3 cannot mark any whole commit
as exact upstream-replaced.

Upstream-preservation rules for DUA3:

| Local area | Upstream `v2.8.1` owner | DUA3 rule |
| --- | --- | --- |
| Compile cache APIs in `01_require.js` and `internal/compile_cache.js` | `54179d8b7a` | Do not replay. Upstream reverted this surface because Deno already has V8 code caching and the polyfill added unwanted environment-permission reads. |
| DNS/fetch resolved-IP permission checks | `3d6c61477c` | Preserve upstream permission behavior. If carrying the local Hickory/rustls security bump, port it around upstream's permissioned resolver rather than replacing that resolver. |
| Lazy ESM source retention | `96aeaf2851` | Preserve upstream clone/retention semantics. Local `ModuleMap::clear_pending_state` is a separate Nimbus warm-reuse reset hunk. |
| Core host-object deserialization | `bd343a7d5a` | Preserve upstream `core.deserialize` host-object brand handling. Carry local `node:v8` JS stream host-object behavior only if focused `node:v8` fixtures still fail on raw `v2.8.1`. |
| `fs.promises`, `fs.exists`, and `fs.watch` open errors | `728239f30f`, `c10cf515cb`, `e805fcd6e4` | Preserve upstream fixes. Carry only distinct local fs/statfs/open/mkdir/truncate/writeFile/duplexify hunks with focused evidence. |
| TLS/PFX/certificate handling and HTTP keep-alive wakeups | `8ab97008ce`, `c2c91051c0`, `7aadfe88fb`, `5ca12ed9c9`, `88d31a2ec6` | Preserve upstream TLS/PFX and HTTP fixes. Carry only distinct local SNI, keylog, http2, or globalAgent behavior after focused tests prove gaps. |
| `process.loadEnvFile` permission and `node:util` APIs/inspect behavior | `044bed8486`, `4e8c17423c`, `41d7773ae8` | Preserve upstream permission/API/Proxy-trap behavior. Local process/util hunks must be replayed only when they do not shadow those upstream changes. |
| Module API and CommonJS loader behavior | `1fab348518`, `54179d8b7a` | Preserve upstream `Module.register` static export and compile-cache removal. Carry local CommonJS global path, loader, or package behavior only with focused loader evidence. |
| `node:sqlite` panic/error behavior | `12a12f2ed2` | Upstream aggregate panic prevention is adjacent, not the local `set_db_config` error-return hunk. Carry local sqlite config error behavior only if raw upstream still panics or loses the error. |

Per-commit replay map:

| Local commit | Upstream-overlap files | Hunk-level action for DUA3/DUA4 |
| --- | --- | --- |
| `7530d3c1a1` | `Cargo.toml`, `Cargo.lock` | Replace the old `v149.0.0-nimbus.1` pin with the DUA2-selected `v149.2.0-nimbus.*` substrate. Do not carry stale lockfile hunks. |
| `363de88e0d` | `libs/core/modules/map.rs` | Carry Nimbus locker lifecycle and warm-reuse reset hunks only around upstream `96aeaf2851` lazy ESM retention. Both behaviors must coexist. |
| `c0d5302324` | none in upstream `v2.8.1` changed-file intersection | Re-test embedded `node:vm` and zlib behavior on `v2.8.1`; carry as Nimbus embedding/Node compatibility only if focused tests still fail. |
| `9225357ba8` | `ext/node_sqlite/database.rs` | Treat as adjacent to upstream `12a12f2ed2`. Preserve upstream aggregate panic fix; carry local config-error return only with focused sqlite proof. |
| `37b6333a1f` | `Cargo.toml`, `Cargo.lock`, `ext/fetch/dns.rs`, `ext/fetch/lib.rs` | Port only the still-needed Hickory/rustls security bump around upstream `3d6c61477c` permission checks. Never drop resolved-IP deny-list enforcement. |
| `21015e0bb9` | none | Re-test MessagePort queued-data delivery; carry only if upstream `v2.8.1` still drops queued data before listeners attach. |
| `70dba91f18` | none | Re-test Node constants binding shape; carry if constants fixtures still fail on raw upstream. |
| `fc5ead421f` | `ext/node/lib.rs` only | Local dgram/tcp/udp behavior has no direct upstream dgram replacement. Preserve unrelated upstream `ext/node/lib.rs` changes and replay network ops only with focused evidence. |
| `e6cf99d4a3` | `ext/node/lib.rs`, `01_require.js`, `fs.ts`, `process.ts`, `util.ts`, `libs/node_resolver/resolution.rs` | Split before replay. Preserve upstream compile-cache removal, `Module.register`, `process.loadEnvFile` permission, `node:util` additions, `util.inspect` Proxy behavior, and package browser-field map support. Carry only distinct REPL/testing/SEA/tracing/global path/fs/util hunks with tests. |
| `ae79fb3e4b` | none | Re-test `addAbortListener` stop-propagation semantics; carry if upstream still diverges from Node. |
| `5099d87414` | none | Re-test legacy URL parsing; carry only if raw upstream still fails the compatibility fixtures. |
| `843e485fb9` | `ext/web/01_console.js` | Adjacent to upstream `41d7773ae8` inspect work. Preserve upstream no-Proxy-trap behavior; carry ArrayBuffer `byteLength` label hunk if focused inspect fixtures still require it. |
| `663306f565` | `ext/node/ops/fs.rs`, `fs.ts`, `internal/fs/promises.ts` | Preserve upstream `fs.promises`, `fs.exists`, and `fs.watch` fixes. Carry only distinct fs/statfs/open/mkdir/truncate/writeFile/stream duplexify behavior with focused and broad evidence. |
| `d1c53e4315` | `ext/node/ops/tls_wrap.rs`, `_http_server.js`, `_tls_common.ts`, `_tls_wrap.js`, `fs.ts`, `ext/node_crypto/keys.rs`, `ext/node_crypto/lib.rs`, `ext/web/01_console.js` | Preserve upstream TLS/PFX/authorized and HTTP timer fixes. Carry distinct SNI, keylog, http2, globalAgent, crypto-key, and console hunks only if focused tests prove remaining gaps. |
| `1f101bf003` | `ext/node/lib.rs`, `01_require.js` | Drop compile-cache surface. Preserve upstream host-object deserialization. Re-test CommonJS loader, crypto, `node:v8`, JS stream host-object, and V8 serialization behavior before carrying local hunks. |

This map is the DUA3 replay contract: replay starts from upstream `v2.8.1`;
upstream-owned fixes are the baseline; local hunks are guilty until focused
tests prove they are still needed and not duplicate local compatibility code.

## Implementation Evidence

No Deno fork code has been changed by DUA1 yet. Evidence gathered so far:

- Upstream tags were fetched into `/Users/jack/src/github.com/denoland/deno`.
- Tag `v2.8.1` was fetched into `/Users/jack/src/github.com/nimbus/deno` from
  the local upstream checkout.
- `git range-diff --creation-factor=80 v2.8.0..v2.8.0-nimbus.15 v2.8.0..v2.8.1`
  reports no exact commit mapping.
- `git cherry -v v2.8.1 v2.8.0-nimbus.15` reports every local Nimbus commit
  with `+`, proving no exact patch-id match against upstream `v2.8.1`.
- Targeted upstream diffs show compile-cache removal, Module.register export,
  lazy ESM/core changes, host-object deserialization, TLS/PFX work,
  `fs.exists`, `fs.watch`, and `fs.promises` overlap areas.

## Focused Verification

Commands run:

```console
git -C /Users/jack/src/github.com/denoland/deno fetch --tags origin
git -C /Users/jack/src/github.com/nimbus/deno fetch --tags local-denoland
git -C /Users/jack/src/github.com/nimbus/deno range-diff --creation-factor=80 v2.8.0..v2.8.0-nimbus.15 v2.8.0..v2.8.1
git -C /Users/jack/src/github.com/nimbus/deno cherry -v v2.8.1 v2.8.0-nimbus.15
git -C /Users/jack/src/github.com/nimbus/deno diff --name-only v2.8.0..v2.8.0-nimbus.15
git -C /Users/jack/src/github.com/nimbus/deno diff --name-only v2.8.0..v2.8.1
git -C /Users/jack/src/github.com/denoland/deno log --oneline --reverse v2.8.0..v2.8.1 -- ext/node libs/node_resolver libs/core ext/node_crypto ext/net ext/fetch ext/tls ext/web
git -C /Users/jack/src/github.com/denoland/deno diff --stat v2.8.0..v2.8.1 -- ext/node libs/node_resolver libs/core ext/node_crypto ext/net ext/fetch ext/tls ext/web
git -C /Users/jack/src/github.com/denoland/deno grep -n "enableCompileCache\\|module.enableCompileCache\\|JSStream\\|js_stream\\|globalPaths\\|NODE_PATH" v2.8.1 -- ext/node
bash scripts/verify-deno-rusty-v8-upstream-alignment.sh
```

Observed:

- `v2.8.1` fetched successfully.
- `range-diff` shows no local commit is equivalent to an upstream `v2.8.1`
  commit.
- `git cherry` shows all 15 Nimbus fork commits as `+` relative to upstream
  `v2.8.1`, so DUA cannot blindly mark any commit as exact upstream-replaced.
- File-level overlap is concentrated in the tables above; several local
  commits have no direct upstream file overlap and must still be retested
  rather than replayed blindly.
- Upstream `v2.8.1` deletes `ext/node/polyfills/internal/compile_cache.js`.
- Upstream `v2.8.1` contains `internal/js_stream_socket.js` and `js_stream`
  internal binding entries, but the local `.15` `internal_binding/js_stream.ts`
  and V8 serializer host-object behavior still need hunk-level comparison.
- `bash scripts/verify-deno-rusty-v8-upstream-alignment.sh` reports
  `8 passed, 15 failed`. The remaining failures are DUA3 through DUA8 and
  closeout gates, which are expected because implementation rows have not run
  yet.

## Broad Verification

DUA1 is an audit row and does not claim runtime compatibility changes. DUA6
must rerun the broad Node compatibility groups after DUA2-DUA5 publish and
repin the upstream-aligned forks.

## Evidence Links

- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-baseline.md`
- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md`
- `/Users/jack/src/github.com/nimbus/deno`
- `/Users/jack/src/github.com/denoland/deno`

## Residual Risks

- DUA1 is audit-complete, but no Deno fork code has been rebuilt yet. DUA3 must
  prove the replay map against a real upstream `v2.8.1` branch.
- Broad mixed commits `c0d5302324`, `e6cf99d4a3`, `663306f565`,
  `d1c53e4315`, and `1f101bf003` need extra care because a commit-level
  disposition is too coarse for safe replay.
- The DUA PR exists at `https://github.com/nimbus/nimbus/pull/11`; hosted
  `gh` auth failures observed inside the sandbox were a permission/escalation
  issue, not a real credential blocker.
- The corrected `rusty_v8` branch CI run for
  `d247474e613e8050fef0348cf11f5e01bd94cdfd` was still in progress when DUA1
  closed. DUA5 must either consume the immutable release tag after artifact
  validation or create a superseding `v149.2.0-nimbus.*` tag from the corrected
  branch head.
