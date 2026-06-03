# Node Compatibility Regression Fixtures

This tree contains Nimbus-authored or Nimbus-adapted regression probes. These
files are intentionally outside the versioned `nodeNN/test` official fixture
roots and must not contribute to official Node fixture denominators.

- `node22/parallel/test-module-wrapper-*` and `node22/parallel/test-vm-context-regression-*` preserve local reduced regressions that were previously mixed into the Node22 official root.
- `async-hooks/test-async-hooks-enable-recursive-fsreqcallback.js` preserves the Nimbus FSREQCALLBACK-specific async_hooks check derived from the official recursive-enable fixture while the official lane root carries the unmodified upstream file.
