import { NimbusReactClient } from "nimbus/react";

const SYSTEM_TENANT = "_nimbus";

let cached: NimbusReactClient | undefined;

export function getNimbusClient(): NimbusReactClient {
  if (cached) return cached;
  cached = new NimbusReactClient(deriveServerUrl(), {
    skipDeploymentUrlCheck: true,
  });
  return cached;
}

function deriveServerUrl(): string {
  // Operator console reads/writes through the system tenant. The convex
  // adapter's tenant-prefixed routes give us a single URL that owns both
  // `/query` and `/ws` so the nimbus client doesn't have to special-case the
  // socket path.
  const origin =
    typeof window === "undefined"
      ? "http://127.0.0.1:8080"
      : window.location.origin;
  return `${origin}/convex/${SYSTEM_TENANT}`;
}
