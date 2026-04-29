# Core Semantics Node Test Slices

Initial upstream Node test-slice manifest for `NLC3`.

Source corpus:

- vendored Node compatibility runner in
  `~/src/github.com/agentstation/deno/tests/node_compat/runner/suite/test`

This file is intentionally a first-pass manifest, not a success claim. It
records the initial Node test globs that correspond to the `NLC3` core
semantics family so future work can run and shrink explicit slices instead of
rediscovering them.

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

## Notes

- These slices came from the pinned Deno-vendored Node corpus currently present
  in `agentstation/deno`, not from memory.
- `node:url` currently includes `urlpattern`-adjacent coverage in the vendored
  corpus, but `NLC3` should keep the final Node22 pass-rate calculation scoped
  to the `node:url` contract it publicly claims.
- `NLC3` still needs an explicit run manifest and failure inventory before the
  family can close.
