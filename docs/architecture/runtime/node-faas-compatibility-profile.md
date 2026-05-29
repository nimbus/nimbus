# Node FaaS Compatibility Profile

Status: canonical NFRC10 manifest index

The checked-in profile is
[`node-faas-compatibility-profile.json`](node-faas-compatibility-profile.json).
It defines the machine-readable support vocabulary for realistic Node.js
functions-as-a-service and Convex-compatible `"use node"` action support.

Validate it with:

```bash
bash scripts/verify-node-faas-compat-profile.sh
```

## Contract

The profile separates three concerns that must not be blurred in public docs:

- Node release phase, such as Node24 Active LTS or Node26 Current/non-LTS.
- Nimbus support promise, such as supported in-process, local-dev-only,
  service/microVM-required, unsupported, or not applicable to FaaS.
- Runtime engine identity: Node lanes are compatibility contracts on the
  current `v8_deno_core` in-process engine, not claims that Nimbus embeds the
  official Node binary or `libnode`.

Every public support claim in the profile cites a checked-in evidence ref. The
validator rejects unknown support statuses, doc claims without evidence refs,
unknown evidence refs, missing evidence paths, and a disabled wide-then-focused
execution strategy.

## Wide-Then-Focused Strategy

The profile makes the plan's compatibility loop machine-readable:

1. Enable or vendor the broadest relevant corpus.
2. Run the broad group and record the issue inventory.
3. Fix or classify individual failures with isolated tests.
4. Rerun the broad group before closing the row.

Rows that touch corpora, classifications, canaries, package behavior, or docs
claims must write proof fields for the initial broad run, focused fixes or
classifications, and final broad rerun.

## Generated Docs

NFRC10 generates the Deno-style public reference pages from this profile and
the generated evidence snapshots:

- `docs/runtimes/nodejs/reference/node-apis.md`
- `docs/runtimes/nodejs/reference/packages.md`
- `docs/runtimes/nodejs/compatibility.md`

Run `make node-compat-publish-docs CHECK=1` to prove that checked-in public
runtime docs match the current manifest and evidence snapshots.
