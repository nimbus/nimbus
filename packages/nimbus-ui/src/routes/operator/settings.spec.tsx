import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { searchRef } = vi.hoisted(() => ({
  searchRef: { current: { section: "general" } as { section: string } },
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => ({
    ...config,
    useSearch: () => searchRef.current,
  }),
  redirect: (options: Record<string, unknown>) => options,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: () => undefined,
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

// The section bodies are stubbed on purpose: this spec asserts which pane the
// URL selects, not what each pane renders.
vi.mock("../../components/appearance-section", () => ({
  AppearanceSection: () => <div data-testid="stub-appearance" />,
}));
vi.mock("./settings/configuration", () => ({
  ConfigurationSection: () => <div data-testid="stub-configuration" />,
}));
vi.mock("./settings/danger-zone", () => ({
  DangerZoneSection: () => <div data-testid="stub-danger-zone" />,
}));
vi.mock("./settings/deploys", () => ({
  DeploysSection: () => <div data-testid="stub-deploys" />,
}));
vi.mock("./settings/integrations", () => ({
  IntegrationsSection: () => <div data-testid="stub-integrations" />,
}));
vi.mock("./settings/server-info", () => ({
  ServerInfoSection: () => <div data-testid="stub-server-info" />,
  TenantHeaderStrip: () => <div data-testid="stub-tenant-header" />,
}));
vi.mock("./settings/hooks", () => ({
  useEncryptionStatus: () => undefined,
  useLicenseSnapshot: () => undefined,
  useRuntimeDiagnostics: () => undefined,
}));

import { routeComponent } from "../../test/route-internals";
import { parseSettingsSection, Route, type SettingsSection } from "./settings";
import { ADMIN_SETTINGS_SUB_DRAWER } from "./settings/sub-drawer";

const SettingsPage = routeComponent(Route);

// `Route` is a fully typed router object; reaching its lifecycle hooks in a
// unit test goes through `unknown`, the same way `test/route-internals` does.
function routeConfig(route: unknown): Record<string, unknown> {
  return route as Record<string, unknown>;
}

function validateSearch(search: Record<string, unknown>): {
  section: SettingsSection;
} {
  const fn = routeConfig(Route).validateSearch;
  if (typeof fn !== "function") {
    throw new Error("Expected the settings route to declare validateSearch.");
  }
  return (fn as (s: Record<string, unknown>) => { section: SettingsSection })(
    search,
  );
}

function beforeLoadRedirect(
  search: Record<string, unknown>,
): { to: string; search: { section: string } } | null {
  const fn = routeConfig(Route).beforeLoad;
  if (typeof fn !== "function") {
    throw new Error("Expected the settings route to declare beforeLoad.");
  }
  try {
    (fn as (args: { search: Record<string, unknown> }) => void)({ search });
    return null;
  } catch (thrown) {
    return thrown as { to: string; search: { section: string } };
  }
}

function renderSection(section: string) {
  searchRef.current = { section };
  return render(<SettingsPage />);
}

beforeEach(() => {
  searchRef.current = { section: "general" };
});

describe("parseSettingsSection", () => {
  it("accepts every section the sub-drawer can produce", () => {
    for (const item of ADMIN_SETTINGS_SUB_DRAWER.items) {
      expect(parseSettingsSection(item.search.section)).toBe(
        item.search.section,
      );
    }
  });

  it("rejects unknown and non-string values", () => {
    for (const value of ["", "Deploys", "danger", 3, null, undefined, {}]) {
      expect(parseSettingsSection(value)).toBeUndefined();
    }
  });
});

describe("settings route search", () => {
  it("defaults a missing or invalid section to general", () => {
    expect(validateSearch({})).toEqual({ section: "general" });
    expect(validateSearch({ section: "nope" })).toEqual({ section: "general" });
  });

  it("keeps a valid section", () => {
    expect(validateSearch({ section: "deploys" })).toEqual({
      section: "deploys",
    });
  });

  // The sub-drawer marks an item active by comparing search values exactly, so
  // a bare `/operator/settings` would leave every item inactive. The redirect
  // is what makes the menu locate the operator.
  it("normalizes a bare URL to the default section", () => {
    const redirected = beforeLoadRedirect({});
    expect(redirected).not.toBeNull();
    expect(redirected?.to).toBe("/operator/settings");
    expect(redirected?.search).toEqual({ section: "general" });
  });

  it("normalizes an invalid section", () => {
    expect(beforeLoadRedirect({ section: "danger" })?.search).toEqual({
      section: "general",
    });
  });

  it("leaves a valid section alone", () => {
    for (const item of ADMIN_SETTINGS_SUB_DRAWER.items) {
      expect(beforeLoadRedirect({ section: item.search.section })).toBeNull();
    }
  });
});

describe("sub-drawer parity", () => {
  type MenuSection =
    (typeof ADMIN_SETTINGS_SUB_DRAWER.items)[number]["search"]["section"];

  // Both conversions must compile. Together they assert set equality between
  // the menu ids and the route's section space: a menu entry the route cannot
  // parse, or a section with no way to reach it, is a compile error here.
  const asRouteSection = (value: MenuSection): SettingsSection => value;
  const asMenuSection = (value: SettingsSection): MenuSection => value;

  it("offers each section exactly once", () => {
    const sections = ADMIN_SETTINGS_SUB_DRAWER.items.map((item) =>
      asRouteSection(item.search.section),
    );
    expect(new Set(sections).size).toBe(sections.length);
    for (const section of sections) {
      expect(asMenuSection(section)).toBe(section);
    }
  });

  it("routes every menu entry at the settings page", () => {
    for (const item of ADMIN_SETTINGS_SUB_DRAWER.items) {
      expect(item.to).toBe("/operator/settings");
    }
  });
});

describe("section rendering", () => {
  it("renders the general section by default", () => {
    renderSection("general");
    expect(screen.getByTestId("page-settings").dataset.section).toBe("general");
    expect(screen.getByTestId("stub-appearance")).toBeTruthy();
    expect(screen.getByTestId("stub-server-info")).toBeTruthy();
    expect(screen.getByTestId("stub-configuration")).toBeTruthy();
    expect(screen.queryByTestId("stub-deploys")).toBeNull();
  });

  it.each([
    ["deploys", "stub-deploys"],
    ["integrations", "stub-integrations"],
    ["shutdown", "stub-danger-zone"],
  ] as const)("renders the %s section on its own", (section, testid) => {
    renderSection(section);
    expect(screen.getByTestId("page-settings").dataset.section).toBe(section);
    expect(screen.getByTestId(testid)).toBeTruthy();
    expect(screen.queryByTestId("stub-appearance")).toBeNull();
  });

  // Menu entries DESIGN.md specifies but this build does not implement. They
  // say so rather than rendering an empty frame.
  it.each([
    "endpoints",
    "token",
    "environment",
  ] as const)("says the %s section is unavailable instead of rendering an empty frame", (section) => {
    renderSection(section);
    const empty = screen.getByTestId(`settings-${section}-unavailable`);
    expect(empty).toBeTruthy();
    expect(
      screen.getByTestId(`settings-${section}-unavailable-body`).textContent,
    ).toContain("not available in this build");
    expect(screen.queryByTestId("stub-appearance")).toBeNull();
    expect(screen.queryByTestId("stub-deploys")).toBeNull();
  });

  it("gives every section its own subtitle", () => {
    const subtitles = ADMIN_SETTINGS_SUB_DRAWER.items.map((item) => {
      const { unmount } = renderSection(item.search.section);
      const text =
        screen.getByTestId("page-settings").querySelector("p")?.textContent ??
        "";
      unmount();
      return text;
    });
    expect(subtitles.every((text) => text.length > 0)).toBe(true);
    expect(new Set(subtitles).size).toBe(subtitles.length);
  });
});
