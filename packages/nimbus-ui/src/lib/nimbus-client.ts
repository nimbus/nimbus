import { NimbusReactClient } from "nimbus/react";

let cached: NimbusReactClient | undefined;

export function getNimbusClient(): NimbusReactClient {
  if (cached) return cached;
  cached = new NimbusReactClient(deriveServerUrl());
  return cached;
}

function deriveServerUrl(): string {
  // Embedded under /ui/*: same origin owns the API.
  // Dev server: proxy to local Nimbus over the conventional 8080 port.
  if (typeof window === "undefined") return "http://127.0.0.1:8080";
  const { origin, pathname } = window.location;
  if (pathname.startsWith("/ui/")) {
    return origin;
  }
  return "http://127.0.0.1:8080";
}
