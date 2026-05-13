# Tooling Package Canaries

This root holds the pinned `RuntimePreset::Tooling` package canaries used by
tooling evidence.

The current canary set covers:

- `tsx`
- `ts-node`
- `jest`
- `prisma`
- `next`

All package versions are pinned in [package.json](/Users/jack/src/github.com/nimbus/nimbus/tests/runtime/node/tooling-canaries/package.json)
and installed through the canonical repo-owned bootstrap command:

```bash
make node-compat-canaries-bootstrap PRESET=tooling
```
