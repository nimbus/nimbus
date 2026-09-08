import { useNimbus } from "@nimbus/nimbus/react";

// The client's own URL is the console's Convex endpoint on the server
// (`<origin>/convex/_nimbus`). Every route a developer connects to hangs
// off the origin, so that is the server URL a page shows and pastes.
export function useServerUrl(): string {
  const client = useNimbus();
  const url =
    client.url || (typeof window === "undefined" ? "" : window.location.origin);
  try {
    return new URL(url).origin;
  } catch {
    return url;
  }
}
