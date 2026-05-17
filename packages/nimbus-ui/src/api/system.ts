export type CheckStatus = "fresh" | "stale" | "never" | "error" | "disabled";

export type UpgradeMethod =
  | "brew"
  | "apt"
  | "dnf"
  | "install-script"
  | "source"
  | "unknown";

export type VersionUpgrade = {
  method: UpgradeMethod;
  command: string | null;
  needsSudo: boolean;
  interactive: boolean;
  fallbackUrl: string;
};

export type VersionInfo = {
  current: string;
  latest: string | null;
  available: boolean;
  url: string | null;
  publishedAt: string | null;
  host: string;
  checkStatus: CheckStatus;
  upgrade: VersionUpgrade;
};

export async function fetchVersionInfo(
  signal?: AbortSignal,
): Promise<VersionInfo> {
  const res = await fetch("/api/system/version-info", {
    credentials: "include",
    signal,
  });
  if (!res.ok) throw new Error(`version-info ${res.status}`);
  return (await res.json()) as VersionInfo;
}
