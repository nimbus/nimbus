# Networking Canaries

This root holds the checked-in package canaries for the `NLC6` networking
family.

These are not prerelease "canary builds". They are pinned package smoke tests
that run real ecosystem code inside the Neovex Application runtime so the
networking family closeout is backed by package-level evidence instead of only
upstream Node unit fixtures.

Current package set:

- `express`
- `fastify`
- `socket.io`
- `socket.io-client`
- `undici`
- `axios`

Install the pinned dependencies locally:

```bash
make node-compat-canaries-bootstrap PROFILE=application
```

Run the current batched runtime canaries:

```bash
make node-compat-canaries PROFILE=application
```

That command now emits a machine-readable report at:

- `target/node-compat/canaries/profile-application.json`

Current lane mapping:

- Node22 default Application lane:
  `express`, `fastify`, `socket.io`, `undici`, `axios`
- Node20 supported Application lane:
  `express`, `fastify`

Checked-in registry:

- `tests/runtime/node/canary-registry.json`
- `tests/runtime/node/README.md`

The sibling `Tooling` package canaries now live under
`tests/runtime/node/tooling-canaries/`.
