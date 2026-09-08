// One canonical declaration of the observability tabs. The route's `tab`
// search param and the tab strip both derive from this list, so adding a
// tab is a single-line change. Traces and Errors join it when their pages
// exist (UIR20); until then the console does not name them.
export const OBSERVABILITY_TABS = [
  { id: "logs", label: "Logs" },
  { id: "runs", label: "Runs" },
] as const;

export type ObservabilityTab = (typeof OBSERVABILITY_TABS)[number]["id"];

// The developer and operator observability routes share one search shape,
// so a link from one to the other carries its filters across unchanged and
// the tab components read the same object on both surfaces.
export type ObservabilitySearch = {
  tab?: ObservabilityTab;
  // The tenant scope. Absent means "the surface's default": the active
  // tenant on the developer page, every tenant on the operator page.
  tenant?: string;
  level?: string;
  category?: string;
  source?: string;
  correlationId?: string;
  // Free text over the line message. Set, the Logs tab reads a search page
  // from the server instead of the live stream.
  q?: string;
  status?: string;
  functionPath?: string;
  // The run whose detail sheet is open on the Runs tab.
  run?: string;
  follow?: boolean;
  pauseOnError?: boolean;
};

export type EventDoc = {
  _id: string;
  _creationTime?: number;
  tenantId?: string;
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
  tenantId?: string;
  bundleId?: string;
  functionPath?: string;
  kind?: string;
  durationMs?: number;
  status?: string;
  error?: unknown;
  startedAt?: number;
};

export function parseTab(value: unknown): ObservabilityTab | undefined {
  return OBSERVABILITY_TABS.find((tab) => tab.id === value)?.id;
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

// Both routes validate through this one parser so a search object built on
// one surface is valid on the other.
export function parseObservabilitySearch(
  search: Record<string, unknown>,
): ObservabilitySearch {
  return {
    tab: parseTab(search.tab),
    tenant: parseString(search.tenant),
    level: parseString(search.level),
    category: parseString(search.category),
    source: parseString(search.source),
    correlationId: parseString(search.correlationId),
    q: parseString(search.q),
    status: parseString(search.status),
    functionPath: parseString(search.functionPath),
    run: parseString(search.run),
    follow: parseBool(search.follow),
    pauseOnError: parseBool(search.pauseOnError),
  };
}

// The log line facets. Any of them set means the empty pane is a filter
// outcome, not a statement about the deployment.
export function hasLineFilters(search: ObservabilitySearch): boolean {
  return (
    search.level !== undefined ||
    search.category !== undefined ||
    search.source !== undefined ||
    search.correlationId !== undefined ||
    search.q !== undefined
  );
}

// The answer to GET /api/console/logs: the newest matching lines, how many
// lines matched in the scanned window, and whether that window held every
// candidate line.
export type LogSearchPage = {
  lines: EventDoc[];
  matched: number;
  scanned: number;
  exhaustive: boolean;
  limit: number;
};
