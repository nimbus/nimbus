# Core Semantics Node Test Slices

Current upstream Node test-slice manifest for `NLC3`.

Source corpus:

- vendored Node compatibility runner in
  `~/src/github.com/agentstation/deno/tests/node_compat/runner/suite/test`
- pinned local corpus identity:
  `~/src/github.com/agentstation/deno @ v2.7.14-locker.19`
- pinned official Node22 validation corpus:
  `nodejs/node @ v22.15.0`
- pinned official Node20 supported corpus:
  `nodejs/node @ v20.20.2`

This file records the pinned Node test globs and the currently manifested
official-fixture subset for the `NLC3` core semantics family. The canonical
source of truth for the executed subset is
[`CORE_SEMANTICS_BATCH`](../../../../crates/neovex-runtime/src/runtime/tests/node/mod.rs)
plus the explicit watchpoints in the same Rust file; this document summarizes
that state so future work can resume without rediscovering it.

## Initial Slice Map

| Module | Initial upstream test slices |
| --- | --- |
| `node:assert` | `test/parallel/test-assert-*.js`, `test/pseudo-tty/test-assert-*.js` |
| `node:buffer` | `test/parallel/test-buffer-*.js`, `test/sequential/test-buffer-*.js`, `test/pummel/test-buffer-*.js` |
| `node:events` | `test/parallel/test-events-*.js`, `test/wpt/test-events.js` |
| `node:path` | `test/parallel/test-path-*.js`, `test/parallel/test-path.js` |
| `node:url` | `test/parallel/test-url-*.js`, `test/wpt/test-url.js`, `test/known_issues/test-url-parse-conformance.js` |
| `node:console` | `test/parallel/test-console-*.js`, `test/wpt/test-console.js` |
| `node:querystring` | `test/parallel/test-querystring*.js` |
| `node:punycode` | `test/parallel/test-punycode.js` |
| `node:string_decoder` | `test/parallel/test-string-decoder-*.js`, `test/pummel/test-string-decoder-large-buffer.js` |

## Current Neovex Fixture Coverage

These local runtime fixtures currently back the narrow public contract while
the upstream pass-rate lanes are still being wired:

- `runtime::tests::basic_invocation::node22_target_supports_core_semantics_builtins_and_subpaths`
- `runtime::tests::basic_invocation::application_node22_commonjs_package_can_require_core_semantics_builtins`

## Current Manifested Official Subset

The current manifested subset is fully data-driven from the checked-in fixture
roots and the `CORE_SEMANTICS_BATCH` table in
`crates/neovex-runtime/src/runtime/tests/node/mod.rs`.

Current manifested batch counts:

- Node22 default lane: `120` official files
- Node20 supported lane: `116` official files
- Node24 supported lane: `122` staged official files

Family breakdown for the manifested batch:

| Family | Node22 green | Node20 green | Node24 supported staged | Notes |
| --- | ---: | ---: | ---: | --- |
| `assert` | `12` | `10` | `12` | Node22-only `deep-with-error` and `class-destructuring` are manifested separately from the shared official LTS set |
| `buffer` | `52` | `50` | `52` | Public-core imported Buffer corpus now includes the warning/deprecation slices `constructor-deprecation-error`, `nopendingdep-map`, and `pending-deprecation` |
| `console` | `17` | `17` | `17` | Public console behavior now includes `console-tty-colors` alongside the existing constructor, formatting, and stdio slices |
| `events` | `5` | `4` | `5` | Node20 `events.once(..., null)` remains an explicit divergence watchpoint |
| `path` | `13` | `15` | `15` | Node22 `normalize` and `makelong` remain explicit watchpoints; `resolve` is a shared runtime-gap watchpoint |
| `punycode` | `1` | `1` | `1` | Public deprecated-module delivery is green; vendored post-22 `url.parse()` deprecation sample stays outside the official Node22 denominator |
| `querystring` | `4` | `4` | `4` | Fully represented |
| `string_decoder` | `3` | `3` | `3` | Fully represented |
| `url` | `13` | `12` | `13` | Node22-only invalid-file-url-path input file is manifested separately from the shared official LTS set |

Imported public-core official corpus status:

- `104` imported official Node20 public-core files are now represented by
  either the manifested green batch or an explicit watchpoint
- no imported public-core fixture files remain unstaged in the current `NLC3`
  corpus
- the Node24 supported corpus is staged from official `nodejs/node v24.15.0`,
  but it is not currently a green claim: the explicit supported lane still aborts
  early through a `rusty_v8` weak-handle panic near `test-buffer-alloc.js`

## Representative Harness Requirements

Sampled files from the pinned corpus show that a first Neovex upstream runner
cannot assume "evaluate one JS file and read the exit code" is enough:

- `parallel/test-assert.js`
  - requires `../common`
  - uses `node:test`
  - uses `vm`
  - mutates `process.env` based on `process.stdout.isTTY`
- `parallel/test-buffer-from.js`
  - requires `../common.invalidArgTypeHelper`
  - uses `vm`
- `parallel/test-url-format-whatwg.js`
  - requires `../common.hasIntl`
  - uses `node:test`
- `parallel/test-console-methods.js`
  - requires `../common`
  - depends on real `console`, `process.stdout`, and constructor semantics

That means the first honest Neovex-owned runner likely needs:

- suite-relative CommonJS resolution into the vendored `test/common` harness
- at least a narrow `node:test` posture or a documented filtered subset that
  avoids it
- predictable `process`, stdio, and `console` behavior
- a test-file execution model closer to Node/Deno CLI semantics than to the
  current `__neovexInvoke` bundle fixture model

Current Neovex-owned harness capabilities:

- suite-relative `require("../common")` resolution inside the staged bundle root
- post-import `mustCall` / `mustCallAtLeast` / `mustNotCall` verification via a
  minimal `test/common` shim
- `invalidArgTypeHelper` support for message-shaped assertion files
- `expectWarning` support for warning-based files that stay inside the current
  runtime contract
- `getArrayBufferViews()` and `getBufferSources()` support for upstream buffer
  and `string_decoder` files that expect the stock Node `test/common` helpers
- `hasIntl` support for URL-format files that guard behavior on ICU presence
- `process.stdout` / `process.stderr` writable stream objects with constructor
  semantics strong enough for the pinned `console` fixture subset
- `require("punycode")` deprecation-warning delivery through
  `process.on("warning")` in the CommonJS wrapper path
- a minimal embedded `Deno.test` bridge plus explicit pending-test flush so
  top-level `node:test`-backed fixtures can execute without exposing
  `globalThis.Deno`

## Notes

- These slices came from the pinned Deno-vendored Node corpus currently present
  in `agentstation/deno`, not from memory.
- The vendored Deno `node_compat` runner is a useful corpus source, but it is
  not a drop-in Neovex harness. Its Rust test runner shells out to a Deno CLI
  executable via `DENO_TEST_UTIL_DENO_EXE` and assumes Deno CLI argument and
  process semantics, so Neovex needs a dedicated slice runner before `NLC3`
  can claim upstream pass-rate evidence.
- `node:url` currently includes `urlpattern`-adjacent coverage in the vendored
  corpus, but `NLC3` should keep the final Node22 pass-rate calculation scoped
  to the `node:url` contract it publicly claims.
- The current green subset is no longer just runner-viability evidence. It is
  now the full imported public-core corpus for the families owned by `NLC3`,
  split cleanly into:
  - manifested green batch entries
  - explicit classified watchpoints
- The explicit gap surface is now tracked as ten ignored fixture families
  (`test-assert-deep.js`, `test-assert-partial-deep-equal.js`,
  `test-buffer-isascii.js`, `test-buffer-isutf8.js`,
  `test-console-issue-43095.js`, `test-events-once.js`,
  `test-path-makelong.js`, `test-path-normalize.js`,
  `test-path-resolve.js`, and the vendored-only
  `test-url-parse-deprecation.js` sample), represented by fifteen ignored Rust
  watchpoint tests across the Node20 and Node22 lanes. The focused contract
  lanes remain green, but the aggregate local `node_compat::` lane is not yet
  a closeout signal: after the latest batch widened `buffer` and `console`,
  the single-process `node_compat::` run still ends in a `SIGSEGV` after
  starting the back-to-back Node20 and Node22 manifested batches, so that
  composite lane stays a harness-stability watchpoint rather than a support
  claim.
- The current remaining-file inventory is now explicit instead of implicit:
  no imported public-core official files remain unstaged for `NLC3`; the only
  remaining tracked items are explicit watchpoints, 16 official files that
  clearly map to later host/process/TTY/module families, and 3 upstream
  internal-only helpers that should not count toward the public `NLC3`
  denominator.
- The current Node20 supported lane uses the official `nodejs/node v20.20.2`
  files for the same staged subset instead of reusing the Deno-vendored
  copies blindly, because multiple files differ textually between the corpora
  even when the exercised behavior still matches.
- The first checked-in upstream `node:assert` slice now uses
  `test-assert-async.js`, `test-assert-calltracker-getCalls.js`,
  `test-assert-calltracker-report.js`, `test-assert-calltracker-verify.js`,
  `test-assert-checktag.js`, `test-assert-fail-deprecation.js`,
  `test-assert-fail.js`, `test-assert-first-line.js`, and
  `test-assert-if-error.js`. Official `nodejs/node v20.20.2` and
  `nodejs/node v22.15.0` are still byte-identical for seven of those files. The
  pinned Deno-vendored copies still match for `test-assert-fail.js` and
  `test-assert-if-error.js`, but `test-assert-async.js` had vendored drift, so
  Neovex stages one shared official LTS body for both lanes instead of growing
  a needless version split. `test-assert-async.js` also gives the current
  harness its first upstream-backed top-level async `node:assert` proof,
  `test-assert-fail-deprecation.js` proves `DEP0094` warning delivery through
  the current `expectWarning()` path, `test-assert-first-line.js` proves the
  bundle runner can stage checked-in `test/fixtures/*` helper files without
  leaning on the host filesystem, and the `CallTracker` `report()` / `verify()`
  pair proves the current lane can execute another shared-official assert batch
  even when Node emits the new `DEP0173` deprecation warning.
- The first explicit split-LTS `node:assert` batch now uses
  `test-assert-calltracker-getCalls.js` and `test-assert-checktag.js`.
  `checktag` differs between Node20 and Node22 in global/globalThis treatment
  and in exact multiline assertion text, and `CallTracker.getCalls()` differs
  in the `node:test` concurrency option (`true` vs `!process.env.TEST_PARALLEL`).
  Neovex stages both official LTS bodies directly instead of flattening those
  differences away, and the checked-in `test/common` shim now intercepts only
  the harness-owned `TEST_PARALLEL` env probe so the official Node22 file can
  execute without broadening the public application-preset env contract.
- The next assert batch keeps the same source-first rule but mixes one paired
  LTS file and one Node22-only file: `test-assert-typedarray-deepequal.js`
  plus `test-assert-deep-with-error.js`. `typedarray-deepequal` differs
  materially between Node20 and Node22 because the newer file adds
  `Float16Array` and `partialDeepStrictEqual()` coverage, so both official LTS
  bodies are staged directly. `deep-with-error` has no official
  `nodejs/node v20.20.2` counterpart, so it widens only the measured Node22
  lane without pretending Node20 has the same file.
- The next Node22-only assert expansion is
  `test-assert-class-destructuring.js`. It has no official
  `nodejs/node v20.20.2` counterpart, but it stays inside the same
  `node:test` + `Assert` class semantics seam and adds no new harness or host
  dependency beyond the current Node22 lane.
- A focused follow-on assert spike against the canonical local Node source
  (`~/src/github.com/nodejs/node`) now shows that the remaining official
  `v22.15.0` `node:assert` files outside the current green subset cluster into
  two buckets:
  - real current-runtime watchpoints (`test-assert-deep.js` and
    `test-assert-partial-deep-equal.js`)
  - later host/process seams (`test-assert-builtins-not-read-from-filesystem.js`,
    `test-assert-calltracker-calls.js`, and
    `test-assert-esm-cjs-message-verify.js`)
  That means further `NLC3` batching should prefer other core-semantics families
  until the plan intentionally promotes the subprocess / exit-handler seam.
- A broader local assert-suite triage against `~/src/github.com/nodejs/node`
  now separates the remaining files into:
  - current-batch-friendly pure assert semantics (`checktag`, deeper assert
    value formatting, selected `CallTracker` follow-ons)
  - later host/process seams (`builtins-not-read-from-filesystem`,
    `esm-cjs-message-verify`, `CallTracker.calls()` exit behavior)
  This is the batching rule future `NLC3` work should keep following instead of
  treating every remaining assert file as independent.
- The newest shared-LTS path/querystring batch now covers
  `test-path-extname.js`, `test-path-parse-format.js`,
  `test-path-relative.js`, and `test-querystring-multichar-separator.js`
  through one checked-in official LTS body for both lanes. `test-path-normalize.js`
  is the new explicit split-LTS path seam: official Node20 and official Node22
  differ there after the Windows device-path hardening work, so both official
  fixture bodies are staged directly and the current runtime only matches the
  Node20 expectation today.
- The next shared-LTS events/path batch now covers
  `test-events-listener-count-with-listener.js`, which is still
  byte-identical between official `nodejs/node v20.20.2` and
  `nodejs/node v22.15.0` and is green in both lanes. The same source-first
  review also surfaced `test-path-resolve.js` as a shared cross-LTS runtime
  gap rather than version drift: both official LTS files currently fail in the
  embedded runtime because `ext:deno_node/path/_win32.ts` rejects
  drive-letter-less `win32.resolve()` inputs without a CWD, so the file is now
  pinned as an ignored watchpoint instead of being counted green in either
  lane.
- The next two manifest-driven buffer batches now follow the same
  tagged-local-Node rule rather than reintroducing one-off wrapper growth.
  Sixteen additional files are green across the live Node22 lane, the official
  Node20 supported lane, and the ignored Node24 supported lane:
  `test-buffer-fill.js`, `test-buffer-indexof.js`,
  `test-buffer-includes.js`, `test-buffer-readint.js`,
  `test-buffer-readuint.js`, `test-buffer-write.js`,
  `test-buffer-writeint.js`, `test-buffer-writeuint.js`,
  `test-buffer-ascii.js`, `test-buffer-badhex.js`,
  `test-buffer-inspect.js`, `test-buffer-readdouble.js`,
  `test-buffer-readfloat.js`, `test-buffer-tojson.js`,
  `test-buffer-writedouble.js`, and `test-buffer-writefloat.js`.
  Most of that group is byte-identical across official
  `nodejs/node v20.20.2`, `v22.15.0`, and `v24.15.0`; `test-buffer-indexof.js`
  splits once between Node20 and Node22/Node24, and `test-buffer-write.js`
  uses separate official bodies for all three versions.
- A follow-on narrowed buffer batch now adds
  `test-buffer-compare-offset.js` and `test-buffer-fakes.js` green across the
  live Node22 lane, the official Node20 supported lane, and the ignored
  Node24 supported lane. The same widening pass also repaired one real runtime
  contract by explicitly exposing `structuredClone` in the embedded Node22
  bootstrap. That fixed the missing-global failure shape and exposed the deeper
  shared runtime seam underneath it: transfer-style `structuredClone()` still
  leaves the original `ArrayBuffer` usable in the embedded runtime, so
  `test-buffer-isascii.js` and `test-buffer-isutf8.js` are now pinned as
  shared ignored watchpoints instead of being counted green. The remaining
  `safe-unsafe` and `swap` candidates from that spike are intentionally left
  out of the green manifest until they are rerun without the transient
  `SIGSEGV` that surfaced during the same widening attempt.
- `test-url-parse-format.js` and `test-url-parse-invalid-input.js` are the
  current highest-signal `node:url` divergences. The Deno-vendored corpus had
  already moved to hard-throw invalid-port semantics, while the official
  Node20 and Node22 LTS files still share `DEP0170` warning behavior. Neovex
  therefore keeps explicit official `node22/` and `node20/` fixture roots for
  those files and treats the vendored Deno copy only as a sampled watchpoint.
- A broader drift audit of the currently executed `node:url` subset showed a
  second, safer pattern: official `nodejs/node v20.20.2` and
  `nodejs/node v22.15.0` are still byte-identical for
  `test-url-domain-ascii-unicode.js`, `test-url-pathtofileurl.js`, and
  `test-url-fileurltopath.js`, while the pinned Deno-vendored copies lag one or
  more of those official fixtures. Neovex therefore now executes one shared
  checked-in official LTS body for those files in both runtime lanes instead of
  treating the vendored Deno copies as a second source of truth.
- `test-url-invalid-file-url-path-input.js` is different in the other
  direction: it is present in the pinned Deno-vendored Node22 corpus but has
  no official `nodejs/node v20.20.2` counterpart, so it widens the Node22 lane
  without changing the measured Node20 subset size.
- The canonical code-first source for future fixture drift review is now the
  local `~/src/github.com/nodejs/node` checkout rather than ad hoc remote fetch
  comparison, with official Node20/Node22 files staged from there whenever the
  LTS lines still share one body.
- A direct code-first drift review of official `nodejs/node v20.20.2` and
  `nodejs/node v22.15.0` `lib/url.js` showed that the legacy parser core is
  still materially the same across both LTS lines. Neovex used that review to
  batch the `warnInvalidPort` family fix in `agentstation/deno
  v2.7.14-locker.13` instead of continuing fixture-by-fixture whack-a-mole.
- The latest Deno-family runtime repin is `agentstation/deno
  v2.7.14-locker.19`, which closes the imported `test-events-add-abort-listener.mjs`
  seam by wiring `events.addAbortListener()` through the real web
  `AbortSignal` stop-immediate-propagation hook in `ext:deno_web/02_event.js`
  rather than the Node-internal event-target shim.
