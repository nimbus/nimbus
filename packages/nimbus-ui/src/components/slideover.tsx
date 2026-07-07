import { type ReactNode, useEffect } from "react";

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
        className="rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
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
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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
        role="dialog"
        aria-label={title}
        className="relative flex h-full w-[480px] flex-col gap-2 border-l border-app bg-bg p-4 shadow-xl"
        data-testid={testid}
      >
        <PanelHeader title={title} onClose={onClose} />
        {children}
      </div>
    </div>
  );
}
