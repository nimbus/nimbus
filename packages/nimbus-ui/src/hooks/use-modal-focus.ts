import { type RefObject, useEffect, useRef } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableWithin(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) =>
      !el.hasAttribute("hidden") && el.getAttribute("aria-hidden") !== "true",
  );
}

// The focus contract a modal surface owes the operator: remember what was
// focused when it opened, move focus inside, keep Tab from walking out into the
// page behind, and hand focus back on close (DESIGN.md: focus restoration on
// close). `Slideover` and `ConfirmDialog` both run it so the two cannot drift —
// the drawer used to cycle Tab and the dialog did not, which left Enter on the
// row behind a modal confirm still firing that row's action.
//
// Restoration needs a live target. A control that takes the `disabled`
// attribute is not focusable, so a caller that disables the opener in the same
// commit that closes the modal gets focus on <body> instead: the operator is
// left at the top of the page with no idea where the caret went. Callers that
// gray out the opener while its work runs mark it `aria-disabled` and refuse
// the action in the handler, which keeps the element focusable.
export function useModalFocus({
  open,
  panelRef,
  initialFocusRef,
  onEscape,
}: {
  open: boolean;
  panelRef: RefObject<HTMLElement | null>;
  initialFocusRef?: RefObject<HTMLElement | null>;
  onEscape: () => void;
}) {
  // Escape is read through a ref instead of a dependency. Callers write the
  // handler inline — `onClose={() => setEditing(null)}` — so it is a different
  // function on every parent render, and the storage route re-renders on every
  // push from its live table subscription. With the handler in the dependency
  // array the effect tore itself down on each of those pushes, restored focus
  // to the opener and re-focused the panel, so the caret left the JSON the
  // operator was typing while another writer touched the table.
  const escapeRef = useRef(onEscape);
  useEffect(() => {
    escapeRef.current = onEscape;
  }, [onEscape]);

  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement as HTMLElement | null;
    (initialFocusRef?.current ?? panelRef.current)?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        escapeRef.current();
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
      opener?.focus?.();
    };
  }, [open, panelRef, initialFocusRef]);
}
