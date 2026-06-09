# Nimbus SDK Resource Model Examples

These examples use the public product SDK:

```ts
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });
```

Adapter APIs stay adapter-shaped. Code that needs Nimbus services, sandboxes, or
sessions imports `Nimbus` directly.

## Start A Compose Service

Compose service names become tenant-scoped Nimbus service names.

```ts
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

await nimbus.services.start({ name: "db", waitUntil: "ready" });
const db = await nimbus.services.get({ name: "db" });

console.log(db.readiness, db.endpoints);
```

## Register A Built-In Load Balancer Service

Built-in load balancer and service-discovery services are Nimbus service
definitions. App code can declare and inspect the service, but the MVP SDK does
not expose raw upstream resolution.

```ts
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

const edge = await nimbus.services.create({
  name: "edge",
  backend: { kind: "builtIn", provider: "loadBalancer" },
  labels: { app: "web" },
});

const status = await nimbus.services.get({ name: edge.metadata.name });
console.log(status.lifecycleState);
```

A sandboxed nginx load balancer consumes generated upstream config from the
control plane or a future explicit resolver. Do not teach application code to
call a service resolver until that route and authority model exist.

## Create A Task Sandbox And Open A Session

Sandboxes are id-addressed isolated execution resources.

```ts
import { Nimbus, type NimbusSandboxSpec } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

const spec = {
  tenantId: "demo",
  owner: { kind: "standalone", displayName: "research-worker" },
  backend: "krun",
  root: {
    kind: "oci_image",
    source: {
      kind: "reference",
      reference: "registry.example.com/research-worker:latest",
    },
  },
  process: { argv: ["worker"] },
} satisfies NimbusSandboxSpec;

const sandbox = await nimbus.sandboxes.create({
  profile: "worker",
  spec,
  labels: { app: "research" },
});

const shell = await nimbus.sessions.open({
  target: { sandbox: { id: sandbox.metadata.id } },
  channels: ["stdio", "files"],
});

console.log(shell.metadata.id);
```

## Open A Built-In Browser Service Session

Services can expose sessions when their backend supports channels.

```ts
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

await nimbus.services.create({
  name: "browser",
  backend: { kind: "builtIn", provider: "browser" },
});

const browser = await nimbus.sessions.open({
  target: { service: { name: "browser" } },
  channels: ["cdp", "page"],
  requestedTtlMs: 15 * 60 * 1000,
});

console.log(browser.status.lifecycleState);
```

## Register A Temporary Sandbox-Backed Service

A dynamically declared service is still name-addressed. Its backing sandbox is
not resolved by sandbox name.

```ts
import { Nimbus, type NimbusSandboxSpec } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

const serviceSandbox = {
  tenantId: "demo",
  owner: { kind: "service", serviceName: "mcp-tools" },
  backend: "krun",
  root: {
    kind: "oci_image",
    source: {
      kind: "reference",
      reference: "registry.example.com/mcp-tools:latest",
    },
  },
  process: { argv: ["mcp-tools"] },
} satisfies NimbusSandboxSpec;

await nimbus.services.create({
  name: "mcp-tools",
  backend: { kind: "sandbox", sandbox: serviceSandbox },
});

const tools = await nimbus.sessions.open({
  target: { service: { name: "mcp-tools" } },
  channels: ["stdio"],
});

console.log(tools.spec.targetSnapshot);
```

## Use Nimbus From An Adapter Action

Adapter contexts do not grow Nimbus resource namespaces.

```ts
import { action } from "./_generated/server";
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus();

export const warmSearch = action({
  args: {},
  handler: async () => {
    await nimbus.services.start({ name: "search", waitUntil: "ready" });
  },
});
```

Convex query/mutation network restrictions still apply. Put service lifecycle
and session work in allowed network tiers such as actions, HTTP actions, Cloud
Functions handlers, native jobs, or operator workflows.
