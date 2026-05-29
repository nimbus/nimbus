# SDK Canaries

This root holds realistic Application-preset SDK package canaries for NFRC8.

These are pinned package smoke tests for common FaaS and Convex `"use node"`
app dependencies. They use local mock HTTP servers where a package normally
talks to a third-party service, so the canaries exercise real SDK code without
requiring external credentials or network access.

Current package set:

- `openai`
- `@anthropic-ai/sdk`
- `ai`
- `stripe`
- `resend`
- `@aws-sdk/client-s3`
- `@slack/web-api`
- `octokit`
- `jose`
- `zod`
- `uuid`
- `nanoid`
- `@upstash/redis`

Install the pinned dependencies locally:

```bash
make node-compat-canaries-bootstrap PRESET=application
```

Run the batched runtime canaries:

```bash
make node-compat-canaries PRESET=application
```

The canary registry records Node22 and Node24 as supported-LTS claim lanes and
also records Node26 as Current-line evidence.
