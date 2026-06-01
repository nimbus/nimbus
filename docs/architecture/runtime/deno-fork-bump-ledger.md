# Deno-Family Runtime Fork Bump Ledger

Status: current operating ledger

This ledger is the human-readable companion to
[`scripts/verify-deno-fork-provenance.sh`](../../../scripts/verify-deno-fork-provenance.sh).
The script proves the Cargo source closure; this file records why each carried
fork patch exists, whether it should move upstream, and what proof is required
before Nimbus releases it.

Operating workflow:
[`docs/operating/deno-fork-workflow.md`](../../operating/deno-fork-workflow.md).

## Current Runtime Pins

| Fork | Upstream base | Nimbus tag | Commit SHA resolved by Cargo | Cargo source proof |
| --- | --- | --- | --- | --- |
| `nimbus/deno` | `v2.8.1` | `v2.8.1-nimbus.1` | `18f76a9a19ab74d49d9a40037733cc4aec983d26` | `deno_core`, `deno_node`, `node_resolver`, `serde_v8`, and related patch-sensitive crates resolve to this tag/SHA. |
| `nimbus/rusty_v8` | `v149.2.0` | `v149.2.0-nimbus.1` | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` | `v8` resolves to this tag/SHA. |

## Disposition Categories

- **Upstream Deno-family**: general Deno, Node compatibility, or `rusty_v8`
  behavior that should be proposed upstream or dropped when upstream ships it.
- **Nimbus-only host integration**: embedding, release, naming, or product
  host-boundary code that is intentionally local to Nimbus.
- **Temporary carry**: required for the current release, but must have a
  concrete split/removal trigger.

## `nimbus/deno` Delta

| Commit | Subject | Disposition | Removal or upstream trigger | Changelog / proof mapping |
| --- | --- | --- | --- | --- |
| `ab9d720179273bb23c11523247683c814f09f87c` | `build: pin rusty_v8 to nimbus v149.2` | Nimbus-only host integration | `n/a` | Release wiring that binds the Deno 2.8.1 fork to the matching Nimbus `rusty_v8` locker build. |
| `661d101dc0916c464a4247db8ac241789099c1d4` | `runtime: restore nimbus locker lifecycle seam` | Nimbus-only host integration | `n/a` | Restores the embedder lifecycle seam Nimbus needs around the forked locker runtime. |
| `eb2602d8dffdcc5cbf72a382b411664dad95718c` | `runtime: harden embedded node vm and zlib` | Temporary carry | Split into upstreamable Node semantics versus Nimbus embedder glue before the next Deno base bump, or mark the upstreamable subset with an upstream issue/PR. | Node compatibility hardening proven by generated Node LTS evidence and NLRT runtime tests. |
| `6a1c70fd3ee9039c13af47a8595c2151e70da295` | `runtime: return sqlite config errors` | Upstream Deno-family | Drop when upstream Deno ships equivalent `node:sqlite` config-error behavior, or keep linked to an upstream issue/PR. | Node compatibility fix for `node:sqlite` error propagation. |
| `f467ef0e53f5ce34d39e7a2cdcd99d2456556af2` | `runtime: update DNS and TLS security dependencies` | Upstream Deno-family | Drop when upstream Deno contains equivalent dependency updates, or keep linked to the upstream dependency bump. | Security/dependency hygiene for Deno-owned DNS/TLS runtime crates. |
| `156b994727213a96c8d6581e0890b934a46a8d09` | `node: preserve queued MessagePort data without listeners` | Temporary carry | Drop when upstream ships equivalent listener-late MessagePort delivery. | Node worker/message-port compatibility evidence in loader-context tests. |
| `cbf2e188dc0ea220ac6697fdbbedbe31dba00805` | `Fix Node constants binding shape` | Temporary carry | Drop when upstream exposes equivalent Node-shaped constants bindings. | Constants binding and public `fs.constants` compatibility evidence. |
| `418a4fd3faa645fd4b56b0e647779dd5c5112f7d` | `Fix Node dgram compatibility` | Temporary carry | Drop when upstream carries equivalent datagram behavior. | Node networking compatibility evidence. |
| `255b8b16083690180000f77ad045b2aac2e6192b` | `Improve Node compatibility surfaces` | Temporary carry | Split broad upstreamable behavior from Nimbus embedding glue before the next Deno base bump. | Node API compatibility evidence across the generated dashboards. |
| `b892e212b126bb009037a1105b0af55888078a84` | `Fix Node addAbortListener stop propagation semantics` | Upstream Deno-family | Drop when upstream Deno ships equivalent `events.addAbortListener` stop-propagation-resistant observable listener behavior. | NDS3 core/networking abort-controller fixture evidence. |
| `0276a97c85b2d1b44d50dc7caaea51db9388c792` | `Align legacy Node URL parsing` | Upstream Deno-family | Drop when upstream Deno ships equivalent legacy `url.parse()` and `pathToFileURL(..., { windows: true })` behavior. | NDS3 URL fixture evidence. |
| `ef2c3c9eab638e30ffb73596d372e40ac8183e2b` | `Align ArrayBuffer inspect byteLength label` | Upstream Deno-family | Drop when upstream Deno ships equivalent non-enumerable `[byteLength]` inspect output. | NDS3 util/inspect evidence. |
| `1d71a8a7e640b8ad8e172020681f87742dc752b6` | `Improve Node fs and stream compatibility` | Upstream Deno-family | Drop when upstream Deno ships equivalent stream duplex lifecycle and local filesystem compatibility semantics. | NDS3 streams/local-I/O broad evidence. |
| `cf3ad68feb6a80a25aca23d8b9544e495cbf920d` | `Improve Node networking compatibility` | Upstream Deno-family | Drop when upstream Deno ships equivalent HTTP/TLS/HTTP2/PFX/keylog/SNI/globalAgent behavior. | NDS3 networking broad evidence. |
| `e65ddf9dc4a74b0adca7ef1d423dae47afa7caf7` | `Improve Node loader crypto and V8 compatibility` | Temporary carry | Keep only patches still proven as upstream-adjacent, Nimbus-embedding-specific, or still-needed Node gaps; split or upstream the rest before the next base bump. | NDS3 loader/context checkpoint: CommonJS global paths, crypto random/cipher behavior, functional V8 helper subset; `test-v8-serdes.js` remains a wire-format boundary. |
| `18f76a9a19ab74d49d9a40037733cc4aec983d26` | `node: close focused DUA4 compatibility gaps` | Temporary carry | Upstream equivalent loader, V8, crypto, or worker behavior should retire the local hunks. DUA6 classified the broad fixture result after Nimbus consumed this tag: foundation groups returned to their pre-DUA counts after local `process.loadEnvFile` and `fs.watch` embedding fixes; loader/context retains the known async_hooks and V8 wire-format watchpoints. | DUA4 focused proof for CommonJS global paths, `node:v8`, crypto random/cipher behavior, and worker/runtime cleanup; DUA6 broad rebaseline proof. |

## `nimbus/rusty_v8` Delta

| Commit | Subject | Disposition | Removal or upstream trigger | Changelog / proof mapping |
| --- | --- | --- | --- | --- |
| `bf8dbbb4211152ce71d8f6777bb5d4980f178f41` | `Add v8::Locker and v8::UnenteredIsolate` | Temporary carry | Upstream or replace with an upstream-supported locker API before retiring the Nimbus V8 fork. | Adds the embedder locker API required by Nimbus runtime isolation. |
| `d453def8df95fed626bf604c95341ab30d9b7ab8` | `fix(locker): add panic safety and improve documentation` | Temporary carry | Same trigger as the locker API carry. | Hardens the carried locker API. |
| `ac89ad25bc4fcb9249ffeaf1f7a2ec2e28d52dde` | `feat(locker): add compile-time safety tests and unsafe documentation` | Temporary carry | Same trigger as the locker API carry. | Proves the carried locker API safety contract. |
| `0b42d99e09f679862451601b1de086abbae7e68d` | `fix(locker): correct Enter/Exit ordering in Locker` | Temporary carry | Same trigger as the locker API carry. | Correctness fix for the carried locker API. |
| `bf9023a36240b44db27c7069fcb6fedc66ea4c02` | `fix(locker): initialize HandleScope annex in Locker scope` | Temporary carry | Same trigger as the locker API carry. | Correctness fix for Locker-scope handle lifecycle. |
| `5bfcbd6523403e5aab8568ee18f6cf3fd79d032b` | `fix(locker): reset weak handles during isolate teardown` | Temporary carry | Same trigger as the locker API carry. | Weak-handle teardown safety for the carried locker runtime. |
| `880c55035bd6bc086333f1dd52e0df067e841395` | `fix(locker): clear active weak handles before isolate teardown` | Temporary carry | Same trigger as the locker API carry. | Weak-handle teardown safety for the carried locker runtime. |
| `a9260c5087e86793c63e92fb768e323c545f8f6e` | `test(locker): harden weak teardown release` | Temporary carry | Same trigger as the locker API carry. | Regression proof for weak-handle teardown safety. |
| `dae4122f2e4c99e5e57fc80de86b0080a39c5a89` | `test: bless compile_fail stderr for Rust 1.91.0` | Nimbus-only host integration | `n/a` | Toolchain-compatibility blessing for the Nimbus fork build. |
| `b99d4cba386b09958be67492c0cf02603565fd31` | `rename: agentstation -> nimbus` | Nimbus-only host integration | `n/a` | Repository/product naming hygiene for the Nimbus fork. |
| `339f5ade950b41720f733e0a7aa65e238deba417` | `style: apply rustfmt to nimbus v149 port` | Nimbus-only host integration | `n/a` | Mechanical formatting after the Nimbus port. |
| `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` | `build: restore nimbus release contract` | Nimbus-only host integration | `n/a` | Release wiring for the published Nimbus `rusty_v8` tag. |

## DUA6 Rebaseline Evidence

The `v2.8.1-nimbus.1` / `v149.2.0-nimbus.1` Cargo pin was rebaselined in
`docs/plans/proof/deno-rusty-v8-upstream-alignment/dua6-node-compat-rebaseline.md`.
The rebaseline found two Nimbus embedder regressions caused by consuming the
upstream-aligned stack:

- `process.loadEnvFile()` now falls back to Nimbus host-policy file reads after
  Deno's own read permission denies an otherwise granted embedded-runtime
  fixture path.
- `fs.watch()` now restores Node's synchronous missing-entry throw when
  `throwIfNoEntry !== false`, while retaining the Deno watcher behavior for
  existing paths and explicit non-throwing watches.

After the focused fixes, the promoted Node24 foundation groups returned to the
pre-DUA counts: `core-semantics` `122 passed, 1 skipped, 0 failed`;
`process-and-timing` `48 passed, 0 skipped, 0 failed`;
`streams-and-local-io` `308 passed, 0 skipped, 0 failed`; and `networking`
`268 passed, 0 skipped, 0 failed`. Loader/context remains
`173 passed, 0 skipped, 4 failed` with three async_hooks promise-count
watchpoints and the V8 serialization wire-format boundary.

## Release Proof Checklist

Before a Nimbus release can cite these runtime pins, the release proof must
record:

- `bash scripts/verify-deno-fork-provenance.sh` output showing the expected
  `nimbus/deno` and `nimbus/rusty_v8` tag/SHA sources.
- `bash scripts/verify-deno-fork-upstream-policy.sh` output showing this
  operating ledger still contains the current pin and disposition contract.
- The fork-side tests used for any new carried runtime behavior.
- The Nimbus focused tests and generated Node evidence changed by the bump.
- Confirmation that Nimbus is repinned to published tags, not local paths.
