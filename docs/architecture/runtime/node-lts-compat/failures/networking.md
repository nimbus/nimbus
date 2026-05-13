# Networking Failure Inventory

Status: `classified`

This file is the checked-in failure inventory for the currently manifested
`networking` subset.

It records only the explicit red/skip remainder for the current family:
watchpoints, validation-lane divergences, supported-lane drift, later-family
dependencies, and preset/capability restrictions. Requirements and closeout
decisions belong in `docs/plans/archive/node-lts-compatibility-plan.md`.

The pinned `networking` family package canary lanes under
`tests/runtime/node/networking-canaries/` are green and do not add any
unexplained networking failures. Only the explicit remainder below stays out
of the denominator.

## Node22 Upstream Slice Status

- Status: `green for the currently manifested subset`
- Current measured subset:
  - `270` official files passed
  - `0` failed
  - explicit Node22 remainder is classified below

- `node22_networking_dgram_cluster_boundary_batch_watchpoint`
  - classification: `networking_cluster_process_boundary`
  - reason: these files currently stop at `cluster.fork(): no script path
    available in process.argv`, so they are blocked on broader
    cluster/child-process execution semantics rather than plain UDP runtime
    behavior
  - files:
    - `test/parallel/test-dgram-bind-socket-close-before-cluster-reply.js`
    - `test/parallel/test-dgram-cluster-bind-error.js`
    - `test/parallel/test-dgram-cluster-close-during-bind.js`
    - `test/parallel/test-dgram-cluster-close-in-listening.js`
    - `test/parallel/test-dgram-exclusive-implicit-bind.js`
    - `test/parallel/test-dgram-unref-in-cluster.js`
  - owner: cross-family cluster/process boundary
  - evidence:
    `runtime::tests::node_compat::node22_networking_dgram_cluster_boundary_batch_watchpoint`

- `node22_networking_dgram_host_profile_boundary_batch_watchpoint`
  - classification: `networking_host_profile_boundary`
  - reason: these files currently stop at the application-preset external-net
    and IPv6 capability boundary instead of a pure UDP semantic mismatch
  - files:
    - `test/parallel/test-dgram-error-message-address.js`
    - `test/parallel/test-dgram-ipv6only.js`
    - `test/parallel/test-dgram-udp6-link-local-address.js`
    - `test/parallel/test-dgram-udp6-send-default-host.js`
  - owner: host/preset capability boundary
  - evidence:
    `runtime::tests::node_compat::node22_networking_dgram_host_profile_boundary_batch_watchpoint`

- `node22_networking_https_address_boundary_batch_watchpoint`
  - classification: `networking_host_profile_boundary`
  - reason: these files currently stop at explicit local-address / IPv6
    capability boundaries instead of a plain HTTPS semantic mismatch
  - files:
    - `test/parallel/test-https-localaddress-bind-error.js`
    - `test/parallel/test-https-connect-address-family.js`
  - owner: host/preset capability boundary
  - evidence:
    `runtime::tests::node_compat::node22_networking_https_address_boundary_batch_watchpoint`

- `test/parallel/test-dgram-reuseport.js`
  - classification: `networking_dgram_reuseport_watchpoint`
  - reason: `../common/udp` now materializes correctly, so the old
    module-not-found issue is gone; the file now blocks in `reusePort`
    bind/lifecycle behavior and stays explicit until that owner seam is fixed
  - owner: UDP `reusePort` lifecycle semantics
  - evidence:
    `runtime::tests::node_compat::node22_dgram_reuseport_watchpoint`

### Other Explicit Node22 Watchpoints

- `test/parallel/test-http-agent-reuse-drained-socket-only.js`
  - classification: `networking_process_report_exit_watchpoint`
  - reason: the official file no longer narrows to a pure `http.Agent`
    networking seam; it currently blocks in `process.report.getReport()`, and
    when that call is bypassed it proceeds far enough to hit `process.exit(0)`,
    which currently depends on hidden `Deno.exit` behavior that the embedded
    runtime does not expose
  - owner: cross-family boundary between the shared Deno-family
    `internal/process/report` polyfill and Nimbus-owned hidden-Deno
    bootstrap/exit wiring, not the `http.Agent` keepalive or socket-pool path
  - evidence:
    `runtime::tests::node_compat::node22_http_agent_reuse_drained_socket_only_watchpoint`

- `test/parallel/test-https-agent-additional-options.js`
  - classification: `networking_crypto_boundary_watchpoint`
  - reason: the official file now narrows cleanly to the legacy
    `secureProtocol: 'TLSv1_1_method'` / `minVersion: 'TLSv1.1'` path, which
    the current rustls-backed TLS owner layer rejects as `unsupported
    protocol`
  - owner: `networking` family/`loader-context` and crypto-compression family boundary between the current networking-family `https`
    helper contract and the broader legacy TLS protocol / crypto fidelity work
  - evidence:
    `runtime::tests::node_compat::node22_https_agent_additional_options_watchpoint`

## Node20 Validation Slice Status

- Status: `green for the currently manifested validation subset`
- Current measured subset:
  - `265` official `nodejs/node v20.20.2` files passed
  - `0` failed
  - `2` explicit Node20 divergence watchpoints outside the green subset

- `test/parallel/test-https-hwm.js`
  - classification: `node20_supported_divergence`
  - reason: the shared official file now completes on Node22 and Node24 but
    still times out on the current Node20 lane after only the first two
    `readableHighWaterMark` callbacks.
  - owner: current Node20 supported-only HTTPS response highWaterMark drift
  - evidence:
    `runtime::tests::node_compat::node20_https_hwm_watchpoint`

- `test/parallel/test-tls-connect-hwm-option.js`
  - classification: `node20_supported_divergence`
  - reason: the shared official file now completes on Node22 and Node24 but
    still times out on the current Node20 lane in the raw TLS
    `readableHighWaterMark` path.
  - owner: current Node20 supported-only raw TLS highWaterMark drift
  - evidence:
    `runtime::tests::node_compat::node20_tls_connect_hwm_option_watchpoint`

## Node24 Preview Status

- Status: `supported-lane watchpoint; not a green support claim`
- Latest explicit supported-lane watchpoint run:
  - `268` passed
  - `0` failed
  - `1` explicit supported-lane divergence outside the green subset

### Explicit Node24 Preview Divergences

- `test/parallel/test-stream-pipeline.js`
  - classification: `node24_supported_lane_divergence`
  - reason: the staged official Node24 file still expects the inner `"Boom!"`
    pipeline error message in one aborted local-http branch, while the current
    runtime returns the higher-level AbortError-style message
    `"The operation was aborted"`
  - owner: future Node24 supported drift review only; this is not part of the
    supported Node22 / Node20 denominator
  - evidence:
    `runtime::tests::node_compat::node24_stream_pipeline_watchpoint`

The current widened networking batch is also green against the staged official
`nodejs/node v24.15.0` copies after holding out that single supported-lane
divergence, but that remains forward-visibility evidence only. It does not
authorize a Node24 support claim.
