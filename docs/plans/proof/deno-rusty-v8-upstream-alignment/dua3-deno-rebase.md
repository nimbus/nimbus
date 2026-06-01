# DUA3 Deno Rebase

status: in_progress
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA3 is in progress. The Deno fork branch exists and has
   a replayed candidate stack, but fork verification is not green yet.
2. **Input baseline.** Source and target SHAs, selected `rusty_v8` substrate,
   and DUA1 replay contract are recorded below.
3. **Disposition table.** Every replayed patch is listed with owner,
   disposition, and conflict outcome. DUA4 still owns dirty/fresh
   changed-behavior reevaluation.
4. **Implementation evidence.** The Deno fork branch
   `nimbus/v2.8.1` was created from upstream `v2.8.1` and replayed on top of
   the selected `rusty_v8` substrate.
5. **Focused verification.** Format and focused Cargo checks are recorded
   below, including the current release-artifact blocker.
6. **Broad verification.** DUA3 does not claim broad Node compatibility moves.
   DUA6 owns the post-repin broad rebaseline.
7. **Residual risks.** Missing Mac prebuilt assets for
   `v149.2.0-nimbus.1` block local Mac `cargo check`; DUA5 must validate or
   supersede the release tag before Nimbus repin.

## Row And Status

DUA3 is in progress. The Deno fork now has a clean candidate branch based on
upstream `denoland/deno@v2.8.1`, with the Nimbus patch stack replayed according
to the DUA1 hunk-level map.

| Field | Value |
| --- | --- |
| Deno fork path | `/Users/jack/src/github.com/nimbus/deno` |
| Candidate branch | `nimbus/v2.8.1` |
| Remote branch | `origin/nimbus/v2.8.1` |
| Upstream base | `v2.8.1` / `3e2030bc01776ce6b4ca355b1e78c4cb98c82dca` |
| Candidate head | `e65ddf9dc4a74b0adca7ef1d423dae47afa7caf7` |
| Selected rusty_v8 tag | `v149.2.0-nimbus.1` |
| rusty_v8 tag SHA | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Corrected rusty_v8 branch head | `d247474e613e8050fef0348cf11f5e01bd94cdfd` |

## Input Baseline

DUA3 started after DUA1 classified all local Deno patches and DUA2 created the
`nimbus/rusty_v8` `nimbus/v149.2.0` branch. The replay input was the local
fork stack from `v2.8.0..v2.8.0-nimbus.15`, but stale `v149.0.0-nimbus.1`
Cargo and lockfile hunks were not reused.

The candidate substrate pin is:

```console
rg -n "source = \"git\\+https://github.com/nimbus/rusty_v8|RUSTY_V8_VERSION|v8 = \\{ git" \
  /Users/jack/src/github.com/nimbus/deno/.cargo/config.toml \
  /Users/jack/src/github.com/nimbus/deno/Cargo.toml \
  /Users/jack/src/github.com/nimbus/deno/Cargo.lock
```

Observed:

```text
/Users/jack/src/github.com/nimbus/deno/Cargo.toml:616:v8 = { git = "https://github.com/nimbus/rusty_v8", tag = "v149.2.0-nimbus.1" }
/Users/jack/src/github.com/nimbus/deno/.cargo/config.toml:41:RUSTY_V8_VERSION = "149.2.0-nimbus.1"
/Users/jack/src/github.com/nimbus/deno/Cargo.lock:11408:source = "git+https://github.com/nimbus/rusty_v8?tag=v149.2.0-nimbus.1#ce6663111a3ff8fde06bc04ba19bbbced60dbc8d"
```

## Disposition Table

| Candidate commit | Source commit | Disposition | Owner | Replay outcome |
| --- | --- | --- | --- | --- |
| `ab9d720179` | `7530d3c1a1` | `nimbus-embedding-specific` | `nimbus/deno` + `nimbus/rusty_v8` | Rewritten for `v149.2.0-nimbus.1`; old `v149.0.0-nimbus.1` lockfile hunk was not carried. |
| `661d101dc0` | `363de88e0d` | `nimbus-embedding-specific` | `nimbus/deno` | Cherry-picked cleanly. Upstream `96aeaf2851` lazy ESM retention remains present beside local `ModuleMap::clear_pending_state`. |
| `eb2602d8df` | `c0d5302324` | `nimbus-embedding-specific` / `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly; DUA4 must retest embedded `node:vm` and zlib. |
| `6a1c70fd3e` | `9225357ba8` | `upstream-adjacent` | `nimbus/deno` | Cherry-picked with upstream sqlite aggregate destructor fix preserved. |
| `f467ef0e53` | `37b6333a1f` | `upstream-adjacent` | `nimbus/deno` | Conflict resolved in `ext/fetch/dns.rs` to preserve upstream `3d6c61477c` resolved-IP permission checks while porting the Hickory/rustls dependency update. |
| `156b994727` | `21015e0bb9` | `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly; DUA4/DUA6 must retest MessagePort delivery. |
| `cbf2e188dc` | `70dba91f18` | `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly; DUA4/DUA6 must retest constants fixtures. |
| `418a4fd3fa` | `fc5ead421f` | `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly with upstream `ext/node/lib.rs` changes preserved. |
| `255b8b1608` | `e6cf99d4a3` | `upstream-adjacent` | `nimbus/deno` | Conflict resolved in `libs/node_resolver/resolution.rs` to keep upstream browser-field pre-map behavior and local `test` / `test/reporters` handling. Compile-cache stayed absent. |
| `b892e212b1` | `ae79fb3e4b` | `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly; DUA4/DUA6 must retest `addAbortListener`. |
| `0276a97c85` | `5099d87414` | `still-needed-node-gap` | `nimbus/deno` | Cherry-picked cleanly; DUA4/DUA6 must retest legacy URL parsing. |
| `ef2c3c9eab` | `843e485fb9` | `upstream-adjacent` | `nimbus/deno` | Cherry-picked cleanly with upstream `util.inspect` Proxy-trap fix preserved. |
| `1d71a8a7e6` | `663306f565` | `upstream-adjacent` | `nimbus/deno` | Conflict resolved in `fs.ts` to preserve upstream fs.watch error-event behavior and carry distinct `throwIfNoEntry: false` handling. |
| `cf3ad68feb` | `d1c53e4315` | `upstream-adjacent` | `nimbus/deno` | Conflicts resolved in favor of upstream `op_node_load_pfx`, PFX extraction, and client-cert verifier behavior; distinct networking/http2/keylog hunks remain for DUA4 testing. |
| `e65ddf9dc4` | `1f101bf003` | `upstream-adjacent` | `nimbus/deno` | Cherry-picked cleanly after prior upstream-preservation conflicts. Compile-cache remained absent; JS stream host-object and `node:v8` behavior need DUA4 focused tests. |

## Implementation Evidence

Commands:

```console
git -C /Users/jack/src/github.com/nimbus/deno switch -c nimbus/v2.8.1 v2.8.1
cargo update -p v8 --precise 149.2.0
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 363de88e0d...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick c0d5302324...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 9225357ba8...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 37b6333a1f...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 21015e0bb...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 70dba91f18...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick fc5ead421f...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick e6cf99d4a3...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick ae79fb3e4...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 5099d87414...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 843e485fb9...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 663306f565...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick d1c53e4315...
git -C /Users/jack/src/github.com/nimbus/deno cherry-pick 1f101bf003...
git -C /Users/jack/src/github.com/nimbus/deno push origin nimbus/v2.8.1
```

Observed candidate stack:

```text
ab9d720179 build: pin rusty_v8 to nimbus v149.2
661d101dc0 runtime: restore nimbus locker lifecycle seam
eb2602d8df runtime: harden embedded node vm and zlib
6a1c70fd3e runtime: return sqlite config errors
f467ef0e53 runtime: update DNS and TLS security dependencies
156b994727 node: preserve queued MessagePort data without listeners
cbf2e188dc Fix Node constants binding shape
418a4fd3fa Fix Node dgram compatibility
255b8b1608 Improve Node compatibility surfaces
b892e212b1 Fix Node addAbortListener stop propagation semantics
0276a97c85 Align legacy Node URL parsing
ef2c3c9eab Align ArrayBuffer inspect byteLength label
1d71a8a7e6 Improve Node fs and stream compatibility
cf3ad68feb Improve Node networking compatibility
e65ddf9dc4 Improve Node loader crypto and V8 compatibility
```

Upstream-replaced behavior check:

```console
rg -n "enableCompileCache|flushCompileCache|getCompileCacheDir|compileCache|internal/compile_cache|compile_cache" \
  ext/node/polyfills/01_require.js ext/node/lib.rs ext/node/polyfills
```

Observed: no matches. The upstream-reverted compile-cache API surface is absent.
No upstream-replaced compile-cache patches remain in the Deno fork candidate.

Replay source locations, owner repo, focused verification, and removal triggers:

| Area | Source location | Owner repo | Focused verification | Removal/upstream trigger |
| --- | --- | --- | --- | --- |
| `rusty_v8` substrate pin | `.cargo/config.toml`, `Cargo.toml`, `Cargo.lock` | `nimbus/deno`, `nimbus/rusty_v8` | `cargo check` must consume a published `v149.2.0-nimbus.*` artifact. | Remove or update when Nimbus consumes an upstream `rusty_v8` line with equivalent locker safety and release assets. |
| Locker lifecycle seam | `libs/core/runtime/*`, `libs/core/modules/map.rs`, `libs/core/tasks.rs` | `nimbus/deno` | DUA3 focused `cargo check`; DUA4/Nimbus runtime reuse tests. | Remove if upstream Deno exposes an embedding-safe locker/reuse lifecycle or Nimbus stops embedding this fork. |
| Fetch DNS/TLS dependency bump | `ext/fetch/dns.rs`, `ext/fetch/lib.rs`, `ext/net/ops.rs`, `ext/tls/tls_key.rs` | `nimbus/deno` | DUA3 focused `cargo check`; DUA4 DNS/TLS focused tests; DUA6 broad fixtures. | Drop local hunks when upstream Deno carries the same Hickory/rustls versions plus resolved-IP permission checks. |
| Node compatibility surfaces | `ext/node/**`, `libs/node_resolver/**`, `libs/resolver/lib.rs` | `nimbus/deno` | DUA4 focused tests for loader, `node:v8`, crypto, fs/stream, networking, MessagePort, constants, URL, and abort semantics. | Drop any hunk that upstream Deno implements with the same observable Node behavior or that only creates fake-success compatibility. |
| Compile-cache removal | `ext/node/polyfills/01_require.js`, `ext/node/lib.rs` | upstream `denoland/deno` | Compile-cache search returns no matches. | Reintroduce only with a product-specific permission and behavior proof; current trigger is no reintroduction. |

## Focused Verification

Commands run:

```console
cargo fmt --check
rg -n "enableCompileCache|flushCompileCache|getCompileCacheDir|compileCache|internal/compile_cache|compile_cache" ext/node/polyfills/01_require.js ext/node/lib.rs ext/node/polyfills
cargo check -p deno_core -p deno_node -p deno_node_crypto -p deno_fetch --locked
env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_core -p deno_node -p deno_node_crypto -p deno_fetch --locked
bash scripts/verify-deno-rusty-v8-upstream-alignment.sh
```

Observed:

- `cargo fmt --check` passed.
- Compile-cache search returned no matches.
- Plain `cargo check` failed before checking Nimbus code because the Deno
  macOS `aarch64-apple-darwin` target config injects `-fuse-ld=lld`, and this
  host's Apple clang reports `invalid linker name in argument '-fuse-ld=lld'`.
- The one-off `CARGO_ENCODED_RUSTFLAGS=` retry got past the linker issue and
  failed in the `v8` build script because
  `v149.2.0-nimbus.1` did not yet publish
  `src_binding_simdutf_release_aarch64-apple-darwin.rs`.
- `gh release view v149.2.0-nimbus.1 --repo nimbus/rusty_v8 --json assets`
  showed Linux and Windows assets present, but no `aarch64-apple-darwin`
  assets yet. The matching tag workflow's `release aarch64-apple-darwin` job
  was still `in_progress`.
- `bash scripts/verify-deno-rusty-v8-upstream-alignment.sh` reports
  `11 passed, 12 failed`. The remaining failures are DUA4 through DUA8 plus
  closeout and repin gates that have not run yet.

## Broad Verification

DUA3 does not claim broad Node compatibility movement. DUA6 must rerun focused
and broad Node compatibility evidence after DUA5 repins Nimbus to published
fork tags.

## Evidence Links

- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua1-deno-overlap-audit.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua2-rusty-v8-alignment.md`
- `/Users/jack/src/github.com/nimbus/deno`
- `/Users/jack/src/github.com/nimbus/rusty_v8`
- `https://github.com/nimbus/rusty_v8/actions/runs/26766437763`

## Residual Risks

- DUA3 cannot close until the focused Deno `cargo check` reaches and verifies
  the changed crates. The current blocker is a missing Mac prebuilt asset on
  the immutable `v149.2.0-nimbus.1` release, not a source conflict.
- If the `v149.2.0-nimbus.1` Mac job fails or never publishes the missing
  assets, DUA5 must use a superseding `v149.2.0-nimbus.*` tag from corrected
  branch head `d247474e613e8050fef0348cf11f5e01bd94cdfd`.
- DUA4 still needs focused behavioral reevaluation for CommonJS global path
  resolution, `node:v8` serializer/deserializer and host-object behavior,
  crypto random/cipher behavior, and `internal_binding` additions.
