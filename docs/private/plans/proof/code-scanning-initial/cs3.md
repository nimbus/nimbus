# CS3 — archive after merge

All six security closeout pull requests merged into `main` on 2026-09-15. The
plan leaves the active index and joins `archive/`.

## Merged repairs

| PR | Squash commit | Alerts | Repair |
| --- | --- | --- | --- |
| [#345](https://github.com/nimbus/nimbus/pull/345) | `3824e4f60` | #3, #4 | Firebase field paths use own-property reads and data-property writes. |
| [#346](https://github.com/nimbus/nimbus/pull/346) | `e02d060a9` | #1, #2, #6919 | Codegen reference trees use own-property lookup, data-property writes, and computed `__proto__` keys. |
| [#347](https://github.com/nimbus/nimbus/pull/347) | `2c2ddcd9a` | #7244 | Bootc binds `--transport=` and `--tag=` and terminates option parsing with `--`. |
| [#348](https://github.com/nimbus/nimbus/pull/348) | `11384e0a0` | #1942 | The filesystem canary requires a capability denial from each probe. |
| [#349](https://github.com/nimbus/nimbus/pull/349) | `ae7dd5694` | #7100 | Brotli FFI destruction reads the allocator opaque from the moved container. |
| [#350](https://github.com/nimbus/nimbus/pull/350) | `cca50ec48` | #7141 | The vendored object-store client no longer offers a TLS verification bypass. |

## Verification before each merge

The owner ran focused tests against each branch rather than hosted CI, on the
owner's instruction to merge on targeted evidence and repair any Actions
failure after merge.

- #347: `cargo test -p nimbus-cli --lib machine::api::bootc` — 4 passed.
  `run_bootc_command` uses `Command::new(&bootc).args(args)`, so the exposure
  was option injection and not shell injection.
- #348: `cargo test -p nimbus-runtime --lib host_heavy` — 2 verifier tests
  passed, 3 Node canary batches stay ignored without `npm ci`.
  `assert_denial_contains_any` keeps ten other callers, so the narrowed
  filesystem path leaves no dead code.
- #349: the vendored crate is not a workspace member, so the tests ran from an
  isolated copy with `ffi-api` enabled — 35 FFI tests and 93 integration tests
  passed, including both new allocator-ownership tests.
- #350: an isolated copy of `object_store-0.14.0` with `aws,reqwest` — 162 lib
  tests passed, including both bypass-rejection tests. No Nimbus caller used
  the removed setter. `crates/nimbus-server/src/tests/tls_serve.rs` keeps
  `danger_accept_invalid_certs` for a `#[cfg(test)]` client against a
  self-signed localhost fixture, which is out of scope.
- #346: `node ./src/selftest.mjs` in `packages/codegen` passed. Mutation check:
  removing the computed-key branch from `renderPropertyKey` fails the new
  fixture on `/\["__proto__"\]:/`.
- #345: `npm test` in `packages/firebase` passed. Mutation check: restoring the
  plain assignment in `setValueAtFieldPath` fails the new fixture, because the
  `__proto__` write no longer lands as an own field.

## Remaining GitHub backlog

- CodeQL must re-analyse `main` and confirm closure of the nine repaired
  security alerts. No dismissal was recorded for them.
- 162 quality alerts stay open. They were outside this security batch and have
  no owner yet.
- Hosted Actions runs for the six merges were not awaited. Any failure they
  report is repaired on `main`.
