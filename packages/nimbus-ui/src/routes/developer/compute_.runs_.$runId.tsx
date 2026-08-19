import { createFileRoute, Link, notFound } from "@tanstack/react-router";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import type { Doc, Id } from "../../../convex/_generated/dataModel";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import {
  resolveStateKind,
  StateChip,
  statePalette,
} from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatAbsoluteTime, formatDuration, shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import { locationLine, parseRunError } from "../../lib/run-error";

export const Route = createFileRoute("/developer/compute_/runs_/$runId")({
  loader: async ({ params }) => {
    const client = getNimbusClient();
    const [run, events] = await Promise.all([
      client.query(api.runs.byId, { id: params.runId as Id<"runs"> }),
      client.query(api.events.recent, {
        source: null,
        level: null,
        category: null,
        correlationId: params.runId,
        limit: 200,
      }),
    ]);
    if (!run) throw notFound();
    return { run, events };
  },
  notFoundComponent: RunNotFound,
  component: RunDetailPage,
});

type RunDoc = Doc<"runs">;
type EventDoc = Doc<"events">;

function RunDetailPage() {
  const { runId } = Route.useParams();
  const { run, events } = Route.useLoaderData();

  const sortedEvents = useMemo(() => {
    return events
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

      <RunDetailBody run={run} runId={runId} events={sortedEvents} />
    </section>
  );
}

function RunDetailBody({
  run,
  runId,
  events,
}: {
  run: RunDoc;
  runId: string;
  events: EventDoc[];
}) {
  const startedAt = run.startedAt ?? run._creationTime;
  const duration = run.durationMs ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto pr-1">
      <Summary run={run} runId={runId} />
      <TraceWaterfall
        startedAt={startedAt}
        duration={duration}
        status={run.status}
        events={events}
      />
      <CorrelatedEvents events={events} runId={runId} />
      {run.error ? (
        <ErrorPanel error={run.error} functionPath={run.functionPath} />
      ) : null}
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
      <span className="text-xs uppercase tracking-wide text-muted">
        {label}
      </span>
      {children}
    </div>
  );
}

function TraceWaterfall({
  startedAt,
  duration,
  status,
  events,
}: {
  startedAt: number | undefined;
  duration: number | null;
  status: string | undefined;
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
        <h2 className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
          Trace timing
        </h2>
        <span className="font-mono tabular text-xs text-muted">
          {formatDuration(total)} total
        </span>
      </div>
      <div className="space-y-2">
        {/* The run's own span reports the run's own status. It was pinned to
            `ok`, so a failed run painted itself success-green above the very
            events that failed it. */}
        <WaterfallBar
          label="run"
          offsetMs={0}
          widthMs={total}
          total={total}
          state={status}
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
              state={span.level ?? "info"}
              testid={`run-detail-trace-span-${span.id}`}
            />
          ))
        )}
      </div>
    </div>
  );
}

type WaterfallTone = "ok" | "muted" | "error";

const toneFills: Record<WaterfallTone, string> = {
  ok: "bg-[color-mix(in_oklch,var(--nimbus-success)_70%,transparent)]",
  muted: "bg-[color-mix(in_oklch,var(--nimbus-muted)_50%,transparent)]",
  error: "bg-[color-mix(in_oklch,var(--nimbus-danger)_75%,transparent)]",
};

/**
 * A bar's fill is a status color, and DESIGN.md rules that color is never the
 * only signal. Each status tone therefore also carries a glyph the eye can
 * separate by *shape* — ✓ against ✗, the way `StateChip` separates its states
 * — and that glyph names its state to assistive tech. `muted` is the absence
 * of a status rather than a status, so it claims neither a glyph nor a name.
 *
 * A full `StateChip` per row was rejected: the waterfall is a dense trace and
 * an uppercase state word on every row would out-weigh the bars it annotates.
 */
const toneMarkers: Record<
  WaterfallTone,
  { glyph: string; token: string } | null
> = {
  ok: { glyph: "✓", token: "--nimbus-success" },
  muted: null,
  error: { glyph: "✗", token: "--nimbus-danger" },
};

/**
 * Resolve any state string the server can write — a run `status`, an event
 * `level` — onto the three bar tones, through the same palette the chips
 * read. A state the palette calls danger can then never paint a success bar.
 */
function toneForState(state: string | null | undefined): WaterfallTone {
  const { token } = statePalette[resolveStateKind(state)];
  if (token === "--nimbus-danger") return "error";
  if (token === "--nimbus-success") return "ok";
  return "muted";
}

function WaterfallBar({
  label,
  offsetMs,
  widthMs,
  total,
  state,
  testid,
}: {
  label: string;
  offsetMs: number;
  widthMs: number;
  total: number;
  state: string | null | undefined;
  testid: string;
}) {
  const safeTotal = total > 0 ? total : 1;
  const leftPct = Math.min(100, Math.max(0, (offsetMs / safeTotal) * 100));
  const widthPct = Math.min(
    100 - leftPct,
    Math.max(0.5, (widthMs / safeTotal) * 100),
  );
  const tone = toneForState(state);
  const marker = toneMarkers[tone];
  return (
    <div
      className="grid grid-cols-[10rem_1fr_5rem] items-center gap-3 font-mono text-xs"
      data-testid={testid}
    >
      {/* The glyph sits outside the truncating span so a long label can never
          clip the row's only non-color signal. */}
      <span className="flex min-w-0 items-center gap-1.5" title={label}>
        {marker ? (
          <span
            role="img"
            aria-label={state ?? tone}
            className="shrink-0 leading-none"
            style={{ color: `var(${marker.token})` }}
            data-testid={`${testid}-marker`}
          >
            {marker.glyph}
          </span>
        ) : null}
        <span className="truncate text-default">{label}</span>
      </span>
      <div className="relative h-3 rounded-full bg-surface-2">
        <div
          className={cn("absolute top-0 h-3 rounded-full", toneFills[tone])}
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
  runId,
}: {
  events: EventDoc[];
  runId: string;
}) {
  return (
    <div
      className="rounded-md border border-app bg-surface"
      data-testid="run-detail-events"
    >
      <div className="flex items-baseline justify-between border-b border-app px-4 py-3">
        <h2 className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
          Correlated events
        </h2>
        <Link
          to="/developer/observability"
          search={{ tab: "logs", correlationId: runId }}
          className="font-mono text-xs uppercase tracking-wide text-muted hover:text-default focus-visible:text-default"
          data-testid="run-detail-open-logs"
        >
          open in logs →
        </Link>
      </div>
      {events.length === 0 ? (
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
              <span className="font-mono text-xs uppercase tracking-wide text-muted">
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

function ErrorPanel({
  error,
  functionPath,
}: {
  error: unknown;
  functionPath?: string;
}) {
  const { message, location } = parseRunError(error);
  const line = location ? locationLine(location) : undefined;
  return (
    <div
      className="rounded-md border border-danger bg-surface p-4"
      data-testid="run-detail-error"
    >
      <h2 className="mb-2 font-mono text-xs uppercase tracking-[0.14em] text-danger">
        Error
      </h2>
      {location ? (
        functionPath && line ? (
          <Link
            to="/developer/compute/$function"
            params={{ function: functionPath }}
            search={{ tab: "source", line }}
            className="mb-2 inline-block rounded border border-danger px-2 py-0.5 font-mono text-xs text-danger hover:bg-surface-2"
            data-testid="run-detail-error-location"
          >
            at {location} ↗
          </Link>
        ) : (
          <span
            className="mb-2 inline-block font-mono text-xs text-danger"
            data-testid="run-detail-error-location"
          >
            at {location}
          </span>
        )
      ) : null}
      <pre className="overflow-auto font-mono text-xs text-default whitespace-pre-wrap">
        {message}
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
      <h2 className="mb-2 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        {title}
      </h2>
      <p className="font-mono text-xs text-muted">{empty}</p>
    </div>
  );
}

function RunNotFound() {
  const { runId } = Route.useParams();
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-run-detail"
    >
      <div
        className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 rounded-md border border-app bg-surface px-6 py-10 text-center"
        data-testid="run-detail-missing"
      >
        <p className="font-mono text-sm text-default">Run not found</p>
        <p className="max-w-md text-xs text-muted">
          No run with id <code className="font-mono text-default">{runId}</code>
          . It may have been pruned, or the correlation id does not point to a
          run record.
        </p>
        <Link
          to="/developer/observability"
          search={{ tab: "runs" }}
          className="mt-2 rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
          data-testid="run-detail-back"
        >
          ← all runs
        </Link>
      </div>
    </section>
  );
}
