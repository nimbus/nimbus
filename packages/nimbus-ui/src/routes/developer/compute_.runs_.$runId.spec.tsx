import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  notFound: vi.fn(() => new Error("__NOT_FOUND__")),
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to?: string;
    children: ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to ?? "#"} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: vi.fn() }),
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./compute_.runs_.$runId";

const NOW = 1_700_000_000_000;

const RUN_OK = {
  _id: "runs:1",
  _creationTime: NOW,
  status: "ok",
  functionPath: "messages:list",
  kind: "query",
  durationMs: 120,
  startedAt: NOW,
};

// The spans the server records for a run: the function's own span at index
// 0 with no parent, then one span per host call under it.
const OK_CHILD_SPAN = {
  name: "convex.ctx.db.get",
  kind: "db",
  parent: 0,
  startMs: 10,
  durationMs: 4,
  status: "ok",
};

const ERROR_CHILD_SPAN = {
  name: "convex.ctx.db.insert",
  kind: "db",
  parent: 0,
  startMs: 40,
  durationMs: 12,
  status: "error",
};

function rootSpan(status: string) {
  return {
    name: "messages:list",
    kind: "function",
    parent: null,
    startMs: 0,
    durationMs: 120,
    status,
  };
}

function renderRun(
  run: Record<string, unknown>,
  spans: Record<string, unknown>[] = [],
) {
  const route = Route as unknown as Record<string, unknown>;
  route.useParams = () => ({ runId: "runs:1" });
  route.useLoaderData = () => ({
    run: { ...run, spans: spans.length > 0 ? spans : undefined },
    events: [],
  });
  const Component = routeComponent(Route);
  return render(<Component />);
}

function marker(testid: string) {
  return within(screen.getByTestId(testid)).queryByRole("img");
}

// The waterfall carried a span's status in the bar's hue and nowhere else, so
// a reader who cannot separate the danger and muted hues saw two identical
// bars. These assertions read the glyph and its accessible name — never a
// class — so they fail if the fix ever collapses back onto color.
describe("run detail trace waterfall", () => {
  it("marks an errored span with a glyph and names it to assistive tech", () => {
    renderRun(RUN_OK, [rootSpan("ok"), OK_CHILD_SPAN, ERROR_CHILD_SPAN]);

    const errored = within(
      screen.getByTestId("run-detail-trace-span-2"),
    ).getByRole("img", { name: "error" });
    expect(errored).toHaveTextContent("✗");
  });

  it("leaves a non-errored span unmarked", () => {
    renderRun(RUN_OK, [rootSpan("ok"), OK_CHILD_SPAN, ERROR_CHILD_SPAN]);

    expect(marker("run-detail-trace-span-1")).toBeNull();
    expect(screen.queryByTestId("run-detail-trace-span-1-marker")).toBeNull();
  });

  it("separates the ok and error markers by shape, not by hue", () => {
    renderRun(RUN_OK, [rootSpan("ok"), ERROR_CHILD_SPAN]);

    const run = marker("run-detail-trace-bar");
    const errored = marker("run-detail-trace-span-1");
    expect(run?.textContent).toBeTruthy();
    expect(errored?.textContent).toBeTruthy();
    expect(run?.textContent).not.toBe(errored?.textContent);
  });

  it("reports a failed run's own span as failed, never ok", () => {
    renderRun({ ...RUN_OK, status: "error" }, [
      rootSpan("error"),
      ERROR_CHILD_SPAN,
    ]);

    const bar = within(screen.getByTestId("run-detail-trace-bar"));
    expect(bar.queryByRole("img", { name: "ok" })).toBeNull();
    expect(bar.getByRole("img", { name: "error" })).toHaveTextContent("✗");
  });

  it("still reports a successful run's own span as ok", () => {
    renderRun(RUN_OK, [rootSpan("ok"), OK_CHILD_SPAN]);

    const bar = within(screen.getByTestId("run-detail-trace-bar"));
    expect(bar.getByRole("img", { name: "ok" })).toHaveTextContent("✓");
  });

  it("names the span's kind and nests it under its parent", () => {
    renderRun(RUN_OK, [rootSpan("ok"), OK_CHILD_SPAN]);

    const row = screen.getByTestId("run-detail-trace-span-1");
    expect(row).toHaveAttribute("data-depth", "0");
    expect(
      within(row).getByTestId("run-detail-trace-span-1-kind"),
    ).toHaveTextContent("db");
    expect(row).toHaveTextContent("convex.ctx.db.get");
  });

  it("says so when a run recorded no spans instead of drawing an empty chart", () => {
    renderRun(RUN_OK, []);

    expect(screen.getByTestId("run-detail-trace-empty")).toHaveTextContent(
      "No spans were recorded for this run.",
    );
    expect(screen.queryByTestId("run-detail-trace-span-0")).toBeNull();
    // The run bar still draws from the row's own status and duration.
    expect(marker("run-detail-trace-bar")?.textContent).toBe("✓");
  });
});
