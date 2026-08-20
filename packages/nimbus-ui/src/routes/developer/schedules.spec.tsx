import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { useSearchMock } = vi.hoisted(() => ({ useSearchMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useSearch: (..._args: unknown[]) => useSearchMock(),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./schedules";

const SchedulesPage = routeComponent(Route);

beforeEach(() => {
  useQueryMock.mockReset();
  useSearchMock.mockReset().mockReturnValue({ section: "scheduled" });
});

describe("SchedulesPage loading state", () => {
  it("holds the scheduled table geometry with skeleton rows", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<SchedulesPage />);

    const loading = screen.getByTestId("schedules-scheduled-loading");
    expect(
      loading.querySelectorAll('[data-testid="skeleton-row"]'),
    ).toHaveLength(8);
    expect(loading.querySelectorAll("thead th")).toHaveLength(5);
    // The header is what makes the swap invisible: it must survive the load.
    expect(loading).toHaveTextContent("Duration");
  });

  it("holds the cron table geometry with skeleton rows", () => {
    useSearchMock.mockReturnValue({ section: "cron" });
    useQueryMock.mockReturnValue(undefined);
    render(<SchedulesPage />);

    const loading = screen.getByTestId("schedules-cron-loading");
    expect(
      loading.querySelectorAll('[data-testid="skeleton-row"]'),
    ).toHaveLength(8);
    expect(loading.querySelectorAll("thead th")).toHaveLength(6);
    expect(loading).toHaveTextContent("Next run");
  });

  it("gives each skeleton the same header as its loaded table", () => {
    for (const section of ["scheduled", "cron"] as const) {
      useSearchMock.mockReturnValue({ section });

      useQueryMock.mockReturnValue(undefined);
      const loading = render(<SchedulesPage />);
      const skeletonHeader = headerLabels(loading.container);
      loading.unmount();

      useQueryMock.mockReturnValue([{ _id: "j1", functionPath: "tasks:run" }]);
      const loaded = render(<SchedulesPage />);
      expect(headerLabels(loaded.container)).toEqual(skeletonHeader);
      loaded.unmount();
    }
  });
});

describe("SchedulesPage empty states", () => {
  it("shows the scheduled empty state once the query settles on zero jobs", () => {
    useQueryMock.mockReturnValue([]);
    render(<SchedulesPage />);

    expect(screen.getByText("No scheduled jobs")).toBeInTheDocument();
    expect(screen.queryByTestId("schedules-scheduled-loading")).toBeNull();
  });

  it("shows the cron empty state once the query settles on zero jobs", () => {
    useSearchMock.mockReturnValue({ section: "cron" });
    useQueryMock.mockReturnValue([]);
    render(<SchedulesPage />);

    expect(screen.getByText("No cron jobs")).toBeInTheDocument();
    expect(screen.queryByTestId("schedules-cron-loading")).toBeNull();
  });
});

function headerLabels(container: HTMLElement) {
  return Array.from(container.querySelectorAll("thead th")).map((cell) =>
    cell.textContent?.trim(),
  );
}
