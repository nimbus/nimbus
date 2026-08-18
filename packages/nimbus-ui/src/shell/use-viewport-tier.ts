import { useSyncExternalStore } from "react";

export type ViewportTier = "mobile" | "tablet" | "desktop";

const MOBILE_QUERY = "(max-width: 767px)";
const TABLET_QUERY = "(max-width: 1023px)";

// Viewport tier is derived state, never persisted. A stored value would
// overwrite the operator's desktop drawer preference on any tablet-width
// visit, so nothing here writes to localStorage or to the ui store.
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
