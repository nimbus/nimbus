# Node.js Runtime

See [Node.js Runtime](README.md) for the runtime support contract.

For Convex-compatible projects, run codegen through the Nimbus binary:

```bash
nimbus codegen --app .
```

`@nimbus/codegen` is embedded in the `nimbus` binary; it is not installed into
apps or invoked with `npx`.
