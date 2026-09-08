import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  Link: ({
    to,
    children,
    "data-testid": testId,
    "aria-current": current,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    "aria-current"?: "page";
    className?: string;
  }) => (
    <a
      href={to}
      data-testid={testId}
      aria-current={current}
      className={className}
    >
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

import { routeComponent } from "../../test/route-internals";
import { ADMIN_OBSERVABILITY_TABS, Route } from "./observability";

function renderPage(search: Record<string, unknown> = { tab: "logs" }) {
  const validateSearch = (
    Route as unknown as {
      validateSearch: (s: Record<string, unknown>) => Record<string, unknown>;
    }
  ).validateSearch;
  const resolved = validateSearch(search);
  (Route as unknown as { useSearch: () => Record<string, unknown> }).useSearch =
    () => resolved;
  const Component = routeComponent(Route);
  render(<Component />);
  return resolved;
}

describe("operator observability sub-view switching", () => {
  it("defaults the tab in the search so the strip can mark it active", () => {
    // The strip decides "active" by comparing an item's id against the
    // search. An undefined tab renders Logs while showing nothing as
    // selected, so the default is resolved here rather than at render.
    const resolved = (
      Route as unknown as {
        validateSearch: (s: Record<string, unknown>) => { tab?: string };
      }
    ).validateSearch({});
    expect(resolved.tab).toBe("logs");
  });

  it("switches sub-views through a tab strip under the header, not a sub-panel", () => {
    useQueryMock.mockReturnValue([]);
    renderPage({ tab: "runs" });

    const strip = screen.getByTestId("admin-observability-tabs");
    expect(strip).toHaveAttribute("aria-label", "Operator observability tabs");
    expect(
      screen
        .getByTestId("admin-observability-header")
        .querySelector('[data-testid="admin-observability-tabs"]'),
    ).toBeNull();
    expect(screen.getByTestId("admin-observability-tab-runs")).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(
      screen.getByTestId("admin-observability-tab-logs"),
    ).not.toHaveAttribute("aria-current");
  });

  it("names only the sub-views that exist", () => {
    // Events and Errors return with their pages (UIR20). Until then the
    // strip does not show a name the operator cannot open.
    expect(ADMIN_OBSERVABILITY_TABS.map((tab) => tab.id)).toEqual([
      "logs",
      "runs",
    ]);
    for (const tab of ADMIN_OBSERVABILITY_TABS) {
      expect(tab.label).not.toMatch(/soon/i);
    }
  });
});

// The page hand-rolled the title/subtitle/trailing molecule instead of using
// `PageHeader`, so its header drifted from every sibling page. jsdom does no
// layout, so the shared component's `data-slot` marker is what a test can read
// back: a hand-rolled `<p>` does not carry it.
describe("operator observability header", () => {
  it("renders its subtitle through the shared PageHeader", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    const header = screen.getByTestId("admin-observability-header");
    expect(header.querySelector("h1")?.textContent).toBe(
      "Operator observability",
    );
    const subtitle = header.querySelector("p");
    expect(subtitle?.textContent).toContain(
      "Logs and runs across every tenant.",
    );
    expect(subtitle?.getAttribute("data-slot")).toBe("page-subtitle");
  });

  it("keeps the scope chip in the header's trailing slot", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    const header = screen.getByTestId("admin-observability-header");
    const chip = header.querySelector(
      '[data-testid="admin-observability-scope"]',
    );
    expect(chip?.textContent).toBe("tenant filter unavailable");
  });
});
