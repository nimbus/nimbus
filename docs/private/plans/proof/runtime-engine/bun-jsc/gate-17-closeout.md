# Bun/JSC Gate 17: Closeout

Date: 2026-05-23

Nimbus prior proof revision: `9a8712c7` (`Record Bun fork hold decision`)

Bun proof commit: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Result

The Bun/JSC proof wave is complete through BJ1-BJ7.

Bun/JSC remains:

- proof-only
- `in_process_trusted_only`
- not selectable from Nimbus product metadata
- rejected before invocation when a manifest asks for `runtime_engine:
  "bun_jsc"`
- held as a local Bun proof delta, not a Nimbus-maintained fork

## Completed Gates

| Gate | Result |
| --- | --- |
| BJ1 / Gate 11 | Permission inventory recorded. Bun filesystem/process/network/plugin/FFI/env, process globals, Web APIs, timers, workers, `eval`, and `new Function` are unsafe bypasses in the proof VM. |
| BJ2 / Gate 12 | Memory behavior recorded. JSC exposes pressure signals but no hard per-VM heap limit was observed. |
| BJ3 / Gate 13 | Package/module policy recorded. Program-wrapper is the selected proof lane; dynamic `import("node:fs")` and `Bun.resolve*` are unmediated. |
| BJ4 / Gate 14 | Lifecycle reuse recorded. Trusted generated-wrapper retained VM reuse survives cancellation recovery. |
| BJ5 / Gate 15 | Runtime metadata seam implemented. `bun_jsc` and `program_wrapper` are explicit metadata values but rejected before invocation. |
| BJ6 / Gate 16 | Fork/upstream/hold decision recorded. Hold local proof delta; do not fork or upstream yet. |
| BJ7 / Gate 17 | Closeout verification completed. |

## Final Verification

Nimbus formatting:

```sh
cargo fmt --all --check
```

Result: passed.

Nimbus clippy:

```sh
cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings
```

Result: passed.

Nimbus ignored Bun proof gate:

```sh
cargo test -p nimbus-runtime --test engine_proofs \
  bun_jsc_build_gate_reproduces_from_bun_build_graph \
  -- --ignored --nocapture
```

Result: passed, 1 test.

Note: the original goal text placed `--ignored` before Cargo's `--` separator.
Cargo rejects that placement. The command above is the equivalent libtest form:
the test filter stays before `--`, and `--ignored --nocapture` goes to the test
binary.

Bun native embed proof:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result: passed. The proof printed Gate 11 permission inventory, Gate 12 memory
behavior, Gate 13 package/module policy, Gate 14 lifecycle reuse stress, and
`[build] check-bun-embed-probe done`.

Whitespace:

```sh
git diff --check
```

Result: passed before this closeout record was written and must pass again
before the closeout commit.

## Handoff

The next implementation order should not productize Bun. The next useful work,
if Nimbus chooses to continue this lane, is to specify the missing product
APIs:

- permission hooks for Bun filesystem, network, environment, subprocess,
  worker, dynamic import, FFI/native-addon, and package-loading surfaces
- a Nimbus-owned Bun resolver policy that is not the Deno/V8
  `node_external_packages` lane
- an outer memory/process/sandbox hard-limit policy
- macOS and Linux CI for the Bun embed proof target
- an upstream API proposal only after the above are concrete
