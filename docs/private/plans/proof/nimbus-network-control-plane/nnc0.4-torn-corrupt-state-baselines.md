# NNC0.4 Torn/Corrupt Segment And IPAM State Baselines

Status: `expected-red predicates reproduced; fail-closed positive control green`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `973db83d2`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile, local temporary
filesystem

## Result

Both current authorities use an exclusive filesystem lock followed by direct
`fs::write` replacement of unversioned, unchecked JSON:

- node-global segment ownership:
  `<state_root>/networks/segments.json`; and
- tenant-local IPAM ownership:
  `<tenant_root>/networks/run/ipam-state.json`.

Four tests separate syntax rejection from integrity enforcement:

1. a syntactically torn IPAM record is rejected at the intended parse boundary
   and its error names the exact authority path (green positive control);
2. the segment parser rejects the same torn record, but its error omits the
   authority path (expected red);
3. valid-looking segment JSON with the committed tenant removed is accepted,
   and a replacement tenant receives the original live `10.0.0.0/24`
   (expected red); and
4. valid-looking IPAM JSON with the committed sandbox removed is accepted, and
   a replacement sandbox receives the original live `10.89.0.2`
   (expected red).

The latter two cases prove that serde validity is not integrity. A checksum or
version envelope must reject loss of authoritative ownership before allocation
logic runs.

## Exact failure predicates

`torn_segment_state_error_must_name_the_authority_path` first proves the
operation reached the current segment parser, then fails only because the
rendered error does not contain the concrete `segments.json` path:

```text
a corruption diagnostic must name the affected authority path:
sandbox operation failed: failed to parse network segment state:
EOF while parsing an object at line 1 column 1
```

The two semantic-corruption tests first assert the unchecked allocator
reissued the original address, then fail only at their safe terminal
invariants:

```text
semantically valid corruption must fail closed instead of reissuing a live segment

semantically valid corruption must fail closed instead of reissuing a live IP
```

All three tests are ignored in ordinary discovery. NNC2.1 owns the atomic,
versioned, checksummed store, the pass-after behavior, and removal or deliberate
migration of these ignore markers.

## Commands and results

The fail-before commands each exited `101` at the named terminal assertion:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::torn_segment_state_error_must_name_the_authority_path \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; generic parse error lacked the segments.json path.

timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::semantically_valid_segment_state_corruption_must_not_reissue_a_live_segment \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; precondition proved the original live CIDR was reissued.

timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::ipam::tests::semantically_valid_ipam_state_corruption_must_not_reissue_a_live_ip \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; precondition proved the original live IP was reissued.
```

The positive control, ordinary focused suites, and static gates remained
green:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::ipam::tests::torn_ipam_state_fails_closed_with_the_authority_path \
  -- --exact --nocapture
# 1 passed; 0 failed.

timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::
# 11 passed; 0 failed; 2 expected-red ignored.

timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::ipam::tests::
# 1 passed; 0 failed; 1 expected-red ignored.

timeout 300 cargo clippy -p nimbus-sandbox --all-targets -- -D warnings
# Exit 0; no warning from nimbus-sandbox. Existing vendored Brotli warnings
# remain outside the changed crate.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
# All exit 0.
```

NNC0.4 uses the success criterion's deterministic truncation/corruption path;
it does not claim a subprocess crash cut. NNC2.1 must use the NNC0.1b
exact-boundary subprocess harness to prove atomic replace, sync ordering,
restart, and retained committed authority in the network-owned store. No
random seed, sleep, provider privilege, KVM, cloud service, cross-target, or
sovereignty-denial lane applies here.

## Independent closeout review

The test-only diff was reviewed with the repository autoreview skill and
independent Claude Opus 4.8 at maximum reasoning, with the NNC0.4/NNC2.1 phase
boundary supplied as context. The review exited `0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.6)
```

The reviewer verified that each ignored test asserts the safe final invariant,
the current defect cannot become a green invariant, the pre-final assertions
pin the named parser/allocation behavior, the IPAM truncation case is a
load-bearing green positive control, temporary-root lifetimes are retained,
and the tests introduce no second persistence path or timing dependency.
