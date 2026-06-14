# Codex Handoff — drive the NDS gate to 0/0 (PR #10)

> Read this whole file once (it is short on purpose), then work. Do **not** read
> the 2000-line test files or run the full suite to "get oriented" — everything
> you need is here.

## Mission

Drive the Nimbus **node-default-runtime-support** gate to a literal **0/0**,
honestly. The gate has two lanes (node22, node24). Each must reach
`v8_isolate_required.gaps == 0` **and** `pass_rate_percent == 100`.

Each remaining "gap" is a **real fork bug or missing feature**. You fix it in the
Nimbus Deno fork (deno_node TypeScript polyfills, deno_core Rust, or deno_crypto),
prove the fixture goes dynamically green, then promote it. There is no shortcut:
you cannot make a fixture "pass" by skipping, weakening an assertion, or editing
the derived posture.

**Current gate (already done): node22 = 19, node24 = 25.** Session cycles 17-77
took it from 81/87 -> 19/25 (published fork tags, reclassifications, promotions,
zero false greens). Your job is to keep going, one fixture at a time.

## THE HONESTY CONTRACT (non-negotiable — a false green is worse than a red gate)

A fixture may leave the `v8_isolate_required` gap set **only** by:
1. **A real dynamic green-guard** — the enforced batch runs the fixture and
   reports `passed≥1, skipped=0, failed=0`. **A skip is NOT a pass.**
2. **A source-confirmed structural reclassification** — you read the fixture
   source and it genuinely needs a host capability the multi-tenant isolate must
   not have (TTY, ambient host network, host process control), is native-syntax /
   V8-version-composition, or is private-internal (`require('internal/*')`).

Never: skip, weaken/delete assertions, or hand-edit
`docs/private/.../node-default-support-posture.json` (it is **derived**, not the
committed input). When unsure, leave it red.

## Where everything is

| What | Value |
| --- | --- |
| Work in this worktree | `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening` |
| Branch (push every cycle) | `codex/node-default-runtime-support-hardening` → PR **#10** |
| Nimbus Deno fork | `/Users/jack/src/github.com/nimbus/deno`, branch `nimbus/v2.8.3`, currently tag `v2.8.3-nimbus.26` |
| rusty_v8 fork | `/Users/jack/src/github.com/nimbus/rusty_v8` (prebuilt; editing its `binding.cc` → from-source V8 build → **OOMs this host** → blocked) |
| Vendored fixtures | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/<lane>/test/parallel/test-*.js` (lanes: node20/22/24/26) |
| Test `mod.rs` (add `include!`s here) | `crates/nimbus-runtime/src/runtime/tests/node/mod.rs` (the `include!("cases/...")` block near the end) |
| Promotion / probe `.rs` files | `crates/nimbus-runtime/src/runtime/tests/node/cases/` |
| Posture (DERIVED, gitignored) | `docs/private/architecture/runtime/node-default-support-posture.json` |
| Remaining-work list (tracked) | `tests/runtime/node/NDS-GATE-BLOCKER.md` |
| Verifier (the gate) | `bash scripts/verify-node-default-runtime-support-hardening.sh` (step 9 is the goal) |
| Python for ALL scripts | `/opt/homebrew/bin/python3.12` (pyenv 3.12.1 has broken blake2b; system 3.9 lacks `zip(strict=)`) |

## ⚡ EFFICIENCY — do NOT burn your context window on tests

This is the single most important section. The test suite is huge; reading it raw
or running it whole will exhaust your context.

- **NEVER** run `make test`, `make ci`, the full `cargo test -p nimbus-runtime`,
  or the verifier's full suite to "see where things stand". The committed posture
  already says where things stand.
- **Census ONE fixture per process** with the env-driven probe (below), wrapped in
  `gtimeout -s KILL 90`, followed by `pkill -9 -f <probe-fn>`. The harness's own
  30s timeout misses some hangs.
- **Always grep-filter cargo output.** Pipe through
  `grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal'`.
  Never let raw cargo/test output into your context.
- **Build incrementally.** Only `cargo clean -p deno_node` (NOT `cargo clean`).
  First build after a clean ≈ 40s, later builds ≈ 10–25s.
- **Read narrowly.** Use `sed -n 'A,Bp'` / `grep -n` to read the *failing
  assertion* and the *one fork function* — never whole files. The probe prints the
  failing line number (`at async <fn> (...:LINE)`); jump straight there.
- **Use the diagnostic artifacts** instead of re-running: batch summaries at
  `target/node-compat/diagnostics/batch/<lane>__<fn>__summary.json`
  (`passed_paths`/`skipped_paths`/`failed_paths`) and per-fixture JSON under
  `target/node-compat/diagnostics/{vm,...}/`.
- **List gaps / read the gate** with a python one-liner, never by reading the 10 MB
  posture JSON into context (commands below).

## How the gate is computed (read once, never re-derive)

- A fixture is **green/supported** iff it is referenced by a **non-`#[ignore]`**
  test fn that calls `run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(`
  with the fixture in a local `let fixture_paths = SOME_CONST.iter()...` var.
  `scripts/runtime/node/classifications.py` **statically scans** for that call and
  expands the path const. It does **NOT** follow helper fns — inline the const.
- The pipeline (`classifications.py` → `status.py` → … → `default_support_posture.py`)
  re-derives the posture from those `.rs` references + the
  `tests/runtime/node/classifications/*.json` catalogs. The static scan does **not**
  check dynamic green — that is YOUR job (run the batch, confirm
  `passed≥1, skipped=0, failed=0`). This is why honesty is enforced by you, not the tool.
- **Committed per cycle:** test `.rs` under `crates/nimbus-runtime/...` +
  `tests/runtime/node/classifications/*.json` + the regenerated **tracked** evidence
  under `tests/runtime/node/compat|published`. The posture/blockers regenerate into
  gitignored `docs/private/` and are **not** committed.

### Useful one-liners (copy verbatim)

```bash
PY=/opt/homebrew/bin/python3.12
POS=docs/private/architecture/runtime/node-default-support-posture.json
# gate numbers:
$PY -c "import json;d=json.load(open('$POS'));print('n22',d['lanes']['node22']['v8_isolate_required']['gaps'],'n24',d['lanes']['node24']['v8_isolate_required']['gaps'])"
# the gap fixture list per lane:
$PY - <<'EOF'
import json
d=json.load(open("docs/private/architecture/runtime/node-default-support-posture.json"))
for l in ("node22","node24"):
    g=sorted(e['test_path'] for e in d['lanes'][l]['entries'] if e.get('support_denominator')=='v8_isolate_required')
    print(f"== {l} ({len(g)}) =="); [print(" ",p) for p in g]
EOF
```

If `docs/private/` is missing (fresh checkout), regenerate it first:
`for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do $PY scripts/runtime/node/$s.py; done`

## The per-fixture cycle (the loop you run)

### 1. Census the gap fixture (find the real failure)

Create a scratch probe once per session (delete before committing — it is `#[ignore]`):

```rust
// crates/nimbus-runtime/src/runtime/tests/node/cases/nds_probe.rs  (scratch; DELETE before commit)
#[test]
#[ignore]
fn nds_probe() {
    let fixture = std::env::var("NIMBUS_RECENSUS_FIXTURE").unwrap();
    let lane = match std::env::var("NIMBUS_RECENSUS_LANE").unwrap_or_else(|_| "node24".into()).as_str() {
        "node22" => NodeCompatLane::Node22, _ => NodeCompatLane::Node24 };
    let dirs_raw = std::env::var("NIMBUS_RECENSUS_EXTRA_DIRS").unwrap_or_else(|_| "test/common".into());
    let dirs: Vec<&str> = dirs_raw.split(':').filter(|s| !s.is_empty()).collect();
    let fixture_paths = vec![fixture];
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs("nds-probe", lane, &fixture_paths, &[], &dirs);
}
```
Add `include!("cases/nds_probe.rs");` to `mod.rs`, then:
```bash
cargo test -p nimbus-runtime --lib nds_probe --no-run 2>&1 | grep -iE 'error\[|Finished'
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-FOO.js" NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|should execute|deep-equal|\+ actual|- expected|at async.*test-FOO'
pkill -9 -f nds_probe
```
Common extra dirs: `test/common`, plus `test/fixtures/es-modules`, `test/fixtures/crypto`, `test/fixtures/keys`, `test/fixtures/webcrypto` as the fixture needs (read its `require('../fixtures/...')`).

**TRAP:** `test result: ok. 1 passed` is the *Rust* test passing (a skip counts as
not-failed). Judge the FIXTURE by the **summary line** `selected=N, passed=P,
skipped=S, failed=F`. Promote only when `passed≥1, skipped=0, failed=0`.

### 2. Decide: fix, reclassify, or skip

- **Behavioral/feature gap** → fix it in the fork (deno_node `ext/node/polyfills/*.{js,ts}`
  is the most common + lowest-risk; deno_core `libs/core` Rust is non-OOM but deeper).
  Multi-assertion fixtures peel one layer at a time — fix the failing assertion, rebuild,
  census, repeat (e.g. `test-vm-module-errors` took 4 composed fixes).
- **Source-confirmed structural** → reclassify in `scripts/runtime/node/default_support_posture.py`
  (add the path to the right frozenset / `HOST_NETWORK_SOCKET_PATHS` /
  `WATCHPOINT_STRUCTURAL_RECLASSIFICATIONS` / the `upstream_or_platform_boundary`
  bucket). No fork build needed. Mirror an existing precedent comment.
- **Genuinely blocked** (see NDS-GATE-BLOCKER.md) → leave it; do not chase.

### 3. Fork-owner flow (when you edited the fork)

```bash
# (a) override Nimbus -> local fork. paths MUST be a top-level key BEFORE any [section],
#     and point at a PACKAGE dir, not the workspace root:
cd <worktree>
{ printf '# TEMP fork override (remove before commit)\npaths = ["/Users/jack/src/github.com/nimbus/deno/ext/node"]\n\n'; cat .cargo/config.toml; } > .cargo/config.toml.new && mv .cargo/config.toml.new .cargo/config.toml
#   (deno_core edits also need ".../deno/libs/core" in the paths array + `cargo clean -p deno_core`)
cargo clean -p deno_node          # REQUIRED for polyfill .js/.ts edits to recompile into the snapshot
# (b) build + census the target via the probe (or a promotion wave) until passed=1/skipped=0/failed=0
# (c) publish the fork:
cd /Users/jack/src/github.com/nimbus/deno
git add ext/node/polyfills/domain.ts ext/node/polyfills/vm.js  # replace with exact edited files
git commit -m "node(...): <what>" && git tag v2.8.3-nimbus.NN && git push origin HEAD && git push origin v2.8.3-nimbus.NN
# (d) repin Nimbus to the published tag + drop the override:
cd <worktree>
git checkout .cargo/config.toml
perl -pi -e 's/v2\.8\.3-nimbus\.16/v2.8.3-nimbus.NN/g' Cargo.toml   # bump PREV->NN
cargo update -p deno_node
# (e) RE-VERIFY on the published tag (rebuild from git, not the local path) — census must stay green
```

### 4. Promote (the wave .rs — inline-const, per the static-scan rule)

```rust
// cases/nds3_cycleNN_wave1.rs
const NDS3_CYCLENN_PATHS: &[&str] = &["test/parallel/test-FOO.js"];
const NDS3_CYCLENN_EXTRA_DIRS: &[&str] = &["test/common"];
#[test]
fn node24_default_lane_executes_cycleNN_batch() {
    let fp = NDS3_CYCLENN_PATHS.iter().copied().map(str::to_string).collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycleNN-batch", NodeCompatLane::Node24, &fp, &[], NDS3_CYCLENN_EXTRA_DIRS);
}
// add a node22 fn too if test-FOO is also a node22 gap
```
`include!` it in `mod.rs`, run it (must show `passed=1, skipped=0, failed=0`), then
**delete the scratch probe** include + file.

### 5. Regression-check, regenerate, commit, push

```bash
# If the fork edit touched shared code, run the REAL green-guards for the affected family —
# NOT an ad-hoc batch. Find the fn name and run it:
grep -rn "fn .*<family>" crates/nimbus-runtime/src/runtime/tests/node/cases/*.rs
cargo test -p nimbus-runtime --lib <fn-substring> 2>&1 | grep -iE 'test result|FAILED'
# regen the pipeline (skip publish_docs.py — it is base-broken on a scrubbed file):
PY=/opt/homebrew/bin/python3.12
$PY scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do $PY scripts/runtime/node/$s.py >/dev/null; done
git diff --stat tests/runtime/node/classifications/      # confirm the promotion moved the catalog
# commit the tracked set + push:
git add Cargo.toml Cargo.lock crates/nimbus-runtime/src/runtime/tests/node/mod.rs \
        crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycleNN_wave1.rs \
        tests/runtime/node/classifications/*.json \
        tests/runtime/node/compat/node-compat-evidence/latest/ tests/runtime/node/published/nodejs/evidence/
git commit -m "nds3(cycle-NN): <fix> (fork .NN; node22 X->Y, node24 X->Y)"
git push origin codex/node-default-runtime-support-hardening
# verify the .cargo override is gone before committing: grep -c 'paths =' .cargo/config.toml  => 0
```

## Traps that each cost hours to learn

1. **Probe skip ≠ pass** (see §1). Judge by the batch summary, not the Rust result.
2. **Ad-hoc regression batches FALSE-FAIL** nimbus-internal fixtures. E.g.
   `test-vm-context-regression-*` live at `node_compat_fixtures/regression/nodeNN/parallel/`
   and are path-remapped in `cases/loader_context_foundation.rs`; a generic
   `test/parallel/...` batch can't find them. **Confirm "regressions" against the
   REAL green-guard fn** (`cargo test ... <fn-substring>`), never an ad-hoc batch.
3. **`paths` override** must be a top-level key (before any `[section]`) and point
   at a package dir (`ext/node`), not the deno workspace root (a virtual manifest).
   The "path override altered dependencies" warning is benign; the build still works.
4. **Polyfill edits need `cargo clean -p deno_node`** (or `-p deno_core` / `-p deno_web`)
   or the change won't enter the snapshot — you'll chase a phantom.
5. **`docs/private/` is gitignored** (owner directive). Commit only test source +
   classifications + evidence. Never `git add docs/private`.
6. **Do NOT merge `origin/main`** — it was history-rewritten (a ~900k-line scrub
   divergence). The owner reconciles the base. New main commits so far are gate-irrelevant.
7. **`/opt/homebrew/bin/python3.12`** for every NDS script. Not `python3`, not pyenv.
8. **Editing deno_core Rust is non-OOM** (V8 is prebuilt). Editing **rusty_v8**
   (a new V8 binding) forces a from-source V8 build that **OOMs this 32 GB host** → blocked.

## What's left (read `tests/runtime/node/NDS-GATE-BLOCKER.md` for the full per-fixture list)

- **5 genuinely blocked — DO NOT chase:** `test-vm-module-hastoplevelawait` (needs a
  rusty_v8 binding → OOM); `test-vm-module-import-meta` (deno_core `bindings.rs:1104`
  panic + needs cross-boundary `initializeImportMeta` wiring); `test-webcrypto-sign-verify-eddsa`
  (Ed448), `test-webcrypto-keygen-kmac`, `test-webcrypto-sign-verify-kmac` (KMAC) —
  native crypto primitives possibly absent from aws-lc.
- **26 unique required fixtures remain, with 5 genuinely blocked and the rest
  tractable-but-deep** by category (owner = `nimbus/deno` unless noted):
  crypto-provider (mostly native — verify primitives before committing a build),
  esm-loader (deno_core), and one remaining promise-hooks fixture (deno_core).
- **Suggested order:** crypto primitive feasibility/probes, then ESM loader, then
  promise-hooks. Cycle71 promoted the Node24
  WebCrypto promise-prototype-pollution fixture; a same-cluster
  `test-webcrypto-deduplicate-usages.js` probe peeled to KMAC/native-provider
  support and stayed red. Cycle72 promoted ESM CJS named export errors; cycle73
  source-confirmed `test-performance-many-marks.js` as an isolate watchdog
  fairness boundary; cycle74 source-confirmed `test-v8-serialize-leak.js` as a
  host-process RSS/GC diagnostic. Cycle77 promoted
  `test-promise-swallowed-event.js` by preserving duplicate promise settle
  callbacks through deno_core and emitting Node's deprecated
  `process` `multipleResolves` event from the node polyfill.

## Verify (the goal)

`bash scripts/verify-node-default-runtime-support-hardening.sh` — **step 9** passes
iff both lanes have `v8_isolate_required.gaps == 0` and `pass_rate_percent == 100`.
The many other failing steps are proof-doc checks reading gitignored `docs/private/`
and are not your target; step 9 is. Stop when step 9 is green, or when only the
genuinely-blocked subset remains (then update NDS-GATE-BLOCKER.md and stop).
