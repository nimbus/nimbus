---
title: Node compatibility
description: Supported Node versions, what supported means, and the evidence behind each claim.
sidebar:
  order: 1
---

Nimbus's Node.js runtime compatibility is evidence-backed and deliberately
bounded: a surface is documented as supported only when it has checked-in
fixture, canary, oracle, or classification evidence. This page is the
contract — the supported Node versions, the vocabulary behind every support
claim, and the headline numbers.

## Supported Node versions

| Target | Status | Release phase | Default | LTS support | Upstream line |
| --- | --- | --- | --- | --- | --- |
| Node20 | Supported for local development only | EOL legacy | no | no | `v20.20.2` |
| Node22 | Supported in process | Maintenance LTS | no | yes | `v22.23.2` |
| Node24 | Supported in process | Active LTS | yes | yes | `v24.20.0` |
| Node26 | Supported in process | Current non-LTS | no | no | `v26.8.1` |

- Node24 is the default compatibility target. The default is a routing
  default, not an evidence priority.
- Node22 and Node24 are the supported LTS targets, each with its own
  per-version evidence.
- Node20 remains selectable as a legacy-grace target after its 2026-04-30
  end of life, but it is not an active LTS support target.
- Node26 is selectable as a Current/non-LTS compatibility target. It becomes
  an LTS support target only after Node itself enters LTS and the same
  evidence gates pass.

## What "supported" means

| Status | Meaning |
| --- | --- |
| Supported in process | Passes production in-process application canaries on the supported LTS versions and may back public support claims. |
| Supported for local development only | Useful for local tooling or development but excluded from production in-process support claims. |
| Service/microVM required | Supported only when explicitly routed to a service or microVM profile with host authority. |
| Import-compatible stub | Module import may succeed for package probing, but runtime use throws a clear Nimbus diagnostic. |
| Unsupported | Rejected with a documented diagnostic and no support claim. |
| Not applicable to functions | CLI, interactive, daemon, or process-wide behavior that does not map to function invocation. |

## The contract

- Convex-compatible `"use node"` action modules can select Node20, Node22,
  Node24, or Node26 through `convex.json` — see
  [configuration](/developers/runtimes/nodejs/configuration/).
- Selecting a Node version does not grant host access. Runtime permission
  mode and explicit grants are a separate axis from the compatibility
  target.
- Nimbus does not claim full Node built-in compatibility for any version.
- Runtime support is narrower than Node CLI parity: `node --test`, the
  inspector, worker threads, child processes, native addons, and other
  host-heavy behavior are service/microVM-routed unless the
  [Node API reference](/reference/runtimes/node-apis/) says otherwise.

## Evidence summary

As of 2026-09-01, from the checked-in evidence snapshots. Fixtures are
official upstream Node test files executed against the Nimbus runtime;
canaries are real packages and application scenarios exercised end-to-end.

### Official fixture results

| Target | Upstream | Fixtures | Passed | Expected failure / known gap | Skipped / excluded | Pass rate | Classified coverage |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | `v20.20.2` | 4248 | 919 | 3316 | 13 | 21.6% | 100.0% |
| Node22 | `v22.23.2` | 4762 | 2362 | 2380 | 20 | 49.6% | 100.0% |
| Node24 | `v24.20.0` | 5671 | 2397 | 3226 | 48 | 42.3% | 100.0% |
| Node26 | `v26.8.1` | 5940 | 2090 | 3795 | 55 | 35.2% | 100.0% |

How to read these numbers:

- The fixture corpora are the vendored official upstream Node test suites.
  Much of each suite exercises CLI, daemon, and process-wide behavior that
  is intentionally out of scope for in-process function invocation, so the
  pass rate is not a percentage of the supported surface.
- Every non-passing fixture is explicitly classified as an expected
  failure, a known gap, or a skip — 100% classified coverage means no
  fixture result is unaccounted for. Classifications are recorded
  boundaries, not support claims.
- Node26 is an early Current-line target: its corpus is vendored and fully
  classified, and its support claims currently rest on canary evidence
  rather than fixture passes.

### Canary results

- 79 package and framework canary claims across 101 checks, all passing on
  their required versions.
- The 79 claims include 68 positive-support claims and 11 diagnostic claims.
  The diagnostic claims prove that host-heavy denials and service routes
  produce the intended errors.
- 0 required canary gaps.

The per-package breakdown is in the
[Node package reference](/reference/runtimes/packages/), and the API-family
breakdown is in the [Node API reference](/reference/runtimes/node-apis/).

## Related pages

- [Node.js runtime overview](/developers/runtimes/nodejs/) — using the Node
  runtime in your functions.
- [How the Node runtime works](/concepts/nodejs-runtime/) — the model behind
  the contract.
