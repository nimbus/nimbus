import { createFileRoute, redirect } from "@tanstack/react-router";

import {
  type NavView,
  readLastRouteForView,
  readLastView,
} from "../store/ui-store";

/**
 * Sections a cold load may restore into, per view. A remembered route is
 * restored only as far as its section root: the deep paths this console can
 * persist (`/developer/storage/<table>`, `/developer/compute/runs/<id>`,
 * `/operator/services/<id>`, …) all end in a resource id that can be gone by
 * the next launch, and cold-loading into a not-found page is a worse first
 * impression than the section index.
 *
 * `index.spec.ts` asserts this list still covers the generated route tree.
 */
export const RESTORABLE_SECTIONS: Record<NavView, readonly string[]> = {
  developer: [
    "compute",
    "files",
    "observability",
    "schedules",
    "services",
    "settings",
    "storage",
  ],
  operator: [
    "machines",
    "network",
    "observability",
    "services",
    "settings",
    "tenants",
  ],
};

/**
 * Resolve where `/` should land, given the persisted last view and its
 * remembered route. Falls back to the view root for anything that is not an
 * exact, in-tree section of that view — including near-miss prefixes such as
 * `/developerXYZ`, which `readLastRouteForView` accepts today.
 */
export function resolveColdStart(view: NavView, stored: string | null): string {
  const root = `/${view}`;
  if (stored === null) return root;
  if (stored !== root && !stored.startsWith(`${root}/`)) return root;
  const section = stored.slice(root.length + 1).split("/")[0] ?? "";
  if (section === "") return root;
  return RESTORABLE_SECTIONS[view].includes(section)
    ? `${root}/${section}`
    : root;
}

export const Route = createFileRoute("/")({
  beforeLoad: () => {
    const view = readLastView();
    throw redirect({ to: resolveColdStart(view, readLastRouteForView(view)) });
  },
});
