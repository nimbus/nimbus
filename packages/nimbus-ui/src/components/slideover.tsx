import { type ReactNode, useRef } from "react";

import { useModalFocus } from "../hooks/use-modal-focus";

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

  // Callers mount the drawer only while it is open, so the trap is armed for
  // the whole life of the component.
  useModalFocus({ open: true, panelRef, onEscape: onClose });

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
        // 480px is the preferred width; `max-w-full` is the clamp under it.
        // With today's bodies the clamp is belt-and-braces: the panel's
        // min-content floor is 207px (insert) and 245px (edit) — a 60-char
        // field name changes neither, because the widest child is a textarea,
        // which takes its intrinsic width from `cols` and never propagates its
        // content outward — so below 480px the panel just flex-shrinks to the
        // viewport. The clamp earns its place when a child floors the panel
        // ABOVE the viewport: a 56-char unbroken title measures a 573px floor,
        // which pins the panel at 480px, and `justify-end` then sends that
        // overflow out by the LEFT edge, taking the close button with it.
        //
        // `max-w-full` here but `min(480px,90vw)` on the shell error card is a
        // gutter choice, not a mechanism one: a max-width lowers a flex item's
        // automatic minimum size too, since the size suggestion that minimum
        // is built from is itself clamped by max-width. This panel is
        // edge-anchored and should meet the viewport edge; that card is
        // centred and needs a gutter to still read as a card.
        className="relative flex h-full w-[480px] max-w-full flex-col gap-2 border-l border-app bg-surface p-4 shadow-xl outline-none"
        data-testid={testid}
      >
        <PanelHeader title={title} onClose={onClose} />
        {children}
      </div>
    </div>
  );
}
