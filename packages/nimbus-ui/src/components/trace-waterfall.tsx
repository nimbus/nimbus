import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { formatDuration } from "../lib/format";
import type { RunSpan } from "../routes/developer/observability/-types";
import { resolveStateKind, statePalette } from "./state-dot";

// TraceWaterfall draws one run's spans on a shared time axis. The server
// records the spans with the run row (crates/nimbus-system/src/records/
// trace.rs): the function's own span first, then one span per host call,
// each naming its parent by index. The run page, the run sheet, and the
// Traces tab all draw the same component, so a trace reads the same
// everywhere.
//
// A run recorded before spans existed, or one whose trace was dropped,
// still gets its own bar from `durationMs`, and the panel says the spans
// are missing instead of showing an empty axis.
export function TraceWaterfall({
  spans,
  status,
  durationMs,
  testid,
  className,
}: {
  spans: readonly RunSpan[] | undefined;
  status: string | null | undefined;
  durationMs: number | null | undefined;
  testid: string;
  className?: string;
}) {
  const rows = useMemo(() => layoutSpans(spans ?? []), [spans]);
  const total = Math.max(
    durationMs ?? 0,
    ...rows.map((row) => row.span.startMs + row.span.durationMs),
    0,
  );
  return (
    <div
      className={cn(
        "rounded-md border border-border-2 bg-bg-panel p-4",
        className,
      )}
      data-testid={testid}
    >
      <div className="mb-3 flex items-baseline justify-between">
        <h2 className="text-xs font-medium text-text-3">Trace</h2>
        <span className="font-mono tabular text-xs text-text-3">
          {formatDuration(total)} total · {spans?.length ?? 0} span
          {spans?.length === 1 ? "" : "s"}
        </span>
      </div>
      <div className="space-y-2">
        {/* The run's own bar reports the run's own status, so a failed run
            never paints itself success-green above the spans that failed
            it. */}
        <WaterfallBar
          label="run"
          depth={0}
          offsetMs={0}
          widthMs={total}
          total={total}
          tone={toneForState(status)}
          state={status}
          testid={`${testid}-bar`}
        />
        {rows.length === 0 ? (
          <p
            className="font-mono text-xs text-text-3"
            data-testid={`${testid}-empty`}
          >
            No spans were recorded for this run. Runs record a span per host
            call; this row predates span recording or its spans were dropped.
          </p>
        ) : (
          rows.map((row) => (
            <WaterfallBar
              key={row.index}
              label={row.span.name}
              kind={row.span.kind}
              depth={row.depth}
              offsetMs={row.span.startMs}
              widthMs={row.span.durationMs}
              total={total}
              // A child span that succeeded is the absence of a problem,
              // not a result to celebrate: it draws muted, and only a
              // failed span carries the ✗ glyph.
              tone={row.span.status === "error" ? "error" : "muted"}
              state={row.span.status}
              testid={`${testid}-span-${row.index}`}
            />
          ))
        )}
      </div>
    </div>
  );
}

type SpanRow = { index: number; span: RunSpan; depth: number };

// The spans in recorded order with the nesting depth each parent chain
// gives them. The function's own span (index 0, no parent) is the run bar,
// so it is not repeated as a row; its children start at depth 0.
export function layoutSpans(spans: readonly RunSpan[]): SpanRow[] {
  const depths: number[] = [];
  const rows: SpanRow[] = [];
  spans.forEach((span, index) => {
    const parent = span.parent;
    const parentDepth =
      parent !== null && parent >= 0 && parent < index ? depths[parent] : -1;
    const depth = parentDepth + 1;
    depths[index] = depth;
    if (index === 0 && parent === null) return;
    rows.push({ index, span, depth: Math.max(0, depth - 1) });
  });
  return rows;
}

type WaterfallTone = "ok" | "muted" | "error";

const toneFills: Record<WaterfallTone, string> = {
  ok: "bg-[color-mix(in_oklch,var(--success)_70%,transparent)]",
  muted: "bg-[color-mix(in_oklch,var(--text-3)_50%,transparent)]",
  error: "bg-[color-mix(in_oklch,var(--error)_75%,transparent)]",
};

/**
 * A bar's fill is a status color, and DESIGN.md rules that color is never the
 * only signal. Each status tone therefore also carries a glyph the eye can
 * separate by *shape* — ✓ against ✗, the way `StatePill` separates its states
 * — and that glyph names its state to assistive tech. `muted` is the absence
 * of a status rather than a status, so it claims neither a glyph nor a name.
 *
 * A full `StatePill` per row was rejected: the waterfall is a dense trace and
 * a state word on every row would out-weigh the bars it annotates.
 */
const toneMarkers: Record<
  WaterfallTone,
  { glyph: string; token: string } | null
> = {
  ok: { glyph: "✓", token: "--success" },
  muted: null,
  error: { glyph: "✗", token: "--error" },
};

/**
 * Resolve any state string the server can write — a run `status`, a span
 * `status` — onto the three bar tones, through the same palette the chips
 * read. A state the palette calls danger can then never paint a success bar.
 */
export function toneForState(state: string | null | undefined): WaterfallTone {
  const { token } = statePalette[resolveStateKind(state)];
  if (token === "--error") return "error";
  if (token === "--success") return "ok";
  return "muted";
}

function WaterfallBar({
  label,
  kind,
  depth,
  offsetMs,
  widthMs,
  total,
  tone,
  state,
  testid,
}: {
  label: string;
  kind?: string;
  depth: number;
  offsetMs: number;
  widthMs: number;
  total: number;
  tone: WaterfallTone;
  state: string | null | undefined;
  testid: string;
}) {
  const safeTotal = total > 0 ? total : 1;
  const leftPct = Math.min(100, Math.max(0, (offsetMs / safeTotal) * 100));
  const widthPct = Math.min(
    100 - leftPct,
    Math.max(0.5, (widthMs / safeTotal) * 100),
  );
  const marker = toneMarkers[tone];
  return (
    <div
      className="grid grid-cols-[minmax(10rem,14rem)_1fr_6rem] items-center gap-3 font-mono text-xs"
      data-testid={testid}
      data-depth={depth}
    >
      {/* The glyph sits outside the truncating span so a long label can never
          clip the row's only non-color signal. */}
      <span
        className="flex min-w-0 items-center gap-1.5"
        style={{ paddingLeft: `${depth * 0.75}rem` }}
        title={`${label} · +${formatDuration(offsetMs)}`}
      >
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
        {kind ? (
          <span className="shrink-0 text-text-3" data-testid={`${testid}-kind`}>
            {kind}
          </span>
        ) : null}
        <span className="truncate text-text-1">{label}</span>
      </span>
      <div className="relative h-3 rounded-full bg-bg-raised">
        <div
          className={cn("absolute top-0 h-3 rounded-full", toneFills[tone])}
          style={{ left: `${leftPct}%`, width: `${widthPct}%` }}
        />
      </div>
      <span className="tabular text-text-3 text-right">
        {formatDuration(widthMs)}
      </span>
    </div>
  );
}
