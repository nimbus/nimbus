# Bun Fork Refresh Spec

Status: spec (contract for `bun-fork-refresh-plan.md`)
Date: 2026-07-08

This spec defines the contract for refreshing the `nimbus/bun` fork to a new
upstream snapshot and repointing Nimbus at the refreshed seam. The plan file
owns execution order; this file owns what must be true when the refresh is
done. It is written to be reusable: future refreshes change the identity
table and re-run the same procedure.

## Fork Identity

| Field | Before (2026-05-25 snapshot) | After (2026-07-08 snapshot) |
| --- | --- | --- |
| Upstream base | `f161e0311d56` (upstream main, 2026-05-23) | `332f7444f94025776a173a96b0d7c584298ffea1` (upstream main, 2026-07-08) |
| Active branch | `nimbus/bun-main-20260525` | `nimbus/bun-main-20260708` |
| Proof tag | `nimbus-bun-jsc-proof-main-20260525` | `nimbus-bun-jsc-proof-main-20260708` |
| Pinned revision | `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57` | HEAD of rebased delta after proofs pass (recorded at tag time) |
| GitHub default branch | `nimbus/bun-main-20260525` | `nimbus/bun-main-20260708` |
| Upstream package version at base | 1.4.0 (unreleased) | read from new base `package.json` at execution |

Naming rules (inherited from the archived fork-upstream-standardization plan,
FUS):

- The fork stays on the **mainline-proof scheme** — date-stamped
  `nimbus/bun-main-<YYYYMMDD>` branch plus
  `nimbus-bun-jsc-proof-main-<YYYYMMDD>` tag — because the latest official
  upstream release (`bun-v1.3.14`, 2026-05-12) predates our base and does not
  contain the Rust workspace/embedder surfaces the adapter needs.
- Do **not** mint release-looking `bun-v1.4.0-nimbus.N` tags. Those exist only
  as immutable historical evidence from before FUS3. Switch to
  `bun-vX.Y.Z-nimbus.N` on a `nimbus/bun-vX.Y.Z` branch only when an official
  upstream release contains the required embedder surfaces.
- Old branches and tags are immutable evidence: never delete or force-move
  `nimbus/bun-main-20260525`, `nimbus-bun-jsc-proof-main-20260525`, or the
  historical `bun-v1.4.0-nimbus.[1-5]` tags.

## Nimbus Delta Contract

The fork carries exactly one Nimbus delta stack — 23 commits
(`5385b59549..ad0e1d2bbc` on the old branch) — that must be ported intact onto
the new base, preserving order and authorship (cherry-pick, not squash). The
stack groups as:

1. **Embed probe target + proofs** (`src/embed_probe/`, ~15 commits): the
   `nimbus_bun_embed_probe_*` proof suite, generated program bundle, embed
   invocation ABI, stack-check configuration.
2. **Build seams** (`scripts/build/*`, `Cargo.toml`, `src/simdutf_sys/*`):
   simdutf namespace build option (`nimbus_bun_simdutf`), shared embedder
   build mode, dynamic TLS for the shared embedder, executable-only stack-size
   filtering.
3. **Embedder entrypoints** (`src/jsc/ModuleLoader.rs`,
   `src/jsc/bindings/ZigGlobalObject.cpp`, `src/jsc/bindings/bindings.cpp`,
   `src/runtime/api/BunObject.rs`, `src/link_bridge/`): HostBridge embed
   entrypoint and the exported invocation wrappers.

Known conflict surface: upstream has touched every file the delta patches
(heaviest churn: `bindings.cpp`, `BunObject.rs`, `ZigGlobalObject.cpp`,
`scripts/build/rust.ts`, `scripts/build/flags.ts`). Conflict resolution must
preserve the invariants below; when upstream refactored a seam the delta
hooks into, re-express the hook in the new upstream shape rather than
reverting upstream code.

## Adapter Contract Invariants

These are asserted by `crates/nimbus-runtime/src/backends/bun_jsc/` tests,
`scripts/bun-jsc-adapter-contract.sh`, and
`scripts/verify-bun-jsc-linked-adapter.sh`. The refresh must not change any of
them — only the source ref/revision pins move:

- ABI name `nimbus-bun-jsc-embedder`, ABI version `1`, schema version `1`.
- Lifecycle `fresh_discard`; memory enforcement `outer_quota_required`.
- simdutf namespace `nimbus_bun_simdutf`; after the build, `libWTF.a` and
  `libJavaScriptCore.a` define namespaced symbols and **zero** plain
  `simdutf::` symbols; the C wrapper exports only `nimbus_bun_simdutf__*`;
  V8/rusty_v8 artifacts own **zero** symbols in the Nimbus namespace.
- The shared adapter (`libnimbus_bun_jsc_embedder.{dylib,so}`) exports exactly
  the 11 symbols in `BUN_JSC_ADAPTER_REQUIRED_EXPORTS` (9 probe entrypoints +
  `nimbus_bun_embed_invoke_program_wrapper_json` and
  `..._with_host_bridge`).
- Proof/build target name `check-bun-embed-shared` (ninja phony target defined
  in the fork's `scripts/build/bun.ts`).
- dlopen safety: generated build graph contains no
  `-ftls-model=initial-exec|local-exec` and no
  `--allow-multiple-definition`/`muldefs` (BJA4L SIGSEGV lesson).

If upstream churn forces a genuine contract change (renamed export, new
required export, ABI shape change), stop and bump the contract deliberately on
both sides (Rust `contract.rs` + shell `bun-jsc-adapter-contract.sh` + this
spec) — never paper over a drifted export list.

## Nimbus Pin Sites

Every site that must move from the old ref/revision to the new pair, in one
atomic Nimbus PR:

| File | What moves |
| --- | --- |
| `crates/nimbus-runtime/src/backends/bun_jsc/contract.rs` | `source_ref`, `git_revision` |
| `crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs` (tests) | expected `source_ref` assertions |
| `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs` (tests) | expected ref + revision assertions |
| `crates/nimbus-server/src/tests/registry_and_license/runtime_metrics.rs` | expected `source_ref` in diagnostics JSON |
| `scripts/bun-jsc-adapter-contract.sh` | `BUN_JSC_ADAPTER_SOURCE_REF`, `BUN_JSC_ADAPTER_SOURCE_REVISION` |
| `scripts/install.sh` | `BUN_JSC_ADAPTER_SOURCE_REF`, `BUN_JSC_ADAPTER_SOURCE_REVISION` |
| `scripts/verify-install-helper.sh` | fixture ref/revision + component version string |
| `.github/workflows/bun-jsc-adapter.yml` | `bun_source_ref`, `bun_source_revision` defaults; `bun_bootstrap_version` only if a newer official npm Bun exists |
| `scripts/verify-fork-upstream-standardization.sh` | `nimbus/bun` registry row: active branch, proof tag, base marker (`332f7444f9` short form), upstream sync tag if upstream released past 1.3.14 |
| `packages/nimbus-ui/src/test/handlers.ts` | MSW fixture `source_ref` + `source_revision` — note the fixture ref is already drifted (`bun-v1.4.0-nimbus.5`, pre-FUS naming); fix to the new proof ref |
| `packages/nimbus-ui/src/test/msw.spec.ts` | asserted ref string (same drift) |
| `packages/nimbus-ui/src/routes/operator/settings/configuration.spec.tsx` | asserted ref string (same drift) |
| `crates/nimbus-runtime/tests/engine_proofs/bun_jsc.rs` | fallback `bun_repo()` default path — repoint `~/src/github.com/oven-sh/bun` → `~/src/github.com/nimbus/bun` (env override unchanged) |
| `scripts/verify-bun-jsc-in-process-lockdown.sh` | same stale default path repoint |

Completeness check: after the pin update,
`grep -rn "nimbus-bun-jsc-proof-main-20260525\|ad0e1d2bbc\|bun-v1.4.0-nimbus" crates/ scripts/ packages/ .github/`
must return zero hits (archived plans and proof history under
`docs/private/plans/` are exempt — they are immutable evidence; the
`gate-66-final-closeout.md` label of `ad0e1d2` as `bun-v1.4.0-nimbus.5` is
historical drift and stays as written).

## GitHub Repository State

After the refresh, `github.com/nimbus/bun` must show:

- `nimbus/bun-main-20260708` pushed and set as the default branch.
- `nimbus-bun-jsc-proof-main-20260708` pushed, pointing at the proof-verified
  HEAD (tag only after the proof suite passes — the tag is a claim).
- Old branch and tags untouched.
- Upstream automation workflows disabled (`gh workflow disable`) so inherited
  schedule/issue automation does not run on the fork — same posture as the
  deno fork.

## Verification Matrix

| Gate | Where | What it proves |
| --- | --- | --- |
| `check-bun-embed-shared` build + probe run | fork, darwin-arm64 (required) | delta compiles on the new base; 11 exports present; proofs pass |
| `scripts/verify-bun-jsc-linked-adapter.sh` | nimbus repo, darwin-arm64, `NIMBUS_BUN_REPO` at the fork | end-to-end: expected ref/rev, build-graph linker/TLS audit, shared-artifact export audit, simdutf/V8 namespace separation |
| `make verify-bun-jsc-runtime-contract` | nimbus repo | Rust/UI contract side with new pins |
| `scripts/verify-fork-upstream-standardization.sh` | nimbus repo | fork registry row matches live local + remote fork state |
| `make ci` | nimbus repo | required merge gate |
| `bun-jsc-adapter.yml` dispatch (linux-x86_64 + darwin-arm64) | hosted, post-merge | adapter artifacts build from the new tag on both platforms |

Linux note: the strict simdutf symbol audit defaults on only for
`x86_64-unknown-linux-gnu`. The hosted dispatch workflow is the Linux leg;
a minicloud run is an acceptable substitute if hosted dispatch is unavailable.

## Risks And Rollback

- **Upstream churn** (575 commits over our base) is the main risk; the
  embedder entrypoint files are upstream's highest-churn areas. Budget the
  rebase as the dominant cost, not the pin edits.
- **Version drift**: upstream `package.json` may have moved past 1.4.0; the
  date-based naming scheme is insulated from this. Record the observed version
  in the completion evidence.
- **Rollback** is cheap by construction: the old branch/tag/revision remain
  live; the Nimbus pin change is one atomic PR that can be reverted; the
  default branch can be flipped back with one API call.
