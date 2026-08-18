import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

const { contributeMock } = vi.hoisted(() => ({ contributeMock: vi.fn() }));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: (spec: unknown) => contributeMock(spec),
}));

import { routeComponent } from "../../test/route-internals";
import { ADMIN_OBSERVABILITY_SUB_DRAWER, Route } from "./observability";

type RailItem = { id: string; active: boolean };
type ContributedSpec = { railItems?: RailItem[] };

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
  it("defaults the tab in the search so the sub-drawer can mark it active", () => {
    // The sub-drawer decides "active" by matching an item's `search` against
    // the location's. An undefined tab renders Logs while showing nothing as
    // selected, so the default is resolved here rather than at render.
    const resolved = (
      Route as unknown as {
        validateSearch: (s: Record<string, unknown>) => { tab?: string };
      }
    ).validateSearch({});
    expect(resolved.tab).toBe("logs");
  });

  it("does not duplicate the sub-drawer as a tab strip", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    // DESIGN.md: do not duplicate primary navigation in the sub-drawer. The
    // drawer owns Logs/Runs/Events/Errors; a second in-page strip is the
    // duplicate that has to stay gone.
    expect(screen.queryByTestId("admin-observability-tabs")).toBeNull();
    expect(screen.getByTestId("page-admin-observability")).toBeInTheDocument();
  });

  it("keeps enabled sub-views reachable from the collapsed icon rail", () => {
    useQueryMock.mockReturnValue([]);
    contributeMock.mockClear();
    renderPage({ tab: "runs" });

    const spec = contributeMock.mock.calls.at(-1)?.[0] as ContributedSpec;
    // Collapsing the drawer must not strand the operator without a switch.
    expect(spec.railItems?.map((item) => item.id)).toEqual(["logs", "runs"]);
    expect(spec.railItems?.find((item) => item.id === "runs")?.active).toBe(
      true,
    );
  });

  it("marks unavailable sub-views in their own label", () => {
    const disabled = ADMIN_OBSERVABILITY_SUB_DRAWER.items.filter(
      (item) => item.disabled,
    );
    expect(disabled.map((item) => item.label)).toEqual([
      "Events · soon",
      "Errors · soon",
    ]);
  });
});
