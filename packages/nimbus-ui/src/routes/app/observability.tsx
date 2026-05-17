import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import {
  type MouseEvent as ReactMouseEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { api } from "../../../convex/_generated/api";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatDuration, shortId } from "../../lib/format";

type ObservabilityTab = "logs" | "runs";

type ObservabilitySearch = {
  tab?: ObservabilityTab;
  level?: string;
  category?: string;
  source?: string;
  correlationId?: string;
  status?: string;
  functionPath?: string;
  follow?: boolean;
  pauseOnError?: boolean;
};

const LEVELS = ["error", "warn", "info", "debug", "trace"] as const;
const RUN_STATUSES = ["ok", "error", "running", "queued"] as const;

interface NimbusPerfEventStore {
  snapshot: () => EventDoc[];
  subscribe: (listener: () => void) => () => void;
}

declare global {
  interface Window {
    __nimbusEvents?: NimbusPerfEventStore;
  }
}

const emptyEvents: EventDoc[] = [];

function getPerfStore(): NimbusPerfEventStore | undefined {
  return typeof window === "undefined" ? undefined : window.__nimbusEvents;
}

function usePerfEventStream(): EventDoc[] | undefined {
  const subscribe = useCallback((listener: () => void) => {
    const store = getPerfStore();
    if (!store) return () => {};
    return store.subscribe(listener);
  }, []);
  const getSnapshot = useCallback(() => {
    const store = getPerfStore();
    return store ? store.snapshot() : emptyEvents;
  }, []);
  const getServerSnapshot = useCallback(() => emptyEvents, []);
  const snapshot = useSyncExternalStore(
    subscribe,
    getSnapshot,
    getServerSnapshot,
  );
  return getPerfStore() ? snapshot : undefined;
}

function parseTab(value: unknown): ObservabilityTab | undefined {
  return value === "logs" || value === "runs" ? value : undefined;
}

function parseString(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length === 0 ? undefined : trimmed;
}

function parseBool(value: unknown): boolean | undefined {
  if (value === true || value === "1" || value === "true") return true;
  if (value === false || value === "0" || value === "false") return false;
  return undefined;
}

export const Route = createFileRoute("/app/observability")({
  validateSearch: (search: Record<string, unknown>): ObservabilitySearch => ({
    tab: parseTab(search.tab),
    level: parseString(search.level),
    category: parseString(search.category),
    source: parseString(search.source),
    correlationId: parseString(search.correlationId),
    status: parseString(search.status),
    functionPath: parseString(search.functionPath),
    follow: parseBool(search.follow),
    pauseOnError: parseBool(search.pauseOnError),
  }),
  component: ObservabilityPage,
});

type EventDoc = {
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

type RunDoc = {
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

function ObservabilityPage() {
  const search = Route.useSearch();
  const tab: ObservabilityTab = search.tab ?? "logs";
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-observability"
    >
      <Header tab={tab} />
      {tab === "logs" ? (
        <LogsTab search={search} />
      ) : (
        <RunsTab search={search} />
      )}
    </section>
  );
}

function Header({ tab }: { tab: ObservabilityTab }) {
  return (
    <header className="flex flex-col gap-3">
      <div>
        <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
          Observability
        </h1>
        <p className="text-sm text-muted">
          Live event stream and recent runs. Reads stream from the{" "}
          <code className="font-mono text-default">_nimbus</code> system tenant.
        </p>
      </div>
      <nav
        aria-label="Observability tabs"
        className="flex gap-px overflow-hidden rounded-md border border-app bg-surface-2 self-start"
        data-testid="observability-tabs"
      >
        <TabLink id="logs" label="Logs" active={tab === "logs"} />
        <TabLink id="runs" label="Runs" active={tab === "runs"} />
      </nav>
    </header>
  );
}

function TabLink({
  id,
  label,
  active,
}: {
  id: ObservabilityTab;
  label: string;
  active: boolean;
}) {
  return (
    <Link
      to="/app/observability"
      search={(prev) => ({ ...prev, tab: id })}
      data-testid={`observability-tab-${id}`}
      aria-current={active ? "page" : undefined}
      className={cn(
        "px-3 py-1.5 font-mono text-xs uppercase tracking-wide",
        active
          ? "bg-surface text-default"
          : "text-muted hover:bg-surface hover:text-default",
      )}
    >
      {label}
    </Link>
  );
}

function LogsTab({ search }: { search: ObservabilitySearch }) {
  const navigate = useNavigate({ from: "/app/observability" });
  const live = useQuery(api.events.recent, {
    source: search.source ?? null,
    level: search.level ?? null,
    category: search.category ?? null,
    correlationId: search.correlationId ?? null,
    limit: 200,
  }) as EventDoc[] | undefined;
  const perf = usePerfEventStream();
  const events = perf ?? live;

  const follow = search.follow ?? false;
  const pauseOnError = search.pauseOnError ?? false;

  const setSearch = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to: "/app/observability",
        search: (prev) => ({ ...prev, ...patch }),
        replace: true,
      });
    },
    [navigate],
  );

  const setSearchAction = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to: "/app/observability",
        search: (prev) => ({ ...prev, ...patch }),
      });
    },
    [navigate],
  );

  const sorted = useMemo(() => {
    return (events ?? [])
      .slice()
      .sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));
  }, [events]);

  const lastErrorRef = useRef<string | null>(null);
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (!pauseOnError) {
      setPaused(false);
      return;
    }
    const newest = sorted[0];
    if (!newest) return;
    const isError =
      (newest.level ?? "").toLowerCase() === "error" ||
      (newest.level ?? "").toLowerCase() === "warn";
    if (isError && lastErrorRef.current !== newest._id) {
      setPaused(true);
      lastErrorRef.current = newest._id;
    }
  }, [pauseOnError, sorted]);

  const visible = useMemo(() => {
    if (!paused) return sorted;
    const idx = sorted.findIndex((e) => e._id === lastErrorRef.current);
    return idx < 0 ? sorted : sorted.slice(idx);
  }, [paused, sorted]);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-logs"
    >
      <LogFilterBar
        search={search}
        setSearch={setSearch}
        follow={follow}
        pauseOnError={pauseOnError}
        paused={paused}
        onResume={() => {
          setPaused(false);
          lastErrorRef.current = null;
        }}
        onClear={() =>
          setSearchAction({
            level: undefined,
            category: undefined,
            source: undefined,
            correlationId: undefined,
          })
        }
      />
      <LogStream events={visible} follow={follow} paused={paused} />
    </div>
  );
}

function LogFilterBar({
  search,
  setSearch,
  follow,
  pauseOnError,
  paused,
  onResume,
  onClear,
}: {
  search: ObservabilitySearch;
  setSearch: (patch: Partial<ObservabilitySearch>) => void;
  follow: boolean;
  pauseOnError: boolean;
  paused: boolean;
  onResume: () => void;
  onClear: () => void;
}) {
  return (
    <div
      className="grid grid-cols-[auto_auto_auto_auto_1fr] items-center gap-2"
      data-testid="observability-log-filters"
    >
      <FilterSelect
        id="log-level"
        label="Level"
        value={search.level ?? ""}
        options={[
          { value: "", label: "all levels" },
          ...LEVELS.map((l) => ({ value: l, label: l })),
        ]}
        onChange={(v) => setSearch({ level: v || undefined })}
        testid="observability-filter-level"
      />
      <FilterInput
        id="log-category"
        label="Category"
        value={search.category ?? ""}
        placeholder="category"
        onChange={(v) => setSearch({ category: v || undefined })}
        testid="observability-filter-category"
      />
      <FilterInput
        id="log-source"
        label="Source"
        value={search.source ?? ""}
        placeholder="source"
        onChange={(v) => setSearch({ source: v || undefined })}
        testid="observability-filter-source"
      />
      <FilterInput
        id="log-correlation"
        label="Correlation"
        value={search.correlationId ?? ""}
        placeholder="run id"
        onChange={(v) => setSearch({ correlationId: v || undefined })}
        testid="observability-filter-correlation"
      />
      <div className="flex items-center justify-end gap-2">
        {paused ? (
          <button
            type="button"
            onClick={onResume}
            className="rounded border border-danger px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-danger hover:bg-surface-2"
            data-testid="observability-log-resume"
          >
            paused · resume
          </button>
        ) : null}
        <Toggle
          id="follow-mode"
          label="Follow"
          value={follow}
          onChange={(v) => setSearch({ follow: v ? true : undefined })}
          testid="observability-log-follow"
        />
        <Toggle
          id="pause-on-error"
          label="Pause on error"
          value={pauseOnError}
          onChange={(v) => setSearch({ pauseOnError: v ? true : undefined })}
          testid="observability-log-pause-on-error"
        />
        <button
          type="button"
          onClick={onClear}
          className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
          data-testid="observability-filter-clear"
        >
          clear
        </button>
      </div>
    </div>
  );
}

function FilterSelect({
  id,
  label,
  value,
  options,
  onChange,
  testid,
}: {
  id: string;
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (v: string) => void;
  testid: string;
}) {
  return (
    <label
      htmlFor={id}
      className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-muted"
    >
      <span>{label}</span>
      <select
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default focus-visible:border-strong"
        data-testid={testid}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function FilterInput({
  id,
  label,
  value,
  placeholder,
  onChange,
  testid,
}: {
  id: string;
  label: string;
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
  testid: string;
}) {
  return (
    <label
      htmlFor={id}
      className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-muted"
    >
      <span>{label}</span>
      <input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default placeholder:text-muted focus-visible:border-strong"
        data-testid={testid}
      />
    </label>
  );
}

function Toggle({
  id,
  label,
  value,
  onChange,
  testid,
}: {
  id: string;
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
  testid: string;
}) {
  return (
    <button
      type="button"
      id={id}
      role="switch"
      aria-checked={value}
      onClick={() => onChange(!value)}
      className={cn(
        "rounded border px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
        value
          ? "border-strong bg-surface text-default"
          : "border-app text-muted hover:bg-surface hover:text-default",
      )}
      data-testid={testid}
    >
      {label}
    </button>
  );
}

function LogStream({
  events,
  follow,
  paused,
}: {
  events: EventDoc[];
  follow: boolean;
  paused: boolean;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const scrollAnchorRef = useRef<{
    top: number;
    height: number;
    version: string;
  } | null>(null);
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    correlationId: string;
  } | null>(null);
  const eventVersion = useMemo(() => {
    const first = events[0]?._id ?? "";
    const last = events.at(-1)?._id ?? "";
    return `${events.length}:${first}:${last}`;
  }, [events]);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (follow && !paused) {
      el.scrollTop = 0;
      return;
    }
    const anchor = scrollAnchorRef.current;
    if (!anchor) return;
    if (anchor.version === eventVersion) return;
    const delta = el.scrollHeight - anchor.height;
    if (delta > 0) {
      el.scrollTop = anchor.top + delta;
    }
  }, [eventVersion, follow, paused]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    scrollAnchorRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      version: eventVersion,
    };
  }, [eventVersion]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [menu]);

  if (events.length === 0) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="observability-log-empty"
      >
        No events match the current filters.
      </div>
    );
  }

  const handleContextMenu = (
    e: ReactMouseEvent<HTMLElement>,
    correlationId: string | null | undefined,
  ) => {
    if (!correlationId) return;
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, correlationId });
  };

  return (
    <div
      ref={containerRef}
      className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface"
      data-testid="observability-log-stream"
    >
      <ul className="divide-y divide-app">
        {events.map((event) => {
          const correlationId = event.correlationId ?? undefined;
          return (
            <li key={event._id}>
              <article
                onContextMenu={(e) => handleContextMenu(e, correlationId)}
                aria-label={`Log entry${correlationId ? `, correlation ${shortId(correlationId, 8)}` : ""}: ${event.message ?? ""}`}
                data-testid={`observability-log-row-${event._id}`}
                className={cn(
                  "grid grid-cols-[auto_auto_auto_1fr_auto] items-baseline gap-2 px-3 py-1.5 text-xs",
                  "hover:bg-surface-2",
                )}
              >
                <RelativeTime
                  epochMs={event.createdAt ?? event._creationTime ?? 0}
                />
                <StateChip state={event.level ?? "info"} />
                <span className="font-mono text-[10px] uppercase tracking-wide text-muted">
                  {event.source ?? "—"}
                  {event.category ? ` · ${event.category}` : ""}
                </span>
                <span className="font-mono text-default truncate">
                  {event.message ?? "(no message)"}
                </span>
                {correlationId ? (
                  <CorrelationBadge
                    correlationId={correlationId}
                    eventId={event._id}
                  />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </article>
            </li>
          );
        })}
      </ul>
      {menu ? (
        <div
          role="menu"
          aria-label="Log entry actions"
          style={{ top: menu.y, left: menu.x }}
          className="fixed z-50 min-w-[160px] rounded-md border border-app bg-surface py-1 font-mono text-xs shadow-lg"
          data-testid="observability-log-context-menu"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => {
            if (e.key === "Escape") setMenu(null);
          }}
        >
          <Link
            to="/app/compute/runs/$runId"
            params={{ runId: menu.correlationId }}
            role="menuitem"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-default hover:bg-surface-2"
            data-testid="observability-log-open-run"
            onClick={() => setMenu(null)}
          >
            Open run
            <span className="ml-auto text-muted">
              {shortId(menu.correlationId, 8)}
            </span>
          </Link>
        </div>
      ) : null}
    </div>
  );
}

function CorrelationBadge({
  correlationId,
  eventId,
}: {
  correlationId: string;
  eventId: string;
}) {
  return (
    <span className="inline-flex items-center gap-1">
      <Link
        to="/app/compute/runs/$runId"
        params={{ runId: correlationId }}
        className="inline-flex items-center gap-1 rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted hover:bg-surface-2 hover:text-default focus-visible:bg-surface-2 focus-visible:text-default"
        data-testid={`observability-log-jump-${eventId}`}
        aria-label={`Jump to run ${correlationId}`}
        title={`Jump to run ${correlationId}`}
      >
        <span>↗</span>
        <span>{shortId(correlationId, 6)}</span>
      </Link>
    </span>
  );
}

function RunsTab({ search }: { search: ObservabilitySearch }) {
  const navigate = useNavigate({ from: "/app/observability" });
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: search.functionPath ?? null,
    status: search.status ?? null,
    limit: 200,
  }) as RunDoc[] | undefined;

  const setSearch = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to: "/app/observability",
        search: (prev) => ({ ...prev, ...patch }),
        replace: true,
      });
    },
    [navigate],
  );

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-runs"
    >
      <AdapterHonesty />
      <div
        className="grid grid-cols-[auto_auto_1fr] items-center gap-2"
        data-testid="observability-run-filters"
      >
        <FilterSelect
          id="run-status"
          label="Status"
          value={search.status ?? ""}
          options={[
            { value: "", label: "all" },
            ...RUN_STATUSES.map((s) => ({ value: s, label: s })),
          ]}
          onChange={(v) => setSearch({ status: v || undefined })}
          testid="observability-filter-run-status"
        />
        <FilterInput
          id="run-function"
          label="Function"
          value={search.functionPath ?? ""}
          placeholder="path/to/fn"
          onChange={(v) => setSearch({ functionPath: v || undefined })}
          testid="observability-filter-run-function"
        />
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() =>
              setSearch({ status: undefined, functionPath: undefined })
            }
            className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
            data-testid="observability-run-filter-clear"
          >
            clear
          </button>
        </div>
      </div>
      <RunsTable runs={runs} />
    </div>
  );
}

function AdapterHonesty() {
  return (
    <div
      className="rounded-md border border-app bg-surface-2 px-3 py-2 font-mono text-xs text-muted"
      data-testid="observability-adapter-honesty"
    >
      <span className="text-default">
        Convex / Nimbus runtime invocation history.
      </span>{" "}
      Native HTTP, scheduler, MongoDB, Firebase, and Cloud Functions traffic is
      surfaced under Logs — see the{" "}
      <Link
        to="/app/observability"
        search={(prev) => ({ ...prev, tab: "logs" })}
        className="underline hover:text-default focus-visible:text-default"
        data-testid="observability-adapter-honesty-events-link"
      >
        Events view
      </Link>{" "}
      for cross-adapter coverage.
    </div>
  );
}

function RunsTable({ runs }: { runs: RunDoc[] | undefined }) {
  if (runs === undefined) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="observability-runs-loading"
      >
        Loading runs…
      </div>
    );
  }
  if (runs.length === 0) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="observability-runs-empty"
      >
        No runs recorded yet.
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface">
      <table
        className="w-full border-collapse text-sm"
        data-testid="observability-runs-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Function</Th>
            <Th>Status</Th>
            <Th>Kind</Th>
            <Th align="right">Duration</Th>
            <Th>Started</Th>
            <Th>Run id</Th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <tr
              key={run._id}
              className="border-t border-app hover:bg-surface-2"
              data-testid={`observability-run-row-${run._id}`}
            >
              <Td>
                <Link
                  to="/app/compute/runs/$runId"
                  params={{ runId: run._id }}
                  className="font-mono text-default hover:underline"
                  data-testid={`observability-run-link-${run._id}`}
                >
                  {run.functionPath ?? shortId(run._id, 12)}
                </Link>
              </Td>
              <Td>
                <StateChip state={run.status} />
              </Td>
              <Td>
                <span className="font-mono text-xs uppercase tracking-wide text-muted">
                  {run.kind ?? "—"}
                </span>
              </Td>
              <Td align="right" mono>
                {formatDuration(run.durationMs)}
              </Td>
              <Td>
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
              <Td>
                <CopyChip
                  label="run id"
                  value={run._id}
                  testid={`observability-run-copy-${run._id}`}
                >
                  {shortId(run._id, 10)}
                </CopyChip>
              </Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={cn(
        "px-3 py-2 font-semibold",
        align === "right" ? "text-right" : "text-left",
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  align = "left",
  mono,
}: {
  children: React.ReactNode;
  align?: "left" | "right";
  mono?: boolean;
}) {
  return (
    <td
      className={cn(
        "px-3 py-2 text-default",
        align === "right" ? "text-right" : "text-left",
        mono && "font-mono tabular",
      )}
    >
      {children}
    </td>
  );
}
