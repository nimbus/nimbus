# Deno-Family Runtime Fork Bump Ledger

Status: current operating ledger

This ledger is the human-readable companion to
[`scripts/verify-deno-fork-provenance.sh`](../../../scripts/verify-deno-fork-provenance.sh).
The script proves the Cargo source closure; this file records why each carried
fork patch exists, whether it should move upstream, and what proof is required
before Nimbus releases it.

Operating workflow:
[`docs/operating/deno-fork-workflow.md`](../../staging/operating/deno-fork-workflow.md).

## Current Runtime Pins

| Fork | Upstream base | Nimbus tag | Commit SHA resolved by Cargo | Cargo source proof |
| --- | --- | --- | --- | --- |
| `nimbus/deno` | `v2.8.0` | `v2.8.0-nimbus.5` | `37b6333a1f703db523efe8a703d36f2152ad087a` | `deno_core`, `deno_node`, `node_resolver`, `serde_v8`, and related patch-sensitive crates resolve to this tag/SHA. |
| `nimbus/rusty_v8` | `v149.0.0` | `v149.0.0-nimbus.1` | `9b77553883f1117ab3df62709b8673b803ed721b` | `v8` resolves to this tag/SHA. |

## Disposition Categories

- **Upstream Deno-family**: general Deno, Node compatibility, or `rusty_v8`
  behavior that should be proposed upstream or dropped when upstream ships it.
- **Nimbus-only host integration**: embedding, release, naming, or product
  host-boundary code that is intentionally local to Nimbus.
- **Temporary carry**: required for the current release, but must have a
  concrete split/removal trigger.

## `nimbus/deno` Delta

| Commit | Subject | Disposition | Removal or upstream trigger | Changelog / proof mapping |
| --- | --- | --- | --- | --- |
| `7530d3c1a1acd3d1aa6ad0a48ed360d021d08084` | `build: pin rusty_v8 to nimbus v149` | Nimbus-only host integration | `n/a` | Release wiring that binds the Deno fork to the matching Nimbus `rusty_v8` locker build. |
| `363de88e0dd6cd87c60704bc8e373dea202817e4` | `runtime: restore nimbus locker lifecycle seam` | Nimbus-only host integration | `n/a` | Restores the embedder lifecycle seam Nimbus needs around the forked locker runtime. |
| `c0d530232406238305a69586769ef62d7d65e4de` | `runtime: harden embedded node vm and zlib` | Temporary carry | Split into upstreamable Node semantics versus Nimbus embedder glue before the next Deno base bump, or mark the upstreamable subset with an upstream issue/PR. | Node compatibility hardening proven by generated Node LTS evidence and NLRT runtime tests. |
| `9225357ba8697cf2c998eef62571779957a7a90c` | `runtime: return sqlite config errors` | Upstream Deno-family | Drop when upstream Deno ships equivalent `node:sqlite` config-error behavior, or keep linked to an upstream issue/PR. | Node compatibility fix for `node:sqlite` error propagation. |
| `37b6333a1f703db523efe8a703d36f2152ad087a` | `runtime: update DNS and TLS security dependencies` | Upstream Deno-family | Drop when upstream Deno contains equivalent dependency updates, or keep linked to the upstream dependency bump. | Security/dependency hygiene for Deno-owned DNS/TLS runtime crates. |

## `nimbus/rusty_v8` Delta

| Commit | Subject | Disposition | Removal or upstream trigger | Changelog / proof mapping |
| --- | --- | --- | --- | --- |
| `665f3f1f5fc0a10a64147dc3dd9318f0deea82c8` | `fix: keep isolate annex alive during teardown (#1978)` | Upstream Deno-family | Drop when upstream `rusty_v8` release contains this teardown fix. | Base weak-handle teardown safety carried into the Nimbus V8 pin. |
| `95d2a5b03afab218f9839310452749ef98de646d` | `Add v8::Locker and v8::UnenteredIsolate` | Temporary carry | Upstream or replace with an upstream-supported locker API before retiring the Nimbus V8 fork. | Adds the embedder locker API required by Nimbus runtime isolation. |
| `27099cb82c1946d8b68f058fddd93c94e665ded1` | `fix(locker): add panic safety and improve documentation` | Temporary carry | Same trigger as the locker API carry. | Hardens the carried locker API. |
| `f9ae6877b958247a78cbe358e0507e705a382a7d` | `feat(locker): add compile-time safety tests and unsafe documentation` | Temporary carry | Same trigger as the locker API carry. | Proves the carried locker API safety contract. |
| `944b24aab899b6bbffc640bac11392877a778cfa` | `fix(locker): correct Enter/Exit ordering in Locker` | Temporary carry | Same trigger as the locker API carry. | Correctness fix for the carried locker API. |
| `65cc3e905ae771d1b644240c479725b11034a5f6` | `fix(locker): initialize HandleScope annex in Locker scope` | Temporary carry | Same trigger as the locker API carry. | Correctness fix for Locker-scope handle lifecycle. |
| `e698466920431bd8248efd64cd37c632aaf10c32` | `fix(locker): reset weak handles during isolate teardown` | Temporary carry | Same trigger as the locker API carry. | Weak-handle teardown safety for the carried locker runtime. |
| `ec9267b6e0fb12047003cf4528902a4076ba1a07` | `fix(locker): clear active weak handles before isolate teardown` | Temporary carry | Same trigger as the locker API carry. | Weak-handle teardown safety for the carried locker runtime. |
| `6a593035f8bc28d0278efec9baed425ab4925874` | `test(locker): harden weak teardown release` | Temporary carry | Same trigger as the locker API carry. | Regression proof for weak-handle teardown safety. |
| `69f42e2b739320fa18fcad975faa768ee67d2d4a` | `test: bless compile_fail stderr for Rust 1.91.0` | Nimbus-only host integration | `n/a` | Toolchain-compatibility blessing for the Nimbus fork build. |
| `2e885b59e8e318801d7f43c1797adc0a6974c83f` | `rename: agentstation -> nimbus` | Nimbus-only host integration | `n/a` | Repository/product naming hygiene for the Nimbus fork. |
| `e1c1895ca0765d5ce14fded80976b0373e79cd6c` | `style: apply rustfmt to nimbus v149 port` | Nimbus-only host integration | `n/a` | Mechanical formatting after the Nimbus port. |
| `9b77553883f1117ab3df62709b8673b803ed721b` | `build: restore nimbus release contract` | Nimbus-only host integration | `n/a` | Release wiring for the published Nimbus `rusty_v8` tag. |

## Release Proof Checklist

Before a Nimbus release can cite these runtime pins, the release proof must
record:

- `bash scripts/verify-deno-fork-provenance.sh` output showing the expected
  `nimbus/deno` and `nimbus/rusty_v8` tag/SHA sources.
- `bash scripts/verify-deno-fork-upstream-policy.sh` output showing this
  operating ledger still contains the current pin and disposition contract.
- The fork-side tests used for any new carried runtime behavior.
- The Nimbus focused tests and generated Node evidence changed by the bump.
- Confirmation that Nimbus is repinned to published tags, not local paths.
