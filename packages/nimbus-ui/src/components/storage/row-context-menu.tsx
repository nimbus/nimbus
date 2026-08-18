import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { cn } from "../../lib/cn";

export type RowMenuItem = {
  readonly id: string;
  readonly label: string;
  /** Trailing detail — a short id, a shortcut, a value preview. */
  readonly hint?: string;
  readonly danger?: boolean;
  readonly onSelect: () => void;
};

const EDGE = 6;

/**
 * The console's row context menu.
 *
 * DESIGN.md makes right-click a peer of click on every resource row, so this is
 * the one place that behaviour is implemented: a fixed-position menu anchored
 * at a point, closed by Escape / outside click / scroll, with arrow-key
 * navigation and focus returned to the row that opened it.
 *
 * Anchoring is by point rather than by element so the same component serves a
 * pointer right-click (`clientX`/`clientY`) and a keyboard invocation
 * (Shift+F10 / the ContextMenu key), which must anchor to the focused row's
 * bounding box — `clientX`/`clientY` are 0,0 for a keyboard-raised menu.
 */
export function RowContextMenu({
  x,
  y,
  label,
  items,
  restoreFocus,
  onClose,
  testid,
}: {
  x: number;
  y: number;
  label: string;
  items: readonly RowMenuItem[];
  /** Element focus returns to when the menu closes. */
  restoreFocus?: HTMLElement | null;
  onClose: () => void;
  testid?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number }>({
    left: x,
    top: y,
  });

  // Clamp against the viewport once the menu has a measured size, so a menu
  // raised on the last row or the right-hand edge is not cut off.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setPos({
      left: Math.max(EDGE, Math.min(x, window.innerWidth - rect.width - EDGE)),
      top: Math.max(EDGE, Math.min(y, window.innerHeight - rect.height - EDGE)),
    });
  }, [x, y]);

  // Move focus into the menu on open. Without this the menu is unreachable by
  // keyboard and its own key handling never fires.
  useEffect(() => {
    ref.current
      ?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')
      ?.focus();
  }, []);

  useEffect(() => {
    // Dismissal keys off `pointerdown`, not `click`/`contextmenu`, because the
    // menu is opened *by* a `contextmenu` event: React runs the row handler at
    // its root container, flushes the mount and this effect, and the very same
    // event then continues on to `window`, where a `contextmenu` listener would
    // close the menu it just opened. A right-click could therefore never leave
    // a menu on screen. The pointer press that raises a menu always precedes
    // the menu, so a `pointerdown` listener installed with it cannot see it.
    const close = (event: Event) => {
      // A press inside the menu is aimed at an item: let the click land.
      if (ref.current?.contains(event.target as Node)) return;
      onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // Capture phase plus `stopPropagation` keeps this Escape from also
      // reaching the slideover and the shell keyboard contract, which both
      // listen on `window` — one Escape must close exactly one thing.
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  // DESIGN.md: "Focus restoration on close."
  useEffect(() => {
    return () => {
      restoreFocus?.focus();
    };
  }, [restoreFocus]);

  const moveFocus = (delta: number) => {
    const nodes = Array.from(
      ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [],
    );
    if (nodes.length === 0) return;
    const current = nodes.indexOf(document.activeElement as HTMLElement);
    const next = (current + delta + nodes.length) % nodes.length;
    nodes[next]?.focus();
  };

  return (
    <div
      ref={ref}
      role="menu"
      aria-label={label}
      style={{ left: pos.left, top: pos.top }}
      className="fixed z-50 min-w-[180px] rounded-md border border-app bg-surface py-1 font-mono text-xs shadow-lg"
      data-testid={testid}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          moveFocus(1);
        } else if (event.key === "ArrowUp") {
          event.preventDefault();
          moveFocus(-1);
        }
      }}
    >
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="menuitem"
          data-testid={testid ? `${testid}-${item.id}` : undefined}
          className={cn(
            "flex w-full items-center gap-3 px-3 py-1.5 text-left hover:bg-surface-2",
            item.danger ? "text-danger" : "text-default",
          )}
          onClick={() => {
            item.onSelect();
            onClose();
          }}
        >
          <span className="flex-1">{item.label}</span>
          {item.hint ? <span className="text-muted">{item.hint}</span> : null}
        </button>
      ))}
    </div>
  );
}
