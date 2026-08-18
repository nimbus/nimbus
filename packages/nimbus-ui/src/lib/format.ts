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

/**
 * Shorten an opaque identifier while keeping it distinguishable.
 *
 * Nimbus document ids are ULIDs: the leading characters encode the creation
 * timestamp, so every row written in the same millisecond shares a long common
 * prefix. A leading-only truncation renders them all identically — a table of
 * a hundred documents reads as a hundred copies of `01M0APV`. Keep both ends
 * so neighbouring rows stay tellable apart; the full value stays available
 * through the copy affordance and the `title` attribute.
 */
export function shortId(value: string, length = 13): string {
  if (value.length <= length) return value;
  const head = Math.ceil((length - 1) / 2);
  const tail = length - 1 - head;
  return `${value.slice(0, head)}\u2026${value.slice(value.length - tail)}`;
}

/**
 * Shorten a content hash. Hash digests are uniformly distributed, so a leading
 * prefix is both distinguishing and the form operators already read elsewhere
 * (git, OCI, the Nimbus CLI). Never use this for ULIDs — see `shortId`.
 */
export function shortHash(value: string, length = 12): string {
  if (value.length <= length) return value;
  return value.slice(0, length);
}

/**
 * Epoch-millisecond range this console is willing to read as a wall-clock
 * time: 2001-09-09 through 2096-10-02. Values outside it are ordinary numbers.
 */
const PLAUSIBLE_EPOCH_MS_MIN = 1_000_000_000_000;
const PLAUSIBLE_EPOCH_MS_MAX = 4_000_000_000_000;

const TIME_FIELD_PATTERN =
  /^_creationTime$|(^|_)(at|ts)$|(^|_)(time|timestamp|date)s?$|(At|Ts|Time|Timestamp|Date)$/;

/**
 * Decide whether a numeric document field should be rendered as a wall-clock
 * time. A generic document browser cannot know the schema's intent, so this
 * requires two independent signals to agree: the field is named like a time
 * (`_creationTime`, `at`, `created_at`, `updatedAt`, `expiryTs`), and the value
 * falls inside a plausible epoch-millisecond window. The raw number stays
 * reachable through the cell's `title`, so a false positive is recoverable and
 * a false negative just prints the number as before.
 */
export function looksLikeEpochMs(field: string, value: number): boolean {
  if (!Number.isFinite(value) || !Number.isInteger(value)) return false;
  if (value < PLAUSIBLE_EPOCH_MS_MIN || value > PLAUSIBLE_EPOCH_MS_MAX) {
    return false;
  }
  return TIME_FIELD_PATTERN.test(field);
}

export function formatMemory(mib: number | undefined | null): string {
  if (mib === undefined || mib === null) return "—";
  if (mib >= 1024) {
    const gib = mib / 1024;
    return `${gib % 1 === 0 ? gib.toFixed(0) : gib.toFixed(1)} GiB`;
  }
  return `${mib} MiB`;
}
