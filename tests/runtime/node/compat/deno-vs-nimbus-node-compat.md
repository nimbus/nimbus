# Deno vs Nimbus: Node.js Compatibility Comparison

Status: qualitative architecture note

Nimbus builds on Deno's `ext/node` stack, so the baseline implementation is
shared. This document explains the architectural difference between stock Deno
and Nimbus; it is not the support matrix and it must not carry hand-maintained
pass rates.

## Source Of Truth

Use generated evidence for current Nimbus support claims:

- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/runtimes/nodejs/evidence/node22.md`
- `docs/runtimes/nodejs/evidence/node24.md`
- `docs/private/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/private/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md`

Use the lane registry for release and support posture:

- `docs/private/architecture/runtime/node-lts-compat/node-lts-lanes.json`
- `docs/private/architecture/runtime/node-lts-compat/node-lts-lanes.md`

Use the Deno-family fork operating contract for runtime-fork bump proof:

- `docs/operating/deno-fork-workflow.md`
- `docs/private/architecture/runtime/deno-fork-bump-ledger.md`

Product default is a routing default, not an evidence priority. Node24 is the
current default, Node22 and Node24 are the supported LTS lanes, and Node20
remains selectable only as legacy-grace regression coverage after its
2026-04-30 EOL.

## Comparison Summary

Nimbus's advantage over stock Deno is verification and product boundary, not a
claim that Nimbus has a separate Node implementation:

- Nimbus uses lane-owned upstream Node fixture corpora and checked-in
  classification catalogs.
- Nimbus publishes generated support evidence instead of hand-maintained
  per-module pass rates.
- Nimbus separates JavaScript compatibility target from runtime permissions.
  Node target selection does not grant filesystem, network, subprocess,
  worker, inspector, FFI, secret, service, or environment authority.
- Nimbus keeps host-heavy behavior bounded by explicit runtime grants and
  manifest evidence.
- Nimbus records expected failures, known gaps, skips, and watchpoints as
  non-support claims rather than hiding them inside aggregate totals.

## Deno-Derived Baseline

The shared Deno substrate remains a strength: Node built-in loading,
CommonJS/ESM interop, many standard built-ins, and a large amount of Node
process behavior come from Deno's maintained implementation. Nimbus should
prefer upstreaming general Node semantic fixes into the Deno-family fork when
the behavior is not Nimbus-specific host integration.

## Nimbus-Owned Boundaries

Nimbus-owned behavior is primarily around product trust:

- compatibility target admission and lane metadata
- runtime permission profiles and grant enforcement
- Convex-compatible `"use node"` routing
- path, environment, network, subprocess, and worker boundaries
- generated fixture, canary, oracle, and dashboard evidence
- explicit classification of unsupported or host-heavy behavior

## Claim Boundary

This document is intentionally prose-only. Any numeric support claim, lane role,
or current pass/failure total belongs in generated evidence or the lane
registry. If this document disagrees with generated evidence, the generated
evidence wins and this document should be corrected.
