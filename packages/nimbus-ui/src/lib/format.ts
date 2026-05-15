export function formatRelativeTime(epochMs: number, now = Date.now()): string {
  const diff = Math.max(0, now - epochMs);
  if (diff < 5_000) return "just now";
  if (diff < 60_000) return `${Math.floor(diff / 1_000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

export function formatAbsoluteTime(epochMs: number): string {
  try {
    return new Date(epochMs).toISOString().replace("T", " ").replace("Z", "");
  } catch {
    return String(epochMs);
  }
}

export function formatUptime(startedAtMs: number, now = Date.now()): string {
  const diff = Math.max(0, now - startedAtMs);
  const days = Math.floor(diff / 86_400_000);
  const hours = Math.floor((diff % 86_400_000) / 3_600_000);
  const minutes = Math.floor((diff % 3_600_000) / 60_000);
  if (days > 0) return `${days}d ${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "—";
  if (ms < 1) return "<1ms";
  if (ms < 1_000) return `${Math.round(ms)}ms`;
  if (ms < 60_000) return `${(ms / 1_000).toFixed(2)}s`;
  return `${Math.floor(ms / 60_000)}m ${Math.floor((ms % 60_000) / 1_000)}s`;
}

export function shortId(value: string, length = 7): string {
  if (value.length <= length + 2) return value;
  return value.slice(0, length);
}
