# Node LTS Lane Registry

Status: canonical lane registry index

The checked-in registry is
[`node-lts-lanes.json`](node-lts-lanes.json). It is the source of truth for
Node major support phase, product default, upstream release line, fixture corpus
binding, and evidence policy.

Validate it with:

```bash
bash scripts/verify-node-lts-lanes.sh
```

## Contract

Do not copy lane facts into hand-written architecture prose. Read
`node-lts-lanes.json` for the current lane list, support phase, upstream tag,
fixture corpus tag, LTS dates, and evidence policy.

The product default is a routing default, not an evidence priority. Supported
LTS lanes must have lane-local evidence before public support claims use that
lane. EOL lanes may stay in the harness as legacy-grace regression coverage,
but they are not active enterprise LTS targets.

## Owners

- `nimbus-runtime` owns the registry, runtime target metadata, process metadata,
  runtime limits presets, and node-compat evidence.
- `nimbus-tenant` consumes the registry for tenant/operator runtime profile and
  production admission policy.
- `nimbus-bridge` consumes the selected lane at execution admission and fallback
  routing boundaries.
- `nimbus-convex` consumes the registry for Convex-compatible manifest runtime
  selection and `"use node"` action packaging.

## Upstream Source

The registry is rechecked against official Node release sources when edited:

- Node release schedule JSON:
  <https://raw.githubusercontent.com/nodejs/Release/main/schedule.json>
- Node releases page: <https://nodejs.org/en/about/releases/>
- Node EOL page: <https://nodejs.org/en/about/eol>
- Node download page: <https://nodejs.org/en/download>
