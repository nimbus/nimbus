import type { UpgradeMethod } from "../api/system";

export type UpgradeEvent =
  | { kind: "stdout"; line: string }
  | { kind: "stderr"; line: string }
  | { kind: "exit"; code: number }
  | { kind: "error"; message: string };

export type DesktopBridge = {
  runUpgrade: (method: UpgradeMethod) => AsyncIterable<UpgradeEvent>;
};

type WindowWithNimbus = {
  nimbus?: DesktopBridge;
};

export function getDesktopBridge(): DesktopBridge | null {
  if (typeof window === "undefined") return null;
  const candidate = (window as unknown as WindowWithNimbus).nimbus;
  if (!candidate || typeof candidate.runUpgrade !== "function") return null;
  return candidate;
}

const LOCAL_HOST_PREDICATES = new Set([
  "",
  "localhost",
  "127.0.0.1",
  "::1",
  "[::1]",
]);

export function isLocalHost(serverHost: string | null | undefined): boolean {
  if (!serverHost) return true;
  const trimmed = serverHost.trim().toLowerCase();
  if (LOCAL_HOST_PREDICATES.has(trimmed)) return true;
  if (typeof window === "undefined") return false;
  const pageHost = window.location.host.toLowerCase();
  if (trimmed === pageHost) return true;
  const pageHostname = window.location.hostname.toLowerCase();
  return trimmed === pageHostname;
}
