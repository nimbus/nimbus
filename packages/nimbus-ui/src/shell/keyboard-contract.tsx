import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

// `/` focuses the page's own filter input. Page-level filters claim
// `primary`; the sub-drawer's filter is the fallback, because it precedes page
// content in the DOM and would otherwise win every document-order lookup on a
// route that has both.
function findInlineSearch(): HTMLInputElement | null {
  return (
    document.querySelector<HTMLInputElement>(
      '[data-inline-search="primary"]',
    ) ??
    document.querySelector<HTMLInputElement>(
      '[data-inline-search]:not([data-inline-search="drawer"])',
    ) ??
    document.querySelector<HTMLInputElement>('[data-inline-search="drawer"]')
  );
}

export function KeyboardContract() {
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const meta = event.metaKey || event.ctrlKey;
      if (meta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        const { paletteOpen } = useUiStore.getState();
        setPaletteOpen(!paletteOpen);
        return;
      }
      if (meta && (event.key === "\\" || event.key === "|")) {
        event.preventDefault();
        const { lensOpen } = useUiStore.getState();
        setLensOpen(!lensOpen);
        return;
      }
      if (event.key === "Escape") {
        const state = useUiStore.getState();
        if (state.paletteOpen) {
          event.preventDefault();
          setPaletteOpen(false);
          return;
        }
        if (state.lensOpen) {
          event.preventDefault();
          setLensOpen(false);
          return;
        }
      }
      if (event.key === "/" && !isTypingTarget(event.target)) {
        const search = findInlineSearch();
        if (search) {
          event.preventDefault();
          search.focus();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPaletteOpen, setLensOpen]);
  return null;
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.isContentEditable
  );
}
