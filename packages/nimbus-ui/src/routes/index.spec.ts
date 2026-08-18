import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  // The real `redirect` returns a value the caller throws; the shape under
  // test is only its `to`.
  redirect: (options: { to: string }) => options,
}));

// `?raw` keeps the drift guard reading the generated file itself rather than a
// hand-copied duplicate of its contents.
import routeTreeSource from "../route-tree.gen.ts?raw";
import type { NavView } from "../store/ui-store";
import { RESTORABLE_SECTIONS, Route, resolveColdStart } from "./index";

const LAST_VIEW_KEY = "nimbus-ui:last-view";
const lastRouteKey = (view: NavView) => `nimbus-ui:last-route:${view}`;

// `Route` is a fully typed router object; reaching its lifecycle hooks in a
// unit test goes through `unknown`, the same way `test/route-internals` does.
function routeConfig(route: unknown): Record<string, unknown> {
  return route as Record<string, unknown>;
}

function coldStartTarget(): string {
  const beforeLoad = routeConfig(Route).beforeLoad;
  if (typeof beforeLoad !== "function") {
    throw new Error("Expected the index route to declare a beforeLoad.");
  }
  try {
    (beforeLoad as () => void)();
  } catch (thrown) {
    return (thrown as { to: string }).to;
  }
  throw new Error("Expected the index route to redirect.");
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("cold start", () => {
  it("lands on the developer console when nothing is persisted", () => {
    expect(coldStartTarget()).toBe("/developer");
  });

  it("honors the persisted view even without a persisted route", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "operator");
    expect(coldStartTarget()).toBe("/operator");
  });

  it("restores the persisted route for the persisted view", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "operator");
    window.localStorage.setItem(lastRouteKey("operator"), "/operator/tenants");
    expect(coldStartTarget()).toBe("/operator/tenants");
  });

  it("restores only the section root of a deep resource path", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "operator");
    window.localStorage.setItem(
      lastRouteKey("operator"),
      "/operator/services/svc_7f3a",
    );
    expect(coldStartTarget()).toBe("/operator/services");
  });

  it("falls back to the view root for a stale or malformed route", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "operator");
    window.localStorage.setItem(lastRouteKey("operator"), "/operator/retired");
    expect(coldStartTarget()).toBe("/operator");
  });

  it("ignores a route persisted under the other view", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "operator");
    window.localStorage.setItem(
      lastRouteKey("developer"),
      "/developer/compute",
    );
    expect(coldStartTarget()).toBe("/operator");
  });

  it("treats an unrecognized persisted view as developer", () => {
    window.localStorage.setItem(LAST_VIEW_KEY, "sysadmin");
    window.localStorage.setItem(
      lastRouteKey("developer"),
      "/developer/compute",
    );
    expect(coldStartTarget()).toBe("/developer/compute");
  });
});

describe("resolveColdStart", () => {
  it("returns the view root for no stored route", () => {
    expect(resolveColdStart("developer", null)).toBe("/developer");
  });

  it("returns the view root for the view root itself", () => {
    expect(resolveColdStart("developer", "/developer")).toBe("/developer");
    expect(resolveColdStart("developer", "/developer/")).toBe("/developer");
  });

  it("rejects a near-miss prefix of the view root", () => {
    // `readLastRouteForView` accepts this today because it only checks
    // `startsWith("/developer")`.
    expect(resolveColdStart("developer", "/developerXYZ")).toBe("/developer");
    expect(resolveColdStart("developer", "/developerXYZ/storage")).toBe(
      "/developer",
    );
  });

  it("rejects a route belonging to the other view", () => {
    expect(resolveColdStart("developer", "/operator/machines")).toBe(
      "/developer",
    );
  });

  it("restores every section it claims to restore", () => {
    for (const view of ["developer", "operator"] as const) {
      for (const section of RESTORABLE_SECTIONS[view]) {
        expect(resolveColdStart(view, `/${view}/${section}`)).toBe(
          `/${view}/${section}`,
        );
        expect(resolveColdStart(view, `/${view}/${section}/deep/leaf`)).toBe(
          `/${view}/${section}`,
        );
      }
    }
  });
});

describe("RESTORABLE_SECTIONS", () => {
  // Drift guard: a section added to the router but missing here silently stops
  // being restorable, and a section listed here but absent from the router
  // cold-starts into a not-found page.
  function generatedSections(view: NavView): string[] {
    const pattern = new RegExp(`'/${view}/([^/']+)`, "g");
    const found = new Set<string>();
    for (const match of routeTreeSource.matchAll(pattern)) {
      // TanStack escapes non-nested segments with a trailing underscore.
      const section = match[1].replace(/_$/, "");
      if (section !== "" && !section.startsWith("$")) found.add(section);
    }
    return Array.from(found).sort();
  }

  it.each([
    "developer",
    "operator",
  ] as const)("covers every %s section in the generated route tree", (view) => {
    const generated = generatedSections(view);
    expect(generated.length).toBeGreaterThan(0);
    expect([...RESTORABLE_SECTIONS[view]].sort()).toEqual(generated);
  });
});
