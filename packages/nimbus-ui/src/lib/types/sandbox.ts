// The sandbox and session resources as the service-control routes serve
// them (`crates/nimbus-compute/src/sandboxes.rs`, `sandbox_spec.rs`, and
// `crates/nimbus-server/src/http/sessions.rs`). Both are live runtime
// state read over HTTP, not system-table documents, so they have no
// generated `Doc<>` type.

export const SANDBOX_PROFILES = ["worker", "desktop"] as const;
export type SandboxProfile = (typeof SANDBOX_PROFILES)[number];

export const SANDBOX_BACKENDS = ["krun", "container"] as const;
export type SandboxBackend = (typeof SANDBOX_BACKENDS)[number];

export type SandboxOwner =
  | { kind: "service"; serviceName?: string }
  | { kind: "standalone"; displayName?: string };

// The public spec a create request carries. The root is always an admitted
// OCI image reference; host paths and build contexts are operator-only.
export type SandboxSpecInput = {
  owner: SandboxOwner;
  backend: SandboxBackend;
  root: { kind: "oci_image"; source: { kind: "reference"; reference: string } };
  process: { argv: string[]; env?: string[]; cwd?: string; terminal?: boolean };
};

export type SandboxCreateRequest = {
  id: string;
  profile: SandboxProfile;
  spec: SandboxSpecInput;
  labels?: Record<string, string>;
};

// The response spec redacts launch inputs an operator chose: values are
// replaced by a count, and a root that was not a public image reference
// reads `redacted`.
export type RedactedValues = { redacted: boolean; valueCount: number };

export type SandboxRootResponse =
  | { kind: "oci_image"; source: { kind: "reference"; reference: string } }
  | { kind: "redacted"; redacted: true; reason: string };

export type SandboxSpecResponse = {
  tenantId: string;
  owner: SandboxOwner;
  backend: string;
  root: SandboxRootResponse;
  process: {
    argv: RedactedValues;
    entrypoint?: RedactedValues;
    command?: RedactedValues;
    environment: RedactedValues;
    cwd: string;
    user?: string;
    terminal: boolean;
  };
};

export type SandboxEndpoint = {
  name: string;
  protocol: string;
  host: string;
  port: number;
};

export type ResourceCondition = {
  type: string;
  status: string;
  reason: string;
  message: string;
  observedGeneration: number;
  lastTransitionTime: string;
};

// `status.lifecycleState` is the sandbox status vocabulary the runtime
// reports (`pending` before the first observation, then `starting`,
// `ready`, `not_ready`, `stopping`, `stopped`, `failed`).
export type SandboxResource = {
  metadata: {
    tenantId: string;
    id: string;
    generation: number;
    resourceVersion: string;
    createdAt: string;
    updatedAt: string;
    labels: Record<string, string>;
  };
  spec: { profile: string; sandbox: SandboxSpecResponse };
  status: {
    lifecycleState: string;
    readiness: string;
    health: string;
    backend: string;
    endpoints: SandboxEndpoint[];
    conditions: ResourceCondition[];
  };
};

export type CollectionMetadata = {
  tenantId: string;
  resourceVersion: string;
  limit: number;
  nextPageToken?: string;
  remainingCount: number;
};

export type SandboxCollection = {
  metadata: CollectionMetadata;
  items: SandboxResource[];
};

// Sessions attach a channel set to one target. A sandbox session admits
// `stdio` and `files`; `stdio` is the one the console streams.
export type SessionTarget =
  | { sandbox: { id: string } }
  | { service: { name: string } };

export type SessionOpenRequest = {
  tenantId?: string;
  target: SessionTarget;
  channels: string[];
  requestedTtlMs?: number;
};

export type SessionResource = {
  metadata: {
    tenantId: string;
    id: string;
    generation: number;
    resourceVersion: string;
    createdAt: string;
    updatedAt: string;
  };
  spec: {
    target: SessionTarget;
    targetSnapshot:
      | {
          sandbox: {
            id: string;
            generation: number;
            profile: string;
            backend: string;
          };
        }
      | {
          service: {
            name: string;
            generation: number;
            backend: string;
            provider?: string;
          };
        };
    channels: string[];
    expiresAt: string;
  };
  status: {
    lifecycleState: "open" | "closed" | "expired";
    expiresAt: string;
    closedAt?: string;
    closeReason?: string;
    conditions: ResourceCondition[];
  };
};

/** Lifecycle states from which a stop request still makes sense. */
export function sandboxCanStop(state: string | undefined): boolean {
  const value = (state ?? "").toLowerCase();
  return !["stopping", "stopped", "failed"].includes(value);
}

/** States the runtime is still moving out of; the console re-reads them. */
export function sandboxIsTransitional(state: string | undefined): boolean {
  const value = (state ?? "").toLowerCase();
  return ["pending", "starting", "stopping", "not_ready"].includes(value);
}

export function sandboxDisplayName(sandbox: SandboxResource): string {
  const owner = sandbox.spec.sandbox.owner;
  if (owner.kind === "standalone" && owner.displayName)
    return owner.displayName;
  if (owner.kind === "service" && owner.serviceName) return owner.serviceName;
  return sandbox.metadata.id;
}
