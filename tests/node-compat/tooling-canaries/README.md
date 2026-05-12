# Tooling Package Canaries

This root holds the pinned `RuntimeProfile::Tooling` package canaries used by
`NLC10` closeout evidence.

The current canary set covers:

- `tsx`
- `ts-node`
- `jest`
- `prisma`
- `next`

All package versions are pinned in [package.json](/Users/jack/src/github.com/agentstation/neovex/tests/node-compat/tooling-canaries/package.json)
and installed through the canonical repo-owned bootstrap command:

```bash
make node-compat-canaries-bootstrap PROFILE=tooling
```

