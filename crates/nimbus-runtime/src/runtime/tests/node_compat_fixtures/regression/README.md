# Node Compatibility Regression Fixtures

This tree contains Nimbus-authored or Nimbus-adapted regression probes. These
files are intentionally outside the versioned `nodeNN/test` official fixture
roots and must not contribute to official Node fixture denominators.

- `node22/parallel/test-module-wrapper-*` and `node22/parallel/test-vm-context-regression-*` preserve local reduced regressions that were previously mixed into the Node22 official root.
- `async-hooks/test-async-hooks-enable-recursive-fsreqcallback.js` preserves the Nimbus FSREQCALLBACK-specific async_hooks check derived from the official recursive-enable fixture while the official lane root carries the unmodified upstream file.
- `async-hooks/test-async-hooks-no-startup-orphans.js` checks that no runtime-internal async resource is pending when the main module starts, so a hook enabled at top level sees `init` for every resource it observes. Official v20, v22, v24 and v26 pass it.
- `handle-wrap/test-handle-close-contract.js` checks the handle close order: a `'close'` listener drains its nextTick queue before its microtasks, `dgram` `close()` frees the port before it returns, and a close started from a `'close'` listener completes. Official v20, v22, v24 and v26 pass it.
- `perf-hooks/test-perf-hooks-resourcetiming-lane-contract.js` pins the `PerformanceResourceTiming` receiver checks, member shape, and `markResourceTiming` arity per Node major, as observed on the official v20, v22, v24 and v26 binaries. The official fixture leaves these unasserted.
- `stream/test-stream-readable-read-size-lane-contract.js` pins how much `Readable#read()` without a size returns per Node major (all buffered data through v24, one chunk from v26), as observed on the official v20, v22, v24 and v26 binaries. Each official infinite-read fixture asserts only its own release line.
- `stream-iter/test-stream-iter-bare-specifier-{flag,no-flag}.mjs` check that the bare `stream/iter` and `zlib/iter` specifiers name the builtins only with `--experimental-stream-iter`, through `import`, `import.meta.resolve`, `require`, `isBuiltin` and `builtinModules`, and that the `node:` specifiers fail as in Node without the flag. `stream-iter/test-stream-iter-bare-specifier-child-exec-argv.js` checks that a `process.execPath` child reads its own flags, not the parent's. Official v26 passes them.
