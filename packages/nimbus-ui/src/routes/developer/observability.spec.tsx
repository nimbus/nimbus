import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./observability";

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
}

// The page hand-rolled the title/subtitle molecule instead of using
// `PageHeader`, and so missed the one thing that component exists for: the
// 68ch cap. Its description set a single unbroken line whose measure changed
// against every sibling page that does go through `PageHeader`. jsdom does no
// layout, so the cap utility is the only thing a test can read back.
describe("developer observability header", () => {
  it("caps the subtitle measure through the shared PageHeader", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    const header = screen.getByTestId("observability-header");
    expect(header.querySelector("h1")?.textContent).toBe("Observability");
    const subtitle = header.querySelector("p");
    expect(subtitle?.textContent).toContain(
      "Live event stream and recent runs",
    );
    expect(subtitle?.className.split(" ")).toContain("max-w-[68ch]");
  });

  it("keeps the system tenant marked up as code in the subtitle", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    // The subtitle is a node, not a string: `_nimbus` is an identifier and
    // reads as prose without the mono face.
    const code = screen
      .getByTestId("observability-header")
      .querySelector("code");
    expect(code?.textContent).toBe("_nimbus");
  });

  it("keeps the tab strip below the header, not inside it", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    // The tabs are a sibling of the header molecule rather than its trailing
    // slot, so `PageHeader` needs no accommodation for them.
    const header = screen.getByTestId("observability-header");
    expect(
      header.querySelector('[data-testid="observability-tabs"]'),
    ).toBeNull();
    expect(screen.getByTestId("observability-tabs")).toBeInTheDocument();
  });

  it("holds the header column against the clipped page column", () => {
    useQueryMock.mockReturnValue([]);
    renderPage();

    // The page column is `overflow-hidden`: anything flexbox compresses here
    // is clipped with no way to scroll it back.
    const column = screen.getByTestId("observability-header").parentElement;
    expect(column?.className).toContain("shrink-0");
    expect(screen.getByTestId("observability-tabs").className).toContain(
      "shrink-0",
    );
  });
});
