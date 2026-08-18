---
title: Agent sandbox quickstart
description: Create an isolated sandbox, lease a session to it, and tear it down with the Nimbus SDK — in about five minutes.
sidebar:
  order: 2
---

Give an agent its own isolated world: create a sandbox, wait for it to come
up, lease a session to it, and tear everything down. One server, one
script, no separate sandbox vendor.

Sandbox execution runs on Linux hosts. On macOS (and WSL2 on Windows),
`nimbus machine` provides the managed Linux VM that hosts sandboxes — see
the [CLI reference](/reference/cli/) — and
[current capabilities](/reference/current-capabilities/) has the full
status table.

## 1. Install Nimbus

```bash
brew install nimbus/tap/nimbus
```

Other platforms ship via the install script and release binaries — see the
[install options](https://github.com/nimbus/nimbus#install). You'll also
need Node.js 22 or newer for the script below.

## 2. Start the server

```bash
nimbus start --port 8080 --data-dir ./data
```

## 3. macOS / WSL2: start the machine first

Sandboxes execute on a Linux host. On Linux you can skip this step. On macOS
or WSL2, start the managed Linux VM that hosts sandboxes before creating one:

```bash
nimbus machine start
```

See the [CLI reference](/reference/cli/) for `nimbus machine` flags
(`--cpus`, `--memory`, and named machines).

## 4. Grab the admin token and create a tenant

The native API is protected by a local admin token, created on first boot:

```bash
# Linux
export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)

# macOS
export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")

curl -s -X POST http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

## 5. Install the SDK

The SDK ships inside the Nimbus binary rather than on the public npm
registry. `nimbus packages provision` materializes it into the project and
points `package.json` at the provisioned copy, so `npm install` resolves it
from disk:

```bash
mkdir agent-sandbox && cd agent-sandbox
npm init -y
nimbus packages provision nimbus
npm install
```

## 6. Create a sandbox and lease a session

Save this as `sandbox.mjs`:

```javascript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});

// Create an isolated world for one task: a root image, a process, an id.
const sandbox = await nimbus.sandboxes.create({
  profile: "worker",
  spec: {
    owner: { kind: "standalone", displayName: "agent-task" },
    backend: "container",
    root: {
      kind: "oci_image",
      source: { kind: "reference", reference: "docker.io/library/node:22-alpine" },
    },
    process: { argv: ["node", "-e", "setInterval(() => {}, 1000)"] },
  },
  labels: { purpose: "quickstart" },
});
console.log("created", sandbox.metadata.id, sandbox.status.lifecycleState);

// Poll until the sandbox is ready — sessions only attach to ready targets.
let current = sandbox;
while (current.status.lifecycleState !== "ready") {
  await new Promise((resolve) => setTimeout(resolve, 250));
  current = await nimbus.sandboxes.get({ id: sandbox.metadata.id });
}

// Lease a session to the sandbox. Every session expires on its own.
const session = await nimbus.sessions.open({
  target: { sandbox: { id: sandbox.metadata.id } },
  channels: ["stdio"],
  requestedTtlMs: 5 * 60 * 1000,
});
console.log("session", session.metadata.id, "expires", session.spec.expiresAt);
console.log("attached to", session.spec.targetSnapshot);

// Tear down: close the lease, stop the world.
await nimbus.sessions.close({ id: session.metadata.id, reason: "done" });
await nimbus.sandboxes.stop({ id: sandbox.metadata.id });
console.log("stopped");
```

## 7. Run it

```bash
node sandbox.mjs
```

The script creates the sandbox, waits for `ready`, opens a five-minute
`stdio` session, prints the target snapshot the session pinned at open
time, then closes the session and stops the sandbox.

## What just happened

- The sandbox is **addressed by id** — a receipt for a resource you
  created, not a name other code can resolve. It ran as an OCI container
  with deny-by-default network egress, inside the same tenant-isolation
  boundary as your data.
- The session is a **lease, not a connection pool entry**: it carried a
  TTL, recorded exactly what it attached to, and would have expired on its
  own if the script crashed before closing it.
- Nothing here was a special agent runtime. The same server is also
  serving your database, functions, and realtime queries.

## Next steps

- [Run sandboxes](/agents/sandboxes/) — specs, labels, listing, and what
  the API redacts.
- [Manage services](/agents/services/) — promote work to a name other code
  can depend on.
- [Open sessions](/agents/sessions/) — channels, TTLs, and target rules.
- [Services, sandboxes, and sessions](/concepts/resource-model/) — why the
  model is shaped this way.
