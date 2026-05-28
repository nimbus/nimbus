# NLRT9 Deno Fork Upstream Policy

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Define the Deno-family fork workflow as an operating contract, record the
current Deno and `rusty_v8` carried patches with upstream/Nimbus-only/temporary
dispositions, and add a verifier so future releases cannot skip tag/SHA,
repin, or changelog proof.

## Files Changed

- `docs/operating/deno-fork-workflow.md`
- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/architecture/runtime/deno-vs-neovex-node-compat.md`
- `docs/README.md`
- `scripts/verify-deno-fork-upstream-policy.sh`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt9-deno-fork-upstream-policy.md`

## Local Fork Evidence

Current `nimbus/deno` delta over upstream `v2.8.0`:

```text
7530d3c1a1acd3d1aa6ad0a48ed360d021d08084 build: pin rusty_v8 to nimbus v149
363de88e0dd6cd87c60704bc8e373dea202817e4 runtime: restore nimbus locker lifecycle seam
c0d530232406238305a69586769ef62d7d65e4de runtime: harden embedded node vm and zlib
9225357ba8697cf2c998eef62571779957a7a90c runtime: return sqlite config errors
37b6333a1f703db523efe8a703d36f2152ad087a runtime: update DNS and TLS security dependencies
```

Current `nimbus/rusty_v8` delta over upstream `v149.0.0`:

```text
665f3f1f5fc0a10a64147dc3dd9318f0deea82c8 fix: keep isolate annex alive during teardown (#1978)
95d2a5b03afab218f9839310452749ef98de646d Add v8::Locker and v8::UnenteredIsolate
27099cb82c1946d8b68f058fddd93c94e665ded1 fix(locker): add panic safety and improve documentation
f9ae6877b958247a78cbe358e0507e705a382a7d feat(locker): add compile-time safety tests and unsafe documentation
944b24aab899b6bbffc640bac11392877a778cfa fix(locker): correct Enter/Exit ordering in Locker
65cc3e905ae771d1b644240c479725b11034a5f6 fix(locker): initialize HandleScope annex in Locker scope
e698466920431bd8248efd64cd37c632aaf10c32 fix(locker): reset weak handles during isolate teardown
ec9267b6e0fb12047003cf4528902a4076ba1a07 fix(locker): clear active weak handles before isolate teardown
6a593035f8bc28d0278efec9baed425ab4925874 test(locker): harden weak teardown release
69f42e2b739320fa18fcad975faa768ee67d2d4a test: bless compile_fail stderr for Rust 1.91.0
2e885b59e8e318801d7f43c1797adc0a6974c83f rename: agentstation -> nimbus
e1c1895ca0765d5ce14fded80976b0373e79cd6c style: apply rustfmt to nimbus v149 port
9b77553883f1117ab3df62709b8673b803ed721b build: restore nimbus release contract
```

Both canonical local fork worktrees were clean when inspected.

## Decisions

- Added `docs/operating/deno-fork-workflow.md` as the durable workflow instead
  of burying the process in the plan. It explicitly requires classify,
  unpin-to-local-fork, prove, commit/tag/push, repin, rerun verification, and
  record release proof.
- Added `docs/architecture/runtime/deno-fork-bump-ledger.md` as the current
  carried-patch ledger. It records the active `nimbus/deno` and
  `nimbus/rusty_v8` tags, commit SHAs resolved by Cargo, upstream bases,
  patch disposition, removal/upstream triggers, and changelog/proof mapping.
- Classified the Deno VM/zlib hardening patch and the `rusty_v8` locker API
  series as temporary carries because they need a future split/upstream trigger
  before Nimbus can claim they are permanent clean deltas.
- Added `scripts/verify-deno-fork-upstream-policy.sh` so the NLRT closeout
  verifier can check that the operating contract and current pin ledger still
  exist.

## Verification

```text
bash scripts/verify-deno-fork-upstream-policy.sh
Summary: 27 passed, 0 failed
```

```text
bash scripts/verify-deno-fork-provenance.sh
Summary: 5 passed, 0 failed
Runtime Deno-family classification: 40 forked, 15 allowlisted
```

```text
git -C /Users/jack/src/github.com/nimbus/deno status --short
<no output>
```

```text
git -C /Users/jack/src/github.com/nimbus/rusty_v8 status --short
<no output>
```

## Remaining Risks

- NLRT11 still needs to call `scripts/verify-deno-fork-upstream-policy.sh` from
  the final `scripts/verify-node-lts-runtime-trust.sh` verifier.
- Temporary carries now have explicit split/upstream triggers, but they still
  require future upstream issue/PR links when the next Deno or `rusty_v8` base
  bump begins.
