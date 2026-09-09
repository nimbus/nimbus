import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";
import { resolveStateKind, type StateKind, statePalette } from "./state-dot";

// Pill reports a lifecycle state: a run that completed, a deploy that
// failed, a schedule that is paused. Every pill shares one recipe (tint
// background, solid text, full radius, 12px medium) so a reader learns it
// once and reads it everywhere. A dot reports liveness; see state-dot.tsx.
export type PillTone = "success" | "warning" | "error" | "info" | "neutral";

const TONES: Record<PillTone, string> = {
  success: "bg-success-tint text-success",
  warning: "bg-warning-tint text-warning",
  error: "bg-error-tint text-error",
  info: "bg-info-tint text-text-2",
  neutral: "bg-bg-raised text-text-3",
};

export function Pill({
  tone,
  className,
  ...props
}: { tone: PillTone } & ComponentProps<"span">) {
  return (
    <span
      data-slot="pill"
      data-tone={tone}
      className={cn(
        "inline-flex h-5 items-center whitespace-nowrap rounded-full px-2 text-xs font-medium",
        TONES[tone],
        className,
      )}
      {...props}
    />
  );
}

const TONE_OF_TOKEN: Record<string, PillTone> = {
  "--success": "success",
  "--warning": "warning",
  "--error": "error",
  "--info": "info",
};

// toneOfKind derives a pill's tone from the shared state palette, so a
// state paints the same hue as a dot or the sidebar upgrade row would.
export function toneOfKind(kind: StateKind): PillTone {
  return TONE_OF_TOKEN[statePalette[kind].token] ?? "neutral";
}

// StatePill renders a server's state word as a lifecycle pill. The label is
// the word as the server spelled it; the tone and `data-state` come from the
// resolved kind. A missing state renders the dash.
export function StatePill({
  state,
  className,
  ...props
}: { state: string | null | undefined } & Omit<
  ComponentProps<"span">,
  "children"
>) {
  const kind = resolveStateKind(state);
  return (
    <Pill
      tone={toneOfKind(kind)}
      data-state={kind}
      data-glyph={statePalette[kind].glyph}
      className={cn(
        "tabular",
        statePalette[kind].strike && "line-through decoration-from-font",
        className,
      )}
      {...props}
    >
      {state ?? "—"}
    </Pill>
  );
}

// CategoryPill names what a thing is (a function kind, an adapter, a storage
// backend), not how it is doing, so it is always the neutral tone. Routing a
// category through StatePill is the bug this component exists to prevent:
// `kind: "query"` matches no state and would render as unknown.
export function CategoryPill({
  value,
  className,
  ...props
}: { value: string | null | undefined } & Omit<
  ComponentProps<"span">,
  "children"
>) {
  const label = value && value.length > 0 ? value : "unknown";
  return (
    <Pill
      tone="neutral"
      data-category={label.toLowerCase()}
      className={className}
      {...props}
    >
      {label}
    </Pill>
  );
}
