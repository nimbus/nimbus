import { Link, useNavigate } from "@tanstack/react-router";
import { useQuery } from "@nimbus/nimbus/react";
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

import { api } from "../../../../convex/_generated/api";
import { StateChip } from "../../../components/state-chip";
import { RelativeTime } from "../../../components/time";
import { cn } from "../../../lib/cn";
import { shortId } from "../../../lib/format";
import { FilterInput, FilterSelect } from "./-filters";
import type { EventDoc, ObservabilitySearch } from "./-types";

const LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

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

export function LogsTab({ search }: { search: ObservabilitySearch }) {
  const navigate = useNavigate({ from: "/developer/observability" });
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
        to: "/developer/observability",
        search: (prev) => ({ ...prev, ...patch }),
        replace: true,
      });
    },
    [navigate],
  );

  const setSearchAction = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to: "/developer/observability",
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
            to="/developer/compute/runs/$runId"
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
        to="/developer/compute/runs/$runId"
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
