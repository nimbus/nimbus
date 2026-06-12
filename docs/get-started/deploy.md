---
title: Deploy to production
description: Push your app to a production Nimbus server with one command — preview the diff, activate atomically, and skip the IaC entirely.
sidebar:
  order: 5
---

Take an app from `nimbus dev` to a production server. There is no
infrastructure layer between the two: the production server is the same
binary you develop against, and the only verb that changes is
`deploy`. This tutorial uses a second local server as "production" so you
can run the whole flow on one machine; the last section covers a real
remote server.

You need an app with a `nimbus/` or `convex/` source root — the
[developer quickstart](/get-started/quickstart/) builds one in five
minutes.

## 1. Start a server that accepts deploys

The deploy endpoint is off by default. It turns on when the server starts
with a deploy token in its environment:

```bash
export NIMBUS_DEPLOY_TOKEN=$(openssl rand -hex 32)
nimbus start --port 8080 --data-dir ./prod-data
```

Keep that shell open — the same exported token authenticates the deploys
below.

## 2. Preview with a dry run

From your app directory, point a dry run at the server:

```bash
cd my-app
nimbus deploy --url http://127.0.0.1:8080 --dry-run
```

`nimbus deploy` runs codegen inside the binary, packages your functions,
HTTP routes, schema, and runtime bundle, and asks the server to validate
and diff them against whatever is currently active — without activating
anything. You see exactly which functions and routes would be added,
changed, or removed.

The deploy token resolves from `NIMBUS_DEPLOY_TOKEN`; because the target
is a loopback address on this machine, the server's local admin token is
read from the local token file automatically.

## 3. Ship it

Same command, without the flag:

```bash
nimbus deploy --url http://127.0.0.1:8080
```

The server stages the artifacts, validates them — manifests, routes,
schema and indexes, and the runtime bundle's SHA-256 integrity — and only
then activates the new generation. The swap is atomic: in-flight requests
finish on the generation they captured, new requests observe the new one.
If validation fails, the previous generation stays active and the command
reports why.

## 4. Verify

Run the dry run again:

```bash
nimbus deploy --url http://127.0.0.1:8080 --dry-run
```

The diff now reports no added, changed, or removed functions or routes —
what the server is running is what you just shipped.

## Deploying to a real server

For a server that isn't on your machine, two things change:

- **The admin token is no longer read automatically.** Pass it with
  `--admin-token` or set `NIMBUS_ADMIN_TOKEN`.
- **You can store the deploy token instead of exporting it.**
  `nimbus auth login --url https://nimbus.example.com` saves the bearer in
  a local credentials file, and `nimbus deploy` falls back to it whenever
  `NIMBUS_DEPLOY_TOKEN` is unset.

```bash
nimbus auth login --url https://nimbus.example.com
nimbus deploy --url https://nimbus.example.com --admin-token "$PROD_ADMIN_TOKEN"
```

Provisioning the server itself is one binary and one systemd unit — see
[deploy to a Linux server](/operators/deploy-linux/).

## What you didn't write

No Terraform, no Helm chart, no Dockerfile, no build pipeline. Dev and
prod run the same binary with the same configuration surface; the
difference between them is `nimbus dev` watching your files versus
`nimbus deploy` pushing validated artifacts. The full server-side
contract — request schema, diff format, and error catalog — is in the
[deploy & admin API reference](/reference/deploy-admin-api/), and every
flag is in the [CLI reference](/reference/cli/).
