// One canonical declaration of the observability tabs. The route's `tab`
// search param and the tab strip both derive from this list, so adding a
// tab is a single-line change.
export const OBSERVABILITY_TABS = [
  { id: "logs", label: "Logs" },
  { id: "runs", label: "Runs" },
  { id: "traces", label: "Traces" },
  { id: "errors", label: "Errors" },
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
  // The error group the Runs tab is narrowed to, set by the Errors tab's
  // drill-in. A fingerprint names one function, error class, and message.
  fingerprint?: string;
  // The run whose detail sheet is open on the Runs tab, or whose waterfall
  // the Traces tab shows.
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

// One span of a run, as `record_run_async` writes it
// (crates/nimbus-system/src/records/trace.rs). `parent` is an index into
// the run's `spans` array; the function's own span is first and has none.
export type RunSpan = {
  name: string;
  // "function" for the run and nested ctx.run* calls, "db" for ctx.db,
  // "scheduler" for ctx.scheduler, "host" for every other host call.
  kind: string;
  parent: number | null;
  startMs: number;
  durationMs: number;
  status: string;
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
  fingerprint?: string;
  spans?: RunSpan[];
  startedAt?: number;
};

// One row of GET /api/console/errors: the failed runs that share a
// fingerprint, with the newest run as the sample.
export type ErrorGroup = {
  fingerprint: string;
  tenantId: string;
  functionPath: string;
  kind: string;
  class: string;
  message: string;
  location: string | null;
  count: number;
  firstSeen: number;
  lastSeen: number;
  latestRunId: string;
};

export type ErrorGroupPage = {
  groups: ErrorGroup[];
  scanned: number;
  exhaustive: boolean;
  limit: number;
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
    fingerprint: parseString(search.fingerprint),
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
