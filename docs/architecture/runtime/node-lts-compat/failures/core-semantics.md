# Core Semantics Failure Inventory

Status: `closeout_ready`

This file is the checked-in failure inventory for `NLC3`.

It now records the measured official Node22 and Node20 public-core corpus for
the `NLC3` families. No imported public-core fixture files remain unstaged;
the only remaining `NLC3` deltas are explicit classified watchpoints and the
later-family/internal buckets called out below.

## Node22 Upstream Slice Status

- Status: `classified`
- Current measured subset:
  - `120` official files passed
  - `0` failed
  - `8` official watchpoints outside the green subset
  - `1` vendored-only sampled watchpoint outside the official Node22 denominator
  - effective official pass rate: `120 / 128` (`93.8%`)
- Green executed upstream files:
  - canonical list lives in
    `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md`
  - latest widened additions include the remaining public Buffer constructor
    tail plus `safe-unsafe`, `sharedarraybuffer`, and `swap`
- Ignored watchpoints in the currently executed subset:
  - `test/parallel/test-assert-deep.js`
    - classification: `upstream_deno_gap`
    - reason: the shared official Node20/Node22 file still fails in the current
      runtime on both the Node20 diff-shape assertion and the Node22 GH-14441
      circular deep-equality cases. The Deno-vendored corpus has drifted away
      from the official Node22 expectation here, so the vendored green state is
      not authoritative for `NLC3`.
  - `test/parallel/test-assert-partial-deep-equal.js`
    - classification: `known_runtime_gap`
    - reason: the official Node22 file currently aborts through a
      `rusty_v8` weak-handle panic in the embedded runtime path instead of
      producing a normal assertion outcome
  - `test/parallel/test-buffer-isascii.js`
    - classification: `known_runtime_gap`
    - reason: transfer-style `structuredClone()` still leaves the original
      `ArrayBuffer` usable in the embedded runtime, so the detached-buffer
      `ERR_INVALID_STATE` assertion path is not reached
  - `test/parallel/test-buffer-isutf8.js`
    - classification: `known_runtime_gap`
    - reason: transfer-style `structuredClone()` still leaves the original
      `ArrayBuffer` usable in the embedded runtime, so the detached-buffer
      `ERR_INVALID_STATE` assertion path is not reached
  - `test/parallel/test-console-issue-43095.js`
    - classification: `known_runtime_gap`
    - reason: revoked-proxy inspection still throws inside the Deno-family
      inspect stack instead of formatting like upstream Node
  - `test/parallel/test-path-normalize.js`
    - classification: `upstream_deno_gap`
    - reason: official Node20 and official Node22 now differ in the Windows
      device-path expectations. The current runtime still matches the older
      Node20 `\\\\?\\?\\D:\\Test` behavior, while official Node22 v22.15.0
      expects the hardened `\\\\?\\test\\?\\D:\\Test` result
  - `test/parallel/test-path-makelong.js`
    - classification: `upstream_deno_gap`
    - reason: official Node22 v22.15.0 expects
      `path.win32.toNamespacedPath('\\\\?\\foo')` to retain the trailing slash,
      while the current runtime still returns the older Node20 `\\\\?\\foo`
      result
  - `test/parallel/test-path-resolve.js`
    - classification: `known_runtime_gap`
    - reason: the shared official Node20/Node22 file currently fails in the
      embedded runtime because `ext:deno_node/path/_win32.ts` rejects
      drive-letter-less `win32.resolve()` inputs without a CWD, so neither
      LTS line is green there yet
  - `test/parallel/test-url-parse-deprecation.js`
    - classification: `upstream_node_delta`
    - reason: pinned Deno-vendored file tracks post-22 `DEP0169` semantics,
      but official `nodejs/node v22.15.0` has no counterpart file, so it
      remains a sampled watchpoint instead of a green/failed contract gate
- Classified failures in the currently executed subset:
  - none

## Node20 Upstream Slice Status

- Status: `classified`
- Current measured subset:
  - `116` official `nodejs/node v20.20.2` files passed
  - `0` failed
  - `6` ignored watchpoints or documented divergences outside the green subset
  - effective official pass rate: `116 / 122` (`95.1%`)
- Divergence from the Node22 staged subset:
  - none in behavior for the currently executed 89-file green paired subset
  - several fixture files differ textually from the Deno-vendored Node22 corpus,
    so Neovex keeps a separate pinned `node20/` fixture root instead of
    pretending the Node22 staged copies are identical
  - `test-path-join.js` is one such divergence: the pinned Deno-vendored Node22
    corpus carries extra UNC join assertions that are absent from the official
    Node20 file, so both copies are checked in deliberately
  - `test-url-parse-format.js` is another: the official Node20 file carries an
    extra Git URL assertion that is absent from the pinned Deno-vendored Node22
    corpus, so both fixture roots stay checked in deliberately
  - `test-url-parse-invalid-input.js` is no longer a Node20 divergence after
    the batch URL drift review. Official Node20 and official Node22 both still
    expect `DEP0170` warning semantics here, so Neovex fixed one shared
    runtime seam instead of maintaining version-specific behavior.
  - `test-url-domain-ascii-unicode.js`, `test-url-pathtofileurl.js`, and
    `test-url-fileurltopath.js` also differ textually from the Deno-vendored
    Node22 corpus, but the official `nodejs/node v20.20.2` and
    `nodejs/node v22.15.0` files are still byte-identical for those cases.
    Neovex now runs one shared official LTS fixture body for both lanes there
    instead of pretending the vendored Node22 copies are the canonical source.
  - `test-assert-async.js` also differs from the Deno-vendored Node22 corpus,
    but the official `nodejs/node v20.20.2` and `nodejs/node v22.15.0` files
    are still byte-identical there. Neovex now runs one shared official LTS
    fixture body for both lanes, which adds the first upstream-backed top-level
    async `node:assert` proof without inventing a fake version split.
  - `test-assert-fail-deprecation.js` and `test-assert-first-line.js` also use
    one shared official LTS body across both lanes. `test-assert-fail-deprecation.js`
    adds explicit `DEP0094` warning-path coverage, and `test-assert-first-line.js`
    proves the bundle runner can stage the checked-in `test/fixtures` helper
    files that the official Node test expects.
  - `test-assert-calltracker-report.js` and
    `test-assert-calltracker-verify.js` also use one shared official LTS body
  across both lanes. They add the first checked-in `assert.CallTracker`
  coverage and surface Node's new `DEP0173` deprecation warning without
  requiring a separate process-host seam.
  - `test-assert-calltracker-getCalls.js` and `test-assert-checktag.js` are
    explicit split-LTS files. `CallTracker.getCalls()` differs in the
    `node:test` concurrency option (`true` in Node20 versus
    `!process.env.TEST_PARALLEL` in Node22), and `checktag` differs in
    global/globalThis handling plus exact multiline assertion text, so both
    official LTS bodies stay checked in deliberately.
  - `test-assert-typedarray-deepequal.js` is also an explicit split-LTS file.
    Node22 adds `Float16Array` plus `partialDeepStrictEqual()` coverage there,
    while the official Node20 file is materially narrower, so both official
    bodies stay checked in deliberately instead of collapsing the distinction.
  - `test-assert-class-destructuring.js` has no official
    `nodejs/node v20.20.2` counterpart, so it currently widens only the
    measured Node22 lane.
  - `test-assert-deep.js` is now a shared official-LTS ignored repro instead of
    a counted green subset file. The same official file currently diverges in
    both lanes, so `NLC3` records it as a pinned watchpoint instead of
    pretending either LTS line is green there.
  - `test-assert-deep-with-error.js` has no official `nodejs/node v20.20.2`
    counterpart, so it currently widens only the measured Node22 lane.
  - `test-path-extname.js`, `test-path-parse-format.js`,
    `test-path-relative.js`, and `test-querystring-multichar-separator.js`
    now join the shared-official-LTS green subset because the official Node20
    and Node22 files are still byte-identical there.
  - `test-events-listener-count-with-listener.js` now also joins the shared-
    official-LTS green subset because the official Node20 and Node22 files are
    still byte-identical there and the current runtime matches both lanes.
  - `test-path-normalize.js` is now an explicit split-LTS divergence. Official
    Node20 remains green, while official Node22 expects the newer post-CVE
    Windows device-path semantics and is currently classified as an ignored
    Node22 watchpoint.
  - `test-path-resolve.js` is now a shared ignored repro outside the green
    subset. The official Node20 and official Node22 files still share the same
    body there, and both currently fail because the embedded runtime rejects
    drive-letter-less `win32.resolve()` inputs without a CWD.
  - `test-events-once.js` is now an explicit Node20-only divergence outside
    the green subset. Official Node20 still accepts `once(emitter, event, null)`,
    while the current runtime matches the newer Node22 invalid-options
    behavior and rejects `null`.
  - `test-path-makelong.js` is now Node20-green but Node22-divergent. Official
    Node20 and official Node24 still expect the older `\\\\?\\foo` result for
    `path.win32.toNamespacedPath('\\\\?\\foo')`, while official Node22 expects
    the newer trailing-slash shape.
  - `test-url-invalid-file-url-path-input.js` has no official `nodejs/node
    v20.20.2` counterpart, so it currently widens only the measured Node22 lane
- Closeout state:
  - the imported official Node20 public-core corpus for `NLC3` is now fully
    represented by either the green manifested batch or an explicit watchpoint
  - no imported public-core official files remain unstaged
  - all current Node20 divergences from Node22 are recorded in this inventory
    or the public matrix

## Remaining Official-File Buckets

- Public-core `NLC3` work still unstaged: `0` files
  - all imported official public-core fixture files are now represented by
    either the manifested batch or an explicit watchpoint
- Cross-family files that should move with later roadmap items instead of
  inflating the `NLC3` denominator: `16` files
  - examples:
    - `test-assert-esm-cjs-message-verify.js` → loader / host-process seam
    - `test-console-diagnostics-channels.js` → `diagnostics_channel`
    - `test-console-tty-colors*.js` / `test-console-stdio-*.js` → process/TTY/stream seam
    - `test-buffer-constructor-node-modules*.js` and `test-buffer-zero-fill-*.js` → module / CLI / process seam
- Internal-only upstream files that should not count toward the public
  compatibility claim: `3` files
  - `test-assert-myers-diff.js`
  - `test-events-customevent.js`
  - `test-url-is-url-internal.js`

## Current Local Evidence

- `runtime::tests::basic_invocation::node22_target_supports_core_semantics_builtins_and_subpaths`
- `runtime::tests::basic_invocation::application_node22_commonjs_package_can_require_core_semantics_builtins`
- `runtime::tests::node_compat::node20_supported_lane_executes_official_core_semantics_subset`
- `runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`
- `runtime::tests::node_compat::node24_supported_lane_executes_manifested_core_semantics_subset` *(ignored by default; explicit supported-lane watchpoint, not a support claim; currently aborts early through a `rusty_v8` weak-handle panic near `test-buffer-alloc.js`)*
- `docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md` (canonical family/count summary for the 120-file Node22 green batch, the paired 116-file official Node20 supported subset, the 122-file staged Node24 supported subset, and the watchpoint-backed remainder)
- `runtime::tests::node_compat::node20_assert_deep_watchpoint`
- `runtime::tests::node_compat::node22_assert_deep_watchpoint`
- `runtime::tests::node_compat::node22_assert_partial_deep_equal_watchpoint`
- `runtime::tests::node_compat::node20_buffer_isascii_watchpoint`
- `runtime::tests::node_compat::node22_buffer_isascii_watchpoint`
- `runtime::tests::node_compat::node20_buffer_isutf8_watchpoint`
- `runtime::tests::node_compat::node22_buffer_isutf8_watchpoint`
- `runtime::tests::node_compat::node20_console_issue_43095_watchpoint`
- `runtime::tests::node_compat::node22_console_issue_43095_watchpoint`
- `runtime::tests::node_compat::node20_events_once_watchpoint`
- `runtime::tests::node_compat::node22_path_makelong_watchpoint`
- `runtime::tests::node_compat::node22_path_normalize_watchpoint`
- `runtime::tests::node_compat::node20_path_resolve_watchpoint`
- `runtime::tests::node_compat::node22_path_resolve_watchpoint`
- `runtime::tests::node_compat::node22_url_parse_deprecation_watchpoint`

Package/framework canary note:

- no package or framework canaries are mapped exclusively to the `NLC3`
  built-in core-semantics family yet
- broader ecosystem canaries start in later families where `process`,
  streams, filesystem, networking, and loader/host-process behavior become the
  externally meaningful contract

## Harness Gap

- The pinned vendored Deno `tests/node_compat` runner cannot be pointed at
  Neovex directly. It shells out to a Deno CLI executable via
  `DENO_TEST_UTIL_DENO_EXE` and expects Deno CLI argument/process semantics,
  not `neovex` runtime invocation semantics.
- `NLC3` therefore still needs a Neovex-owned upstream-slice runner that can:
  - execute the pinned Node corpus against `RuntimePreset::Application` /
    `CompatibilityTarget::Node22`
  - capture stdout/stderr/exit status in a Node-test-shaped way
  - produce repeatable per-family pass/fail counts for both Node22 and Node20
- The current Neovex-owned runner now proves the first narrow subset can run
  with checked-in `test/common` shims, but it still does not emulate general
  Node CLI harness behavior or the full `test/common` contract.
- The same runner now also executes official `test-assert-async.js` cleanly in
  both Node22 and Node20 lanes, which shows the current `__neovexInvoke` path
  plus `nextTick` drains are already strong enough for at least one upstream
  top-level `Promise.all(...).then(common.mustCall())` assertion file.
- The same runner now also executes official `test-assert-fail-deprecation.js`
  and `test-assert-first-line.js` cleanly in both lanes, which proves the
  current harness can deliver `DEP0094` warning expectations and can stage the
  checked-in `test/fixtures/*` helper files those upstream Node tests depend on.
- The same runner now also executes official
  `test-assert-calltracker-report.js` and
  `test-assert-calltracker-verify.js` cleanly in both lanes, which proves the
  current harness can absorb `DEP0173` `assert.CallTracker` deprecation warnings
  while still asserting the current `report()` / `verify()` behavior.
- The same runner now also executes the first explicit split-LTS assert
  bodies (`test-assert-calltracker-getCalls.js`,
  `test-assert-checktag.js`, and `test-assert-typedarray-deepequal.js`) with a
  narrow `TEST_PARALLEL` env shim that only normalizes the harness-owned
  Node22 `process.env.TEST_PARALLEL` probe to `undefined` when the underlying
  application-preset env proxy would deny it. That keeps the public env
  contract unchanged while allowing the official Node22 file to execute.
- The same runner now also executes official Node22
  `test-assert-deep-with-error.js`, which widens the measured Node22 lane with
  deeper `Error.cause` assertion semantics without introducing a new Node20
  claim, because no official `nodejs/node v20.20.2` counterpart exists.
- The same runner now also executes official Node22
  `test-assert-class-destructuring.js`, which widens the measured Node22 lane
  with `Assert` class destructuring semantics and still adds no new host or
  filesystem seam beyond the current `node:test` bridge.
- The same runner now also executes two larger tagged-Node buffer batches
  across all three staged versions: `test-buffer-fill.js`,
  `test-buffer-indexof.js`, `test-buffer-includes.js`,
  `test-buffer-readint.js`, `test-buffer-readuint.js`,
  `test-buffer-write.js`, `test-buffer-writeint.js`,
  `test-buffer-writeuint.js`, `test-buffer-ascii.js`,
  `test-buffer-badhex.js`, `test-buffer-inspect.js`,
  `test-buffer-readdouble.js`, `test-buffer-readfloat.js`,
  `test-buffer-tojson.js`, `test-buffer-writedouble.js`, and
  `test-buffer-writefloat.js`. That confirms the manifest-driven direction is
  scaling by family instead of requiring one more hand-written repro wrapper
  per fixture.
- The same runner now also executes a narrowed follow-on buffer batch with
  `test-buffer-compare-offset.js` and `test-buffer-fakes.js` green across the
  live Node22 lane, the official Node20 supported lane, and the ignored
  Node24 supported lane.
- The same runner now also proves a deeper shared runtime gap instead of just a
  missing global: explicitly exposing `structuredClone` in the embedded Node22
  bootstrap fixed the `ReferenceError` shape, but
  `test-buffer-isascii.js` and `test-buffer-isutf8.js` still fail because
  transfer-style `structuredClone()` leaves the original `ArrayBuffer` usable
  in the embedded runtime rather than detached. Those two files are now pinned
  as shared ignored watchpoints instead of being counted green.
- The runner now executes six real `node:test`-backed upstream URL files
  (`test-url-format.js`, `test-url-format-invalid-input.js`,
  `test-url-domain-ascii-unicode.js`, `test-url-format-whatwg.js`,
  `test-url-fileurltopath.js`, and `test-url-parse-format.js`) via the embedded
  `Deno.test` bridge, but broader `node:test` hooks, suites, reporters, and
  full CLI semantics are still out of contract for `NLC3`.
- The matching Deno-fork regression source now exists in
  `~/src/github.com/agentstation/deno/tests/unit_node/vm_test.ts`, but this
  machine could not execute the fork-built Deno lane because the local toolchain
  lacks `cmake`. Neovex therefore keeps the runtime proof grounded in the
  repinned `v2.7.14-locker.19` integration lane rather than overstating the
  Deno-side local verification result.
- The full `node_compat::` lane is still a stability watchpoint, not a green
  closeout lane. The focused Node22 and Node20 manifested lanes both pass in
  isolation, but the aggregate `node_compat::` run still ends in a `SIGSEGV`
  after starting the back-to-back manifested batches in one Rust test process.
  Treat that as a harness-stability gap until the combined lane becomes as
  repeatable as the focused per-lane runs.
- Representative sampled tests also confirm that the runner must account for
  more than raw builtin imports:
  - suite-relative `require("../common")` harness access
  - `node:test` in at least some `assert` and `url` cases
  - `process.stdout.isTTY`, `console`, and related process/stdio behavior

## Open Closeout Work

- Add an upstream slice runner or documented manual invocation for the `NLC3`
  test corpus.
- Record Node22 pass-rate results.
- Record Node20 pass-rate results and any divergence.
- Add package canary outcomes if any are mapped to the core-semantics family.
