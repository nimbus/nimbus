import type { SandboxResource, SessionResource } from "../lib/types/sandbox";

const NOW = "2026-09-08T10:00:00Z";

// A sandbox resource as the service-control routes return it: one place the
// route specs and the hook specs read their fixtures from.
export function makeSandbox(
  overrides: {
    id?: string;
    tenantId?: string;
    displayName?: string;
    profile?: string;
    backend?: string;
    lifecycleState?: string;
    readiness?: string;
    health?: string;
    endpoints?: SandboxResource["status"]["endpoints"];
    conditions?: SandboxResource["status"]["conditions"];
    labels?: Record<string, string>;
  } = {},
): SandboxResource {
  const id = overrides.id ?? "sb-1";
  const tenantId = overrides.tenantId ?? "acme";
  const backend = overrides.backend ?? "krun";
  return {
    metadata: {
      tenantId,
      id,
      generation: 1,
      resourceVersion: "3",
      createdAt: NOW,
      updatedAt: NOW,
      labels: overrides.labels ?? {},
    },
    spec: {
      profile: overrides.profile ?? "worker",
      sandbox: {
        tenantId,
        owner: { kind: "standalone", displayName: overrides.displayName },
        backend,
        root: {
          kind: "oci_image",
          source: {
            kind: "reference",
            reference: "docker.io/library/alpine:3.20",
          },
        },
        process: {
          argv: { redacted: false, valueCount: 3 },
          environment: { redacted: true, valueCount: 2 },
          cwd: "/work",
          terminal: false,
        },
      },
    },
    status: {
      lifecycleState: overrides.lifecycleState ?? "ready",
      readiness: overrides.readiness ?? "ready",
      health: overrides.health ?? "healthy",
      backend,
      endpoints: overrides.endpoints ?? [],
      conditions: overrides.conditions ?? [],
    },
  };
}

export function makeSession(id = "sess-1", tenantId = "acme"): SessionResource {
  return {
    metadata: {
      tenantId,
      id,
      generation: 1,
      resourceVersion: "1",
      createdAt: NOW,
      updatedAt: NOW,
    },
    spec: {
      target: { sandbox: { id: "sb-1" } },
      targetSnapshot: {
        sandbox: {
          id: "sb-1",
          generation: 1,
          profile: "worker",
          backend: "krun",
        },
      },
      channels: ["stdio"],
      expiresAt: "2026-09-08T10:30:00Z",
    },
    status: {
      lifecycleState: "open",
      expiresAt: "2026-09-08T10:30:00Z",
      conditions: [],
    },
  };
}

/** An NDJSON body that sends one frame per chunk, in order, with a gap. */
export function ndjsonBody(
  frames: unknown[],
  gapMs = 2,
): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream<Uint8Array>({
    async start(controller) {
      for (const frame of frames) {
        controller.enqueue(encoder.encode(`${JSON.stringify(frame)}\n`));
        await new Promise((resolve) => setTimeout(resolve, gapMs));
      }
      controller.close();
    },
  });
}
