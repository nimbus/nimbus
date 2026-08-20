import { useSyncExternalStore } from "react";

export type ViewportTier = "mobile" | "tablet" | "desktop";

const MOBILE_QUERY = "(max-width: 767px)";
const TABLET_QUERY = "(max-width: 1023px)";

// Viewport tier is derived state, never persisted. A stored value would
// overwrite the operator's desktop drawer preference on any tablet-width
// visit, so nothing here writes to localStorage or to the ui store.
//
// `mobile` is normative, not dead code: DESIGN.md's responsive section
// collapses the primary drawer and sub-drawer into a single hamburger sheet
// with bottom navigation at this tier. The shell has not built that yet and
// still renders both rails, so what is missing is the shell work — delete the
// gap, not the tier.
function readTier(): ViewportTier {
  if (typeof window === "undefined" || !window.matchMedia) return "desktop";
  if (window.matchMedia(MOBILE_QUERY).matches) return "mobile";
  if (window.matchMedia(TABLET_QUERY).matches) return "tablet";
  return "desktop";
}

function subscribe(onChange: () => void): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => {};
  const lists = [
    window.matchMedia(MOBILE_QUERY),
    window.matchMedia(TABLET_QUERY),
  ];
  for (const list of lists) list.addEventListener("change", onChange);
  return () => {
    for (const list of lists) list.removeEventListener("change", onChange);
  };
}

export function useViewportTier(): ViewportTier {
  return useSyncExternalStore(subscribe, readTier, () => "desktop");
}
