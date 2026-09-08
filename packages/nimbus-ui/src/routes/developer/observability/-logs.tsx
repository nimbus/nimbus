import { useQuery } from "@nimbus/nimbus/react";
import { Link, useNavigate } from "@tanstack/react-router";
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
import { toast } from "sonner";

import { api } from "../../../../convex/_generated/api";
import { EmptyState } from "../../../components/empty-state";
import {
  FacetBar,
  FacetButton,
  FacetInput,
  FacetToggle,
} from "../../../components/facet-bar";
import { LoadingState } from "../../../components/loading-state";
import { callFunctionCommand } from "../../../components/onboarding/next-action";
import { CategoryPill, StatePill } from "../../../components/pill";
import { Select } from "../../../components/select";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../../components/storage/row-context-menu";
import { Td, Th } from "../../../components/table-cells";
import { RelativeTime } from "../../../components/time";
import { useServerUrl } from "../../../hooks/use-server-url";
import { formatDuration, shortId } from "../../../lib/format";
import {
  ALL_OPTION,
  type ObservabilityTabProps,
  SystemLensButton,
  TenantFacet,
  TenantScopeNote,
} from "./-facets";
import { groupLogs, type LogGroup } from "./-log-groups";
import {
  type EventDoc,
  hasLineFilters,
  type ObservabilitySearch,
  type RunDoc,
} from "./-types";

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

// The perf harness (tests/perf/log-stream.spec.ts) feeds the stream through
// a window-level store instead of the server, so the render cost of the
// stream is measured on its own.
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

function isAlarm(level: string | undefined): boolean {
  const l = (level ?? "").toLowerCase();
  return l === "error" || l === "warn";
}

export function LogsTab({
  search,
  tenantId,
  allowAllTenants,
  setSearch,
  setSearchAction,
}: ObservabilityTabProps) {
  const live = useQuery(api.events.recent, {
    tenantId,
    source: search.source ?? null,
    level: search.level ?? null,
    category: search.category ?? null,
    correlationId: search.correlationId ?? null,
    limit: 200,
  }) as EventDoc[] | undefined;
  const runs = useQuery(api.runs.recent, {
    tenantId,
    bundleId: null,
    functionPath: null,
    status: null,
    limit: 200,
  }) as RunDoc[] | undefined;
  const perf = usePerfEventStream();
  const events = perf ?? live;

  const follow = search.follow ?? false;
  const pauseOnError = search.pauseOnError ?? false;

  // `undefined` travels the whole way to the stream rather than being
  // collapsed here. Both reads are undefined until they land, and again
  // after every facet change re-keys them, so flattening to `[]` would
  // report a pending read as an empty log.
  const sorted = useMemo(() => {
    if (events === undefined) return undefined;
    return events
      .slice()
      .sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));
  }, [events]);

  // Pause on error freezes the stream at the newest alarm line: lines and
  // runs newer than it are held back until the reader resumes, so the
  // failure stays at the top while they read it.
  const pausedAtRef = useRef<{ id: string; at: number } | null>(null);
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (!pauseOnError) {
      setPaused(false);
      pausedAtRef.current = null;
      return;
    }
    const newest = sorted?.[0];
    if (!newest) return;
    if (isAlarm(newest.level) && pausedAtRef.current?.id !== newest._id) {
      pausedAtRef.current = { id: newest._id, at: newest.createdAt ?? 0 };
      setPaused(true);
    }
  }, [pauseOnError, sorted]);

  const lineFiltered =
    search.level !== undefined ||
    search.category !== undefined ||
    search.source !== undefined;

  const groups = useMemo(() => {
    if (sorted === undefined || runs === undefined) return undefined;
    const frozen = paused ? pausedAtRef.current : null;
    const visibleEvents = frozen
      ? sorted.filter((e) => (e.createdAt ?? 0) <= frozen.at)
      : sorted;
    const visibleRuns = frozen
      ? runs.filter((r) => (r.startedAt ?? 0) <= frozen.at)
      : runs;
    return groupLogs(visibleRuns, visibleEvents, {
      correlationId: search.correlationId,
      lineFiltered,
    });
  }, [lineFiltered, paused, runs, search.correlationId, sorted]);

  const clearFilters = useCallback(
    () =>
      setSearchAction({
        level: undefined,
        category: undefined,
        source: undefined,
        correlationId: undefined,
      }),
    [setSearchAction],
  );

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-logs"
    >
      <LogFacetBar
        search={search}
        tenantId={tenantId}
        allowAllTenants={allowAllTenants}
        setSearch={setSearch}
        follow={follow}
        pauseOnError={pauseOnError}
        paused={paused}
        onResume={() => {
          setPaused(false);
          pausedAtRef.current = null;
        }}
        onClear={clearFilters}
      />
      <TenantScopeNote />
      <LogStream
        groups={groups}
        follow={follow}
        paused={paused}
        filtered={hasLineFilters(search)}
        tenantId={tenantId}
        setSearch={setSearch}
        onClear={clearFilters}
      />
    </div>
  );
}

function LogFacetBar({
  search,
  tenantId,
  allowAllTenants,
  setSearch,
  follow,
  pauseOnError,
  paused,
  onResume,
  onClear,
}: Pick<
  ObservabilityTabProps,
  "search" | "tenantId" | "allowAllTenants" | "setSearch"
> & {
  follow: boolean;
  pauseOnError: boolean;
  paused: boolean;
  onResume: () => void;
  onClear: () => void;
}) {
  return (
    <FacetBar
      label="Log facets"
      testid="observability-log-filters"
      trailing={
        <>
          {paused ? (
            <FacetButton
              tone="danger"
              onClick={onResume}
              testid="observability-log-resume"
            >
              paused · resume
            </FacetButton>
          ) : null}
          <FacetToggle
            id="follow-mode"
            label="Follow"
            value={follow}
            onChange={(v) => setSearch({ follow: v ? true : undefined })}
            testid="observability-log-follow"
          />
          <FacetToggle
            id="pause-on-error"
            label="Pause on error"
            value={pauseOnError}
            onChange={(v) => setSearch({ pauseOnError: v ? true : undefined })}
            testid="observability-log-pause-on-error"
          />
          <FacetButton onClick={onClear} testid="observability-filter-clear">
            clear
          </FacetButton>
          <SystemLensButton />
        </>
      }
    >
      <TenantFacet
        tenantId={tenantId}
        allowAllTenants={allowAllTenants}
        setSearch={setSearch}
      />
      <Select
        label="Level"
        value={search.level ?? ALL_OPTION}
        options={[
          { value: ALL_OPTION, label: "all levels" },
          ...LEVELS.map((l) => ({ value: l, label: l })),
        ]}
        onChange={(v) => setSearch({ level: v === ALL_OPTION ? undefined : v })}
        testid="observability-filter-level"
      />
      <FacetInput
        id="log-category"
        label="Category"
        value={search.category ?? ""}
        placeholder="category"
        onChange={(v) => setSearch({ category: v || undefined })}
        testid="observability-filter-category"
      />
      <FacetInput
        id="log-source"
        label="Source"
        value={search.source ?? ""}
        placeholder="source"
        onChange={(v) => setSearch({ source: v || undefined })}
        testid="observability-filter-source"
      />
      <FacetInput
        id="log-correlation"
        label="Correlation"
        value={search.correlationId ?? ""}
        placeholder="run id"
        onChange={(v) => setSearch({ correlationId: v || undefined })}
        testid="observability-filter-correlation"
      />
    </FacetBar>
  );
}

type MenuState = {
  x: number;
  y: number;
  runId: string;
  element: HTMLElement | null;
};

function groupsVersion(groups: LogGroup[] | undefined): string {
  if (groups === undefined) return "loading";
  const first = groups[0];
  const last = groups.at(-1);
  const lines = groups.reduce((n, g) => n + g.events.length, 0);
  return `${groups.length}:${lines}:${first?.id ?? ""}:${first?.events[0]?._id ?? ""}:${last?.id ?? ""}`;
}

/**
 * Three states, three treatments, one frame. `undefined` is a read that has
 * not answered, `[]` is an answer, and the two must not render the same node:
 * a slow backend used to be reported as an empty log, blaming filters the
 * reader may never have set.
 *
 * The scroll container is mounted in every state and holds the ref the follow
 * and anchor effects read, so the box, its border and its scroll position are
 * the same object across the swap and nothing moves when the first batch
 * lands.
 */
function LogStream({
  groups,
  follow,
  paused,
  filtered,
  tenantId,
  setSearch,
  onClear,
}: {
  groups: LogGroup[] | undefined;
  follow: boolean;
  paused: boolean;
  filtered: boolean;
  tenantId: string | null;
  setSearch: (patch: Partial<ObservabilitySearch>) => void;
  onClear: () => void;
}) {
  const navigate = useNavigate();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const scrollAnchorRef = useRef<{
    top: number;
    height: number;
    version: string;
  } | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const version = useMemo(() => groupsVersion(groups), [groups]);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (follow && !paused) {
      el.scrollTop = 0;
      return;
    }
    const anchor = scrollAnchorRef.current;
    if (!anchor) return;
    if (anchor.version === version) return;
    const delta = el.scrollHeight - anchor.height;
    if (delta > 0) {
      el.scrollTop = anchor.top + delta;
    }
  }, [version, follow, paused]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    scrollAnchorRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      version,
    };
  }, [version]);

  const openMenu = (e: ReactMouseEvent<HTMLElement>, runId: string) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, runId, element: e.currentTarget });
  };

  const menuItems: RowMenuItem[] = menu
    ? [
        {
          id: "open-run",
          label: "Open run",
          hint: shortId(menu.runId, 8),
          onSelect: () =>
            void navigate({
              to: "/developer/compute/runs/$runId",
              params: { runId: menu.runId },
            }),
        },
        {
          id: "only-this-run",
          label: "Only this run",
          onSelect: () => setSearch({ correlationId: menu.runId }),
        },
        {
          id: "copy-id",
          label: "Copy run id",
          onSelect: () => {
            void navigator.clipboard
              .writeText(menu.runId)
              .then(() => toast("Copied run id", { description: menu.runId }))
              .catch(() => toast.error("Failed to copy run id"));
          },
        },
      ]
    : [];

  return (
    <div
      ref={containerRef}
      className="min-h-0 flex-1 overflow-auto rounded-md border border-border-2 bg-bg-panel"
      data-testid="observability-log-stream"
    >
      {groups === undefined ? (
        <LoadingState
          label="Loading runs and log lines…"
          testid="observability-log-loading"
        />
      ) : groups.length === 0 ? (
        <LogEmptyState
          filtered={filtered}
          tenantId={tenantId}
          onClear={onClear}
        />
      ) : (
        // Fixed tracks, not per-row intrinsic sizing: a log reader scans down a
        // constant left edge, so time / level / source / message / run must
        // start at the same x on every line regardless of that line's content.
        <table
          className="w-full table-fixed border-collapse text-xs"
          data-testid="observability-log-table"
        >
          <colgroup>
            <col className="w-[88px]" />
            <col className="w-[92px]" />
            <col className="w-[220px]" />
            <col />
            <col className="w-[112px]" />
          </colgroup>
          <thead className="sticky top-0 z-10 bg-bg-raised text-xs font-medium text-text-3">
            <tr className="h-8">
              <Th align="right" className="py-1.5">
                Time
              </Th>
              <Th className="py-1.5">Level</Th>
              <Th className="py-1.5">Source</Th>
              <Th className="py-1.5">Message</Th>
              <Th className="py-1.5">Run</Th>
            </tr>
          </thead>
          {groups.map((group) => (
            <LogGroupBody key={group.id} group={group} onMenu={openMenu} />
          ))}
        </table>
      )}
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label="Log line actions"
          items={menuItems}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="observability-log-context-menu"
        />
      ) : null}
    </div>
  );
}

// One `<tbody>` per group: the head row names the run, the body rows are
// its lines. A run with no lines says so, because a bare head reads as a
// line that failed to render.
function LogGroupBody({
  group,
  onMenu,
}: {
  group: LogGroup;
  onMenu: (e: ReactMouseEvent<HTMLElement>, runId: string) => void;
}) {
  const runId = group.kind === "run" ? group.id : undefined;
  return (
    <tbody
      data-testid={`observability-log-group-${group.id}`}
      data-kind={group.kind}
      className="border-t border-border-2"
    >
      <tr
        className="h-9 bg-bg-raised/60"
        data-testid={`observability-log-group-head-${group.id}`}
      >
        <td colSpan={5} className="px-3 py-1.5">
          {group.kind === "run" ? (
            <RunHead group={group} />
          ) : (
            <ServerHead count={group.events.length} />
          )}
        </td>
      </tr>
      {group.events.length === 0 && group.kind === "run" ? (
        <tr
          className="h-9"
          data-testid={`observability-log-group-empty-${group.id}`}
        >
          <td colSpan={5} className="px-3 py-1.5 font-mono text-text-3">
            No log lines recorded for this run.
          </td>
        </tr>
      ) : null}
      {group.events.map((event) => (
        <LogRow
          key={event._id}
          event={event}
          runId={runId}
          onMenu={runId ? (e) => onMenu(e, runId) : undefined}
        />
      ))}
    </tbody>
  );
}

function RunHead({ group }: { group: LogGroup & { kind: "run" } }) {
  const { run, id, events } = group;
  const startedAt = run?.startedAt ?? run?._creationTime ?? group.at;
  return (
    <div className="flex min-w-0 items-center gap-2">
      <StatePill state={run?.status ?? "unknown"} />
      <span
        className="min-w-0 truncate font-mono text-text-1"
        title={run?.functionPath ?? id}
      >
        {run?.functionPath ?? shortId(id, 12)}
      </span>
      {run?.kind ? <CategoryPill value={run.kind} /> : null}
      {run ? (
        <span className="tabular text-text-3">
          {formatDuration(run.durationMs)}
        </span>
      ) : null}
      <RelativeTime epochMs={startedAt} />
      <span className="tabular text-text-3">
        {events.length === 1 ? "1 line" : `${events.length} lines`}
      </span>
      <Link
        to="/developer/compute/runs/$runId"
        params={{ runId: id }}
        className="ml-auto shrink-0 rounded-xs border border-border-2 px-1.5 py-0.5 text-xs font-medium text-text-3 hover:bg-bg-raised hover:text-text-1 focus-visible:bg-bg-raised focus-visible:text-text-1"
        data-testid={`observability-log-group-open-${id}`}
      >
        Open run ↗
      </Link>
    </div>
  );
}

function ServerHead({ count }: { count: number }) {
  return (
    <div className="flex items-center gap-2">
      <CategoryPill value="server" />
      <span className="text-text-3">Lines that belong to no run</span>
      <span className="tabular text-text-3">
        {count === 1 ? "1 line" : `${count} lines`}
      </span>
    </div>
  );
}

function LogRow({
  event,
  runId,
  onMenu,
}: {
  event: EventDoc;
  runId: string | undefined;
  onMenu?: (e: ReactMouseEvent<HTMLElement>) => void;
}) {
  const source = `${event.source ?? "—"}${event.category ? ` · ${event.category}` : ""}`;
  const message = event.message ?? "(no message)";
  return (
    <tr
      onContextMenu={onMenu}
      aria-label={`Log entry${runId ? `, run ${shortId(runId, 8)}` : ""}: ${event.message ?? ""}`}
      data-testid={`observability-log-row-${event._id}`}
      // h-9 pins every row at the dense band's 36px. Cells truncate rather
      // than wrap, so the height is exact, not a minimum that a long source
      // or message can push past.
      className="h-9 border-t border-border-2 hover:bg-bg-raised"
    >
      <Td align="right" className="whitespace-nowrap py-1.5">
        <RelativeTime epochMs={event.createdAt ?? event._creationTime ?? 0} />
      </Td>
      <Td className="py-1.5">
        <StatePill state={event.level ?? "info"} />
      </Td>
      <Td className="py-1.5">
        <span
          title={source}
          className="block truncate text-xs font-medium text-text-3"
        >
          {source}
        </span>
      </Td>
      <Td className="py-1.5">
        <span title={message} className="block truncate font-mono text-text-1">
          {message}
        </span>
      </Td>
      <Td className="py-1.5">
        {runId ? (
          <Link
            to="/developer/compute/runs/$runId"
            params={{ runId }}
            className="inline-flex items-center gap-1 rounded-xs border border-border-2 px-1.5 py-0.5 text-xs font-medium text-text-3 hover:bg-bg-raised hover:text-text-1 focus-visible:bg-bg-raised focus-visible:text-text-1"
            data-testid={`observability-log-jump-${event._id}`}
            aria-label={`Jump to run ${runId}`}
            title={`Jump to run ${runId}`}
          >
            <span>↗</span>
            <span>{shortId(runId, 6)}</span>
          </Link>
        ) : (
          <span className="tabular text-text-3">—</span>
        )}
      </Td>
    </tr>
  );
}

/**
 * The two empty results the stream can produce. "Nothing matches the current
 * filters" is only true when facets are set; on a fresh deployment it named a
 * cause the reader could not act on and pointed at controls they never
 * touched.
 */
function LogEmptyState({
  filtered,
  tenantId,
  onClear,
}: {
  filtered: boolean;
  tenantId: string | null;
  onClear: () => void;
}) {
  const serverUrl = useServerUrl();
  if (filtered) {
    return (
      <EmptyState
        title="Nothing matches the current filters"
        body="Level, category, source, and correlation narrow the same stream, so a line has to satisfy every one that is set. Clear them to see every run."
        cta={{ label: "Clear filters", onClick: onClear }}
        testid="observability-log-empty"
      />
    );
  }
  return (
    <EmptyState
      title="No runs or log lines yet"
      body="Every query, mutation, and action is a run, and the lines it writes sit under it. Deploy an app with nimbus dev, then call a function; the run appears here without a reload."
      snippet={callFunctionCommand({
        serverUrl,
        tenant: tenantId,
        functionPath: null,
      })}
      testid="observability-log-empty"
    />
  );
}
