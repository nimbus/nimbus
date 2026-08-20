import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
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

import { resolveStateKind } from "../../../components/state-chip";
import { RUN_STATUSES, RunsTab } from "./-runs";

/**
 * The status dropdown is a closed list, so every option it offers is a promise
 * that rows exist behind it. `running` and `queued` were on that list and no
 * run has ever carried either: a row is written only after the invocation
 * returns, with `result.is_ok() ? "ok" : "error"`. Both options answered "No
 * runs" for every deployment that has ever existed, which reads as "your runs
 * are missing" rather than "this state does not occur here".
 */
describe("run status filter", () => {
  it("offers only the two values a run can carry", () => {
    expect([...RUN_STATUSES]).toEqual(["ok", "error"]);
  });

  it("names every offered value in the state palette", () => {
    for (const status of RUN_STATUSES) {
      expect(resolveStateKind(status)).toBe(status);
    }
  });
});

/**
 * A fresh install lands on this tab and its empty state was one 12px muted
 * line in a box. The same condition one nav entry away — Operator ->
 * Observability -> Runs — renders the full `EmptyState`, and DESIGN.md's
 * whole-tab scope calls for a mono title plus a two-line body plus a next
 * action. This tab is whole-tab scope and had none of it.
 */
describe("RunsTab read states", () => {
  it("distinguishes a read in flight from a settled empty result", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<RunsTab search={{ tab: "runs" }} />);

    expect(screen.getByTestId("observability-runs-loading")).toHaveTextContent(
      /Loading runs/i,
    );
    expect(screen.queryByTestId("observability-runs-empty")).toBeNull();
  });

  it("gives the empty tab a title, a body, and a next action", () => {
    useQueryMock.mockReturnValue([]);
    render(<RunsTab search={{ tab: "runs" }} />);

    const title = screen.getByTestId("observability-runs-empty-title");
    expect(title.tagName).toBe("H2");
    expect(title).toHaveTextContent("No runs yet");
    expect(
      screen.getByTestId("observability-runs-empty-body").textContent ?? "",
    ).not.toHaveLength(0);
    expect(
      screen.getByTestId("observability-runs-empty-cta"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("observability-runs-loading")).toBeNull();
  });

  it("names the filters, and offers to clear them, only when some are set", () => {
    useQueryMock.mockReturnValue([]);
    render(<RunsTab search={{ tab: "runs", status: "error" }} />);

    expect(
      screen.getByTestId("observability-runs-empty-title"),
    ).toHaveTextContent("No runs match the current filters");
    expect(
      screen.getByTestId("observability-runs-empty-cta"),
    ).toHaveTextContent(/Clear filters/i);
  });

  it("does not blame filters for an unfiltered empty list", () => {
    useQueryMock.mockReturnValue([]);
    render(<RunsTab search={{ tab: "runs" }} />);

    expect(
      screen.getByTestId("observability-runs-empty"),
    ).not.toHaveTextContent(/current filters/i);
  });
});
