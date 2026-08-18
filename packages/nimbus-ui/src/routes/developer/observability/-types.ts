// One canonical declaration of the observability tab universe. The full
// union (`ObservabilityTab`) is what shows up in the tab strip; the active
// subset (`ActiveObservabilityTab`) is what the route's `tab` search param
// is allowed to take. Both unions derive from the literal arrays below so
// adding a tab is a single-line change.
export const ACTIVE_OBSERVABILITY_TABS = ["logs", "runs"] as const;
export const DISABLED_OBSERVABILITY_TABS = ["events", "errors"] as const;

export type ActiveObservabilityTab =
  (typeof ACTIVE_OBSERVABILITY_TABS)[number];
export type DisabledObservabilityTab =
  (typeof DISABLED_OBSERVABILITY_TABS)[number];
export type ObservabilityTab =
  | ActiveObservabilityTab
  | DisabledObservabilityTab;

export type ObservabilitySearch = {
  tab?: ActiveObservabilityTab;
  level?: string;
  category?: string;
  source?: string;
  correlationId?: string;
  status?: string;
  functionPath?: string;
  follow?: boolean;
  pauseOnError?: boolean;
};

export type EventDoc = {
  _id: string;
  _creationTime?: number;
  source?: string;
  level?: string;
  category?: string;
  message?: string;
  data?: Record<string, unknown> | null;
  correlationId?: string | null;
  createdAt?: number;
};

export type RunDoc = {
  _id: string;
  _creationTime?: number;
  bundleId?: string;
  functionPath?: string;
  kind?: string;
  durationMs?: number;
  status?: string;
  error?: unknown;
  startedAt?: number;
};

export function parseTab(value: unknown): ActiveObservabilityTab | undefined {
  return ACTIVE_OBSERVABILITY_TABS.find((id) => id === value);
}

export function parseString(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length === 0 ? undefined : trimmed;
}

export function parseBool(value: unknown): boolean | undefined {
  if (value === true || value === "1" || value === "true") return true;
  if (value === false || value === "0" || value === "false") return false;
  return undefined;
}
