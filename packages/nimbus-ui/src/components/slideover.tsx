import { type ReactNode, useEffect, useRef } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableWithin(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) =>
      !el.hasAttribute("hidden") && el.getAttribute("aria-hidden") !== "true",
  );
}

// Header bar shared by the Slideover and the inline side panels (schema,
// indexes): a mono title on the left and a close button on the right.
export function PanelHeader({
  title,
  onClose,
}: {
  title: string;
  onClose: () => void;
}) {
  return (
    <div className="flex items-center justify-between border-b border-app px-3 py-2">
      <h2 className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
        {title}
      </h2>
      <button
        type="button"
        onClick={onClose}
        aria-label={`Close ${title}`}
        className="rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        close
      </button>
    </div>
  );
}

// Right-anchored overlay panel: a dimmed backdrop, Escape-to-close, and a
// `PanelHeader`. Callers supply the body; the overlay and dismissal wiring live
// here so drawers don't re-hand-roll them per route.
export function Slideover({
  title,
  onClose,
  testid,
  children,
}: {
  title: string;
  onClose: () => void;
  testid: string;
  children: ReactNode;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  // Move focus into the panel on open, keep Tab inside it while open, and hand
  // focus back to the opener on close (DESIGN.md: focus restoration on close).
  useEffect(() => {
    previouslyFocusedRef.current =
      (document.activeElement as HTMLElement | null) ?? null;
    panelRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const panel = panelRef.current;
      if (!panel) return;
      const items = focusableWithin(panel);
      if (items.length === 0) {
        e.preventDefault();
        panel.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (active && !panel.contains(active)) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
        return;
      }
      if (e.shiftKey && (active === first || active === panel)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previouslyFocusedRef.current?.focus?.();
    };
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-30 flex justify-end">
      <button
        type="button"
        aria-label={`Dismiss ${title}`}
        onClick={onClose}
        className="absolute inset-0 bg-black/40"
        data-testid={`${testid}-overlay`}
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        // tabIndex -1 so the panel itself can take programmatic focus on open;
        // outline-none so that lands without painting a ring on the whole panel.
        tabIndex={-1}
        className="relative flex h-full w-[480px] flex-col gap-2 border-l border-app bg-bg p-4 shadow-xl outline-none"
        data-testid={testid}
      >
        <PanelHeader title={title} onClose={onClose} />
        {children}
      </div>
    </div>
  );
}
