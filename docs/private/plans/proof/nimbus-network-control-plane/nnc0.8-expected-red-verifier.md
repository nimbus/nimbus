# NNC0.8 Expected-Red Verifier Proof

Date: 2026-07-23

Source commit before the item: `94f1e2212`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Scope

NNC0.8 adds
`scripts/verify-nimbus-network-control-plane.sh` as an aggregate, named-condition
architecture gate. It does not change production Rust behavior or dependencies.
The verifier is expected red until later extraction bands establish the
`nimbus-network` crate and remove the current duplicate allocation and portable
vocabulary authorities.

The plan routing status in `docs/private/plans/README.md` was also reconciled
from its stale NNC0.2 checkpoint to the active NNC0.8 checkpoint.

## Normal Expected-Red Run

Command:

```text
set +e
scripts/verify-nimbus-network-control-plane.sh > /tmp/nnc0.8-verifier.out 2>&1
verifier_status=$?
set -e
cat /tmp/nnc0.8-verifier.out
test "${verifier_status}" -eq 1
test "$(rg -c '^PASS NNCV' /tmp/nnc0.8-verifier.out)" -eq 8
test "$(rg -c '^FAIL NNCV' /tmp/nnc0.8-verifier.out)" -eq 4
```

Result: exited `0`, proving that the verifier itself exited exactly `1`, with
eight passing and four failing conditions.

Passing conditions:

- `NNCV000 required-tools`
- `NNCV001 plan-in-HEAD`
- `NNCV002 required-baseline-inputs`
- `NNCV006 unclassified-production-bind`
- `NNCV007 dependency-profiles-acyclic`
- `NNCV008 checkpoint-ledger-recoverable`
- `NNCV009 sole-plan-routing-owner`
- `NNCV010 core-runtime-foundation-invariants`

The four intended pre-extraction failures:

- `NNCV003 nimbus-network-crate`: the crate manifest is not present yet.
- `NNCV004 network-dependency-contract`: the dependency contract cannot be
  inspected while the crate is absent.
- `NNCV005 no-duplicate-port-allocation-authority`: the verifier names the
  sandbox `PortManager`, CLI dev probe/drop helpers, and machine SSH allocation
  helpers that NNC3 must replace with the shared authority.
- `NNCV011 single-portable-vocabulary-owner`: the verifier names
  `NetworkSegment` in core plus the allocator and published-endpoint vocabulary
  still owned by sandbox.

There were no unexpected red conditions. In particular, the live production
source census matched the 24-site baseline inventory and found zero
unclassified TCP/UDP bind or allocation authorities.

## Verifier Negative Self-Tests

Command:

```text
scripts/verify-nimbus-network-control-plane.sh --self-test
```

Result: exited `0`; `7 passed, 0 failed`.

The child-process self-tests prove:

1. a missing plan fails only the `NNCV001 plan-in-HEAD` assertion for that
   condition and cannot report it as pass;
2. a missing bind inventory fails `NNCV002 required-baseline-inputs`;
3. a missing dependency artifact also fails
   `NNCV002 required-baseline-inputs`; and
4. an injected production `TcpListener::bind` site fails
   `NNCV006 unclassified-production-bind`;
5. a production bind after a test-only item still fails NNCV006, proving the
   scanner does not truncate a file at its first `#[cfg(test)]`;
6. a bind wholly inside a `#[cfg(test)]` module remains a legitimate test-only
   exemption; and
7. a missing `nimbus-core` source root fails
   `NNCV010 core-runtime-foundation-invariants` rather than converting a
   ripgrep error into an empty successful scan.

Every child is expected to remain nonzero for the ordinary pre-extraction
conditions. The self-test asserts the named target condition has an exclusive
FAIL rather than treating the overall nonzero exit as sufficient proof.

## Static and Repository Checks

Commands and results:

```text
bash -n scripts/verify-nimbus-network-control-plane.sh
# exit 0

shellcheck -s bash scripts/verify-nimbus-network-control-plane.sh
# exit 0, no findings

git diff --check
# exit 0

npm --prefix website run build
# exit 0; 109 pages built

bash scripts/check-docs.sh
# exit 0; 108 pages link-clean, source map resolves, private fence intact,
# titles unique

bash scripts/verify-nimbus-docs-site.sh
# exit 0; 17/17 conditions green
```

The script is compatible with the repository's Bash 3.2 baseline. Its source
scans distinguish “no match” from an unreadable input, and required JSON inputs
must exist, be nonempty, parse, satisfy their shape/count invariants, and name a
source commit that is an ancestor of `HEAD`.

## Independent Review Disposition

The first Claude Opus 4.8 maximum-reasoning review found two P2 fail-open
risks:

- NNCV010 discarded the core-source ripgrep exit status; and
- NNCV006 truncated every source file at its first `#[cfg(test)]`.

Both findings were accepted. NNCV010 now distinguishes no-match from scan
failure. NNCV006 masks comments/literals, removes only the attributed
test-only Rust item or module using balanced braces, and continues scanning
later production items. The three additional negative tests above pin the
corrected behaviors.

The post-fix Claude Opus 4.8 maximum-reasoning review reported no accepted or
actionable findings and rated the patch correct (`0.8`). It independently
traced both regression guards, confirmed the intended eight-pass/four-fail
normal result, checked Bash 3.2 and exact temporary cleanup behavior, and
confirmed the plan/index/ledger checkpoint is internally consistent.

## Proof Obligations Established

- The canonical plan must exist in branch `HEAD`; a staged or ignored-only
  working-tree copy fails.
- `nimbus-network` cannot silently acquire an upper-layer workspace edge. Once
  the crate exists, its workspace dependency set must be exactly
  `nimbus-core`; `nimbus-testing` is explicitly rejected.
- The dependency profiles must remain distinct and acyclic.
- The task-band and checkpoint-ledger item sets must remain a duplicate-free
  bijection with exactly one recoverable `in_progress` row.
- The routing index must contain exactly one canonical plan entry and its
  normalized status must match the plan.
- `nimbus-core` zero-I/O and `nimbus-runtime` zero-workspace-dependency
  invariants remain guarded.
- New production bind/allocation sites cannot enter silently, including when
  located in a file that is not test-only.

NNC1.6 and NNC9.1 own the planned expansion from this executable scaffold to
all 24 final static-verifier conditions.
