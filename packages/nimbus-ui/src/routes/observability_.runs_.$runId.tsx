import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../convex/_generated/api";
import { Breadcrumb } from "../components/breadcrumb";
import { CopyChip } from "../components/copy-chip";
import { StateChip } from "../components/state-chip";
import { RelativeTime } from "../components/time";
import { cn } from "../lib/cn";
import { formatAbsoluteTime, formatDuration, shortId } from "../lib/format";

export const Route = createFileRoute("/observability_/runs_/$runId")({
  component: RunDetailPage,
});

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

function RunDetailPage() {
  const { runId } = Route.useParams();
  const run = useQuery(api.runs.byId, {
    id: runId as never,
  }) as RunDoc | null | undefined;

  const events = useQuery(api.events.recent, {
    source: null,
    level: null,
    category: null,
    correlationId: runId,
    limit: 200,
  }) as EventDoc[] | undefined;

  const sortedEvents = useMemo(() => {
    return (events ?? [])
      .slice()
      .sort((a, b) => (a.createdAt ?? 0) - (b.createdAt ?? 0));
  }, [events]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-run-detail"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={[
            { label: "observability", href: "/observability" },
            { label: "runs", href: "/observability" },
            {
              label: shortId(runId, 12),
              copyValue: runId,
              copyLabel: "run id",
              active: true,
            },
          ]}
          testid="run-detail-breadcrumb"
        />
        <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
          Run detail
        </h1>
      </header>

      {run === undefined ? (
        <Loading label="Loading run…" />
      ) : run === null ? (
        <Missing runId={runId} />
      ) : (
        <RunDetailBody
          run={run}
          runId={runId}
          events={sortedEvents}
          eventsLoading={events === undefined}
        />
      )}
    </section>
  );
}

function RunDetailBody({
  run,
  runId,
  events,
  eventsLoading,
}: {
  run: RunDoc;
  runId: string;
  events: EventDoc[];
  eventsLoading: boolean;
}) {
  const startedAt = run.startedAt ?? run._creationTime;
  const duration = run.durationMs ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto pr-1">
      <Summary run={run} runId={runId} />
      <TraceWaterfall
        startedAt={startedAt}
        duration={duration}
        events={events}
      />
      <CorrelatedEvents events={events} loading={eventsLoading} runId={runId} />
      {run.error ? <ErrorPanel error={run.error} /> : null}
    </div>
  );
}

function Summary({ run, runId }: { run: RunDoc; runId: string }) {
  const startedAt = run.startedAt ?? run._creationTime;
  return (
    <div
      className="grid grid-cols-2 gap-x-6 gap-y-3 rounded-md border border-app bg-surface p-4 md:grid-cols-4"
      data-testid="run-detail-summary"
    >
      <Field label="Function" testid="run-detail-function">
        <span className="font-mono text-sm text-default">
          {run.functionPath ?? "—"}
        </span>
      </Field>
      <Field label="Status" testid="run-detail-status">
        <StateChip state={run.status} />
      </Field>
      <Field label="Kind" testid="run-detail-kind">
        <span className="font-mono text-xs uppercase tracking-wide text-muted">
          {run.kind ?? "—"}
        </span>
      </Field>
      <Field label="Duration" testid="run-detail-duration">
        <span className="font-mono tabular text-sm text-default">
          {formatDuration(run.durationMs)}
        </span>
      </Field>
      <Field label="Run id" testid="run-detail-id">
        <CopyChip label="run id" value={runId} testid="run-detail-id-copy">
          {shortId(runId, 14)}
        </CopyChip>
      </Field>
      <Field label="Bundle" testid="run-detail-bundle">
        {run.bundleId ? (
          <CopyChip
            label="bundle id"
            value={run.bundleId}
            testid="run-detail-bundle-copy"
          >
            {shortId(run.bundleId, 12)}
          </CopyChip>
        ) : (
          <span className="tabular text-muted">—</span>
        )}
      </Field>
      <Field label="Started" testid="run-detail-started">
        {typeof startedAt === "number" ? (
          <span
            className="font-mono tabular text-xs text-default"
            title={formatAbsoluteTime(startedAt)}
          >
            <RelativeTime epochMs={startedAt} />
          </span>
        ) : (
          <span className="tabular text-muted">—</span>
        )}
      </Field>
      <Field label="Correlation" testid="run-detail-correlation">
        <CopyChip
          label="correlation id"
          value={runId}
          testid="run-detail-correlation-copy"
        >
          {shortId(runId, 14)}
        </CopyChip>
      </Field>
    </div>
  );
}

function Field({
  label,
  testid,
  children,
}: {
  label: string;
  testid: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1" data-testid={testid}>
      <span className="text-[10px] uppercase tracking-wide text-muted">
        {label}
      </span>
      {children}
    </div>
  );
}

function TraceWaterfall({
  startedAt,
  duration,
  events,
}: {
  startedAt: number | undefined;
  duration: number | null;
  events: EventDoc[];
}) {
  const spans = useMemo(() => {
    if (typeof startedAt !== "number")
      return [] as Array<{
        id: string;
        label: string;
        offsetMs: number;
        level?: string;
      }>;
    return events
      .filter((e) => typeof e.createdAt === "number")
      .map((e) => ({
        id: e._id,
        label: e.message ?? e.category ?? e.source ?? "event",
        offsetMs: Math.max(0, (e.createdAt ?? 0) - startedAt),
        level: e.level,
      }));
  }, [events, startedAt]);

  const total =
    duration ?? (spans.length > 0 ? spans[spans.length - 1].offsetMs + 1 : 0);

  if (typeof startedAt !== "number") {
    return (
      <Panel
        title="Trace timing"
        testid="run-detail-trace"
        empty="Trace timing requires a startedAt timestamp on the run record."
      />
    );
  }

  return (
    <div
      className="rounded-md border border-app bg-surface p-4"
      data-testid="run-detail-trace"
    >
      <div className="mb-3 flex items-baseline justify-between">
        <h2 className="font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
          Trace timing
        </h2>
        <span className="font-mono tabular text-xs text-muted">
          {formatDuration(total)} total
        </span>
      </div>
      <div className="space-y-2">
        <WaterfallBar
          label="run"
          offsetMs={0}
          widthMs={total}
          total={total}
          tone="ok"
          testid="run-detail-trace-bar"
        />
        {spans.length === 0 ? (
          <p
            className="font-mono text-xs text-muted"
            data-testid="run-detail-trace-empty"
          >
            No correlated events yet — only the run span is shown.
          </p>
        ) : (
          spans.map((span) => (
            <WaterfallBar
              key={span.id}
              label={span.label}
              offsetMs={span.offsetMs}
              widthMs={Math.max(2, total * 0.02)}
              total={total}
              tone={
                (span.level ?? "info").toLowerCase() === "error"
                  ? "error"
                  : "muted"
              }
              testid={`run-detail-trace-span-${span.id}`}
            />
          ))
        )}
      </div>
    </div>
  );
}

function WaterfallBar({
  label,
  offsetMs,
  widthMs,
  total,
  tone,
  testid,
}: {
  label: string;
  offsetMs: number;
  widthMs: number;
  total: number;
  tone: "ok" | "muted" | "error";
  testid: string;
}) {
  const safeTotal = total > 0 ? total : 1;
  const leftPct = Math.min(100, Math.max(0, (offsetMs / safeTotal) * 100));
  const widthPct = Math.min(
    100 - leftPct,
    Math.max(0.5, (widthMs / safeTotal) * 100),
  );
  const tones: Record<typeof tone, string> = {
    ok: "bg-[color-mix(in_oklch,var(--color-success)_70%,transparent)]",
    muted: "bg-[color-mix(in_oklch,var(--color-muted)_50%,transparent)]",
    error: "bg-[color-mix(in_oklch,var(--color-danger)_75%,transparent)]",
  };
  return (
    <div
      className="grid grid-cols-[10rem_1fr_5rem] items-center gap-3 font-mono text-[11px]"
      data-testid={testid}
    >
      <span className="truncate text-default" title={label}>
        {label}
      </span>
      <div className="relative h-3 rounded-full bg-surface-2">
        <div
          className={cn("absolute top-0 h-3 rounded-full", tones[tone])}
          style={{ left: `${leftPct}%`, width: `${widthPct}%` }}
        />
      </div>
      <span className="tabular text-muted text-right">
        {offsetMs === 0 ? "0ms" : `+${formatDuration(offsetMs)}`}
      </span>
    </div>
  );
}

function CorrelatedEvents({
  events,
  loading,
  runId,
}: {
  events: EventDoc[];
  loading: boolean;
  runId: string;
}) {
  return (
    <div
      className="rounded-md border border-app bg-surface"
      data-testid="run-detail-events"
    >
      <div className="flex items-baseline justify-between border-b border-app px-4 py-3">
        <h2 className="font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
          Correlated events
        </h2>
        <Link
          to="/observability"
          search={{ tab: "logs", correlationId: runId }}
          className="font-mono text-[10px] uppercase tracking-wide text-muted hover:text-default focus-visible:text-default"
          data-testid="run-detail-open-logs"
        >
          open in logs →
        </Link>
      </div>
      {loading ? (
        <div
          className="px-4 py-6 font-mono text-xs text-muted"
          data-testid="run-detail-events-loading"
        >
          Loading events…
        </div>
      ) : events.length === 0 ? (
        <div
          className="px-4 py-6 font-mono text-xs text-muted"
          data-testid="run-detail-events-empty"
        >
          No events recorded for this run.
        </div>
      ) : (
        <ul className="divide-y divide-app">
          {events.map((event) => (
            <li
              key={event._id}
              className="grid grid-cols-[auto_auto_auto_1fr] items-baseline gap-2 px-4 py-1.5 text-xs"
              data-testid={`run-detail-event-${event._id}`}
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
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ErrorPanel({ error }: { error: unknown }) {
  let body: string;
  try {
    body = typeof error === "string" ? error : JSON.stringify(error, null, 2);
  } catch {
    body = String(error);
  }
  return (
    <div
      className="rounded-md border border-danger bg-surface p-4"
      data-testid="run-detail-error"
    >
      <h2 className="mb-2 font-mono text-[11px] uppercase tracking-[0.14em] text-danger">
        Error
      </h2>
      <pre className="overflow-auto font-mono text-xs text-default whitespace-pre-wrap">
        {body}
      </pre>
    </div>
  );
}

function Panel({
  title,
  testid,
  empty,
}: {
  title: string;
  testid: string;
  empty: string;
}) {
  return (
    <div
      className="rounded-md border border-app bg-surface p-4"
      data-testid={testid}
    >
      <h2 className="mb-2 font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
        {title}
      </h2>
      <p className="font-mono text-xs text-muted">{empty}</p>
    </div>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div
      className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
      data-testid="run-detail-loading"
    >
      {label}
    </div>
  );
}

function Missing({ runId }: { runId: string }) {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 rounded-md border border-app bg-surface px-6 py-10 text-center"
      data-testid="run-detail-missing"
    >
      <p className="font-mono text-sm text-default">Run not found</p>
      <p className="max-w-md text-xs text-muted">
        No run with id <code className="font-mono text-default">{runId}</code>.
        It may have been pruned, or the correlation id does not point to a run
        record.
      </p>
      <Link
        to="/observability"
        search={{ tab: "runs" }}
        className="mt-2 rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
        data-testid="run-detail-back"
      >
        ← all runs
      </Link>
    </div>
  );
}
