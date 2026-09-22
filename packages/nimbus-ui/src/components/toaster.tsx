import type { CSSProperties } from "react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { TRANSIENT_TOAST_MS, VISIBLE_TOAST_LIMIT } from "@/components/toast";
import { Toaster as ToastStack, useToastManager } from "@/components/ui/toast";

/**
 * The console's one toast stack, bottom-right, with the cap and the transient
 * lifetime DESIGN.md sets. The stack pauses its clocks while the operator
 * hovers or focuses it and while the window is in the background, so a
 * confirmation is not lost to a glance away. An error has no clock at all;
 * see `toast` in `@/components/toast`.
 */
export function Toaster() {
  return (
    <ToastStack limit={VISIBLE_TOAST_LIMIT} timeout={TRANSIENT_TOAST_MS}>
      <ToastOverflow />
    </ToastStack>
  );
}

/** The registry stack's `--gap` and `--peek`, in rem; it does not export them. */
const STACK_GAP_REM = 0.75;

/**
 * The "+N more" line DESIGN.md asks for.
 *
 * The stack keeps every toast past the cap mounted but transparent and inert.
 * Hovering does not reveal them and they cannot be clicked shut; they surface
 * only as the visible three are dismissed. Errors stay until dismissed, so a
 * fourth failure waits off-stack for as long as the operator leaves the first
 * three alone: a failure report the console holds and never shows. This line
 * is the only evidence that there is anything behind the stack.
 *
 * It is portaled into the viewport because that is where the geometry is.
 * Collapsed, the viewport carries `--toast-frontmost-height` and each toast
 * behind the front one peeks out by `--peek`, so the top of a full stack sits
 * `frontmost + 2 * peek` above the viewport's bottom edge. Expanded (the
 * viewport carries `data-expanded` while hovered or focused), every toast
 * stands at its own height with `--gap` between, and the store reports each
 * height, so the line climbs to the top of that column. Anchoring from
 * outside would mean measuring the stack on every change and still lagging
 * its 500ms transitions.
 */
function ToastOverflow() {
  const { toasts } = useToastManager();
  const [viewport, setViewport] = useState<HTMLElement | null>(null);
  const shown = toasts.filter((entry) => !entry.limited);
  const hidden = toasts.length - shown.length;
  const stackHeight = shown.reduce(
    (sum, entry) => sum + (entry.height ?? 0),
    0,
  );

  useEffect(() => {
    // The viewport is a sibling portal that mounts after this component, so
    // the lookup keys off the count rather than running once at mount.
    if (hidden < 1) {
      setViewport(null);
      return;
    }
    setViewport(
      document.querySelector<HTMLElement>('[data-slot="toast-viewport"]'),
    );
  }, [hidden]);

  if (hidden < 1 || viewport === null) return null;

  return createPortal(
    <div
      role="status"
      data-testid="toast-overflow"
      className="absolute right-0 bottom-(--overflow-offset) rounded-xs border border-border-2 bg-bg-panel px-2 py-0.5 font-mono text-xs text-text-3 transition-all duration-500 in-data-expanded:bottom-(--overflow-offset-expanded)"
      style={
        {
          "--overflow-offset": `calc(var(--toast-frontmost-height) + ${shown.length - 1} * ${STACK_GAP_REM}rem + 8px)`,
          "--overflow-offset-expanded": `calc(${stackHeight}px + ${shown.length - 1} * ${STACK_GAP_REM}rem + 8px)`,
        } as CSSProperties
      }
    >
      +{hidden} more
    </div>,
    viewport,
  );
}
