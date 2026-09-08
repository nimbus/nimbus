import { useSyncExternalStore } from "react";

export type ViewportTier = "mobile" | "tablet" | "desktop";

const MOBILE_QUERY = "(max-width: 767px)";
const TABLET_QUERY = "(max-width: 1023px)";

// SMALL_SCREEN_QUERY is the sidebar's one breakpoint: below Tailwind's `sm`
// (640px) the sidebar is a sheet behind a top-bar button and the main column
// takes the whole width. It sits inside the mobile tier rather than on its
// edge because a 700px window still has room for the 64px rail beside a
// page, and a 600px one does not.
export const SMALL_SCREEN_QUERY = "(max-width: 639px)";

// Viewport tier is derived state, never persisted. A stored value would
// overwrite the operator's desktop panel preference on any tablet-width
// visit, so nothing here writes to localStorage or to the ui store.
//
// `mobile` drops the keyboard hints and turns the sub-panel into an overlay sheet;
// the sidebar itself reads `useSmallScreen` below, which is narrower.
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

function readSmallScreen(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia(SMALL_SCREEN_QUERY).matches;
}

function subscribeSmallScreen(onChange: () => void): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => {};
  const list = window.matchMedia(SMALL_SCREEN_QUERY);
  list.addEventListener("change", onChange);
  return () => list.removeEventListener("change", onChange);
}

// The stylesheet handles most breakpoint styling; this hook exists because
// the component tree itself differs on either side of the line, so the sheet
// is mounted below it and the aside above it.
export function useSmallScreen(): boolean {
  return useSyncExternalStore(
    subscribeSmallScreen,
    readSmallScreen,
    () => false,
  );
}
