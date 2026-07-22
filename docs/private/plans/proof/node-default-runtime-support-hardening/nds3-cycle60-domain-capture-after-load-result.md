# NDS3 cycle 60 - domain capture callback after load ordering

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixture was dynamically promoted for node22 and node24:

- `test/parallel/test-domain-set-uncaught-exception-capture-after-load.js`

Gate movement from the generated private posture:

- node22: 39 -> 38 gaps, 98.39% pass rate
- node24: 46 -> 45 gaps, 98.13% pass rate
- unique remaining required fixtures: 47

Deno fork tag: `v2.8.3-nimbus.14` (`c99b5eb5d4`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

Node rejects public `process.setUncaughtExceptionCaptureCallback()` after the
`domain` module has been loaded, because `domain` and a user capture callback are
mutually exclusive. Deno's Node polyfills already enforced the opposite ordering
(`setUncaughtExceptionCaptureCallback()` first, then `require("domain")`) but
missed the reverse ordering.

The fixture also asserts that the later error stack carries Node's domain-load
breadcrumb, including a forty-dash separator and the original caller frame
(`foobar`). The fork fix records the `require("domain")` stack on the process
object when the domain module loads, then appends that stack to
`ERR_DOMAIN_CANNOT_SET_UNCAUGHT_EXCEPTION_CAPTURE` when a user later attempts to
install a capture callback.

`domain.ts` internally installs its own domain uncaught-exception handler through
the same process API, so the fix adds a private internal-update sentinel around
that path. The public ordering guard remains active for user calls while the
domain module's own handler can still be installed.

The fix stays in `deno_node` TypeScript/JavaScript polyfills and does not touch
V8/rusty_v8.

## Verification

Local-fork focused proof before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-set-uncaught-exception-capture-after-load.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-set-uncaught-exception-capture-after-load.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 852 filtered out; finished in 2.16s
```

Local-fork regression guard after the internal domain-update sentinel:

```bash
cargo test -p nimbus-runtime --lib nds3_domain_fork_promoted_batch_fixture -- --nocapture
# node_compat node24-default-lane-executes-nds3-domain-fork-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node22-supported-lane-executes-nds3-domain-fork-promoted-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 851 filtered out; finished in 3.94s
```

Deno fork publish:

```bash
git add ext/node/polyfills/01_require.js ext/node/polyfills/domain.ts ext/node/polyfills/process.ts
git commit -m "node(domain): enforce capture callback ordering after load"
# [nimbus/v2.8.3 c99b5eb5d4] node(domain): enforce capture callback ordering after load
#  3 files changed, 43 insertions(+), 3 deletions(-)

git tag v2.8.3-nimbus.14
git push origin nimbus/v2.8.3
# a470e7d569..c99b5eb5d4  nimbus/v2.8.3 -> nimbus/v2.8.3

git push origin v2.8.3-nimbus.14
# * [new tag]               v2.8.3-nimbus.14 -> v2.8.3-nimbus.14
```

Immutable-tag preparation:

```bash
cargo update -p deno_node
# Locking 40 packages to latest compatible versions
# deno_node v0.189.0: v2.8.3-nimbus.13#a470e7d5 -> v2.8.3-nimbus.14#c99b5eb5

cargo clean -p deno_node
# Removed 541 files, 361.3MiB total
```

Immutable-tag focused probes after removing the local path override and
repinning Nimbus to `v2.8.3-nimbus.14`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-set-uncaught-exception-capture-after-load.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 852 filtered out; finished in 2.01s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-set-uncaught-exception-capture-after-load.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 852 filtered out; finished in 1.93s
```

Immutable-tag regression guard:

```bash
cargo test -p nimbus-runtime --lib nds3_domain_fork_promoted_batch_fixture -- --nocapture
# node_compat node22-supported-lane-executes-nds3-domain-fork-promoted-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-nds3-domain-fork-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 851 filtered out; finished in 3.87s
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle60_domain_capture_after_load_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle60-domain-capture-after-load-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle60-domain-capture-after-load-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 852 filtered out; finished in 3.90s
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated private posture counts:

```text
node22 38 98.39
node24 45 98.13
unique remaining required fixtures: 47
```

The checked-in public evidence summaries also moved exactly one manifested
fixture per lane:

```text
node22 documented_manifested_green_count: 2327 -> 2328
node24 documented_manifested_green_count: 2357 -> 2358
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=38 / node24=45, not 0/0.
```
