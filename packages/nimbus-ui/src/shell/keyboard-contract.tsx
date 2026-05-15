import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

export function KeyboardContract() {
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  const setActionMenuOpen = useUiStore((s) => s.setActionMenuOpen);

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
      if (meta && event.key === ".") {
        event.preventDefault();
        const { actionMenuOpen } = useUiStore.getState();
        setActionMenuOpen(!actionMenuOpen);
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
        if (state.actionMenuOpen) {
          event.preventDefault();
          setActionMenuOpen(false);
          return;
        }
      }
      if (event.key === "/" && !isTypingTarget(event.target)) {
        const search = document.querySelector<HTMLInputElement>(
          "[data-inline-search]",
        );
        if (search) {
          event.preventDefault();
          search.focus();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPaletteOpen, setLensOpen, setActionMenuOpen]);
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
