# SUC4.3 — Egress HTTPS CONNECT Method/Path Rule

## Finding Status: Already Fixed On Main — Verified, Now Pinned

The July-21 review HIGH finding (an HTTPS rule with method/path predicates
denies all CONNECT requests, making HTTPS through the proxy unusable) was
fixed by PR #231 ("Close full codebase review findings"), which landed both
required halves:

1. **Gate deferral** — `CompiledEgressPolicy::authorize_connect`
   (`crates/nimbus-egress/src/policy.rs`) authorizes the CONNECT authority
   phase with L7 method/path checks skipped; the proxy's CONNECT parse
   (`crates/nimbus-proxy/src/request.rs`) produces an authority-only request
   and `worker.rs` routes `ConnectTunnel` through `authorize_connect`.
2. **Forced interception** — `EgressRule::requires_connect_interception`
   returns true for rules carrying methods/path_prefixes (or
   credential/DLP), `connect_requires_interception` feeds
   `classify_connect`, so such tunnels are TLS-intercepted rather than
   spliced, and the decrypted inner request is re-authorized with full L7
   (`https_intercept.rs`). Without this half, relaxing the gate would have
   traded a false deny for an unenforced opaque tunnel.

## Gap Closed Here: Regression Coverage

No test covered the CONNECT shape (the string CONNECT / a method-less
`EgressRequest` against an L7 rule appeared nowhere in `nimbus-egress`
tests; the pre-existing method/path test uses an HTTPS request carrying a
path — a shape the proxy cannot produce for HTTPS). Added to
`crates/nimbus-egress/src/policy.rs`:

- `connect_gate_defers_method_path_and_forces_interception` — full contract:
  full-L7 authorize denies the method-less request; `authorize_connect`
  allows it; `connect_requires_interception` is true; the inner request is
  still enforced (POST /v1/ allowed, GET /other denied).
- `connect_gate_without_l7_predicates_splices` — a predicate-free rule does
  not force interception (splice remains correct).

## Verification

`cargo nextest run -p nimbus-egress`: 32 passed, 0 skipped (includes the two
new tests). `cargo clippy -p nimbus-egress --all-targets -- -D warnings`
clean; `cargo fmt` clean.
