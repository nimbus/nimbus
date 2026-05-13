# Node LTS Compatibility Summary

Generated machine-owned Node compatibility baseline.

## Metadata

- `generated_at_utc`: `2026-04-29T14:58:57+00:00`
- `node20_url`: `https://nodejs.org/download/release/latest-v20.x/docs/api/all.json`
- `node20_etag`: `"6ad7aab45da329642a1c43356a17c3b3"`
- `node20_last_modified`: `Tue, 24 Mar 2026 03:28:14 GMT`
- `node22_url`: `https://nodejs.org/download/release/latest-v22.x/docs/api/all.json`
- `node22_etag`: `"b205ea07cce12bde5a37c9b12078c400"`
- `node22_last_modified`: `Tue, 24 Mar 2026 04:15:15 GMT`
- `deno_compat_url`: `https://docs.deno.com/runtime/reference/node_apis/`
- `deno_compat_etag`: `W/"1b985-9UvrDk5GwfdlaQ/w1Oe4efXtGhp"`
- `deno_compat_last_modified`: `Wed, 22 Apr 2026 13:34:59 GMT`
- `deno_repo`: `/Users/jack/src/github.com/nimbus/deno`
- `deno_git_branch`: `locker-v2.7.14`
- `deno_git_commit`: `a2cc5bfdc77713c9028709f386dbd671dd3f1150`
- `generator_path`: `scripts/runtime/node/generate_matrix.py`

## Counts

- Node 20 symbol rows: `1502`
- Node 22 symbol rows: `1631`
- Node 20 → Node 22 delta rows: `1638`
- Deno module inventory rows: `47`
- Joined compatibility matrix rows: `3276`

## Initial Findings

- Node 22-only symbol rows: `136`
- Deno docs modules with partial/stub caveats: `27`
- Modules still starting at `NeedsVerification`: `19`

## Modules With Published Deno Caveats

- `node:async_hooks`: `partial`
- `node:cluster`: `stub-only`
- `node:crypto`: `partial`
- `node:dgram`: `partial`
- `node:dns`: `partial`
- `node:domain`: `stub-only`
- `node:fs`: `partial`
- `node:fs/promises`: `partial`
- `node:http`: `partial`
- `node:http2`: `partial`
- `node:https`: `partial`
- `node:inspector`: `partial`
- `node:module`: `partial`
- `node:net`: `partial`
- `node:perf_hooks`: `partial`
- `node:process`: `partial`
- `node:repl`: `partial`
- `node:sea`: `partial`
- `node:sqlite`: `partial`
- `node:tls`: `partial`
- `node:trace_events`: `stub-only`
- `node:util`: `partial`
- `node:v8`: `partial`
- `node:vm`: `partial`
- `node:wasi`: `stub-only`
- `node:worker_threads`: `partial`
- `node:zlib`: `partial`

## Per-Module Baseline Snapshot

| Module | Node 20 symbols | Node 22 symbols | Deno docs status | First-baseline support state |
| --- | ---: | ---: | --- | --- |
| `node:assert` | `25` | `27` | `supported` | `NeedsVerification` |
| `node:async_hooks` | `24` | `24` | `partial` | `Partial` |
| `node:buffer` | `77` | `77` | `supported` | `NeedsVerification` |
| `node:child_process` | `20` | `20` | `supported` | `NeedsVerification` |
| `node:cluster` | `23` | `23` | `stub-only` | `StubOnly` |
| `node:console` | `23` | `23` | `supported` | `NeedsVerification` |
| `node:constants` | `1` | `1` | `not_listed` | `NeedsVerification` |
| `node:crypto` | `111` | `110` | `partial` | `Partial` |
| `node:dgram` | `1` | `1` | `partial` | `Partial` |
| `node:diagnostics_channel` | `23` | `62` | `supported` | `NeedsVerification` |
| `node:dns` | `26` | `27` | `partial` | `Partial` |
| `node:domain` | `9` | `9` | `stub-only` | `StubOnly` |
| `node:events` | `50` | `50` | `supported` | `NeedsVerification` |
| `node:fs` | `155` | `159` | `partial` | `Partial` |
| `node:fs/promises` | `1` | `1` | `partial` | `Partial` |
| `node:http` | `102` | `103` | `partial` | `Partial` |
| `node:http2` | `105` | `105` | `partial` | `Partial` |
| `node:https` | `11` | `11` | `partial` | `Partial` |
| `node:inspector` | `15` | `18` | `partial` | `Partial` |
| `node:module` | `12` | `19` | `partial` | `Partial` |
| `node:net` | `57` | `61` | `partial` | `Partial` |
| `node:os` | `20` | `20` | `supported` | `NeedsVerification` |
| `node:path` | `12` | `12` | `supported` | `NeedsVerification` |
| `node:perf_hooks` | `43` | `43` | `partial` | `Partial` |
| `node:process` | `1` | `1` | `partial` | `Partial` |
| `node:punycode` | `4` | `4` | `supported` | `NeedsVerification` |
| `node:querystring` | `6` | `6` | `supported` | `NeedsVerification` |
| `node:readline` | `35` | `37` | `supported` | `NeedsVerification` |
| `node:repl` | `8` | `8` | `partial` | `Partial` |
| `node:sea` | `1` | `1` | `partial` | `NotSupported` |
| `node:stream` | `77` | `77` | `supported` | `NeedsVerification` |
| `node:string_decoder` | `3` | `3` | `supported` | `NeedsVerification` |
| `node:sys` | `1` | `1` | `not_listed` | `NeedsVerification` |
| `node:test` | `60` | `74` | `supported` | `NeedsVerification` |
| `node:test/reporters` | `1` | `1` | `not_listed` | `NotSupported` |
| `node:timers` | `21` | `21` | `supported` | `NeedsVerification` |
| `node:tls` | `49` | `51` | `partial` | `Partial` |
| `node:trace_events` | `1` | `1` | `stub-only` | `StubOnly` |
| `node:tty` | `12` | `12` | `supported` | `NeedsVerification` |
| `node:url` | `29` | `30` | `supported` | `NeedsVerification` |
| `node:util` | `93` | `99` | `partial` | `Partial` |
| `node:v8` | `54` | `56` | `partial` | `Partial` |
| `node:vm` | `19` | `21` | `partial` | `Partial` |
| `node:wasi` | `4` | `4` | `stub-only` | `StubOnly` |
| `node:worker_threads` | `34` | `40` | `partial` | `Partial` |
| `node:zlib` | `42` | `50` | `partial` | `Partial` |
| `node:sqlite` | `1` | `27` | `partial` | `Partial` |

## First-Baseline Caveats

- This first generated baseline is intentionally conservative.
- Module and symbol coverage unresolved from the source scrape remain `NeedsVerification` instead of being guessed.
- `support_state` values in this baseline are source- and docs-derived starting points; later compatibility-family work must refine them with measured Nimbus verification.
