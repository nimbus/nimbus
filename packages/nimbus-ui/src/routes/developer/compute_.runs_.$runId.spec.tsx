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

const INFO_EVENT = {
  _id: "evt-info",
  _creationTime: NOW,
  createdAt: NOW + 10,
  level: "info",
  source: "runtime",
  category: "invoke",
  message: "handler entered",
};

const ERROR_EVENT = {
  _id: "evt-error",
  _creationTime: NOW,
  createdAt: NOW + 40,
  level: "error",
  source: "runtime",
  category: "invoke",
  message: "db read failed",
};

function renderRun(
  run: Record<string, unknown>,
  events: Record<string, unknown>[] = [],
) {
  const route = Route as unknown as Record<string, unknown>;
  route.useParams = () => ({ runId: "runs:1" });
  route.useLoaderData = () => ({ run, events });
  const Component = routeComponent(Route);
  return render(<Component />);
}

function marker(testid: string) {
  return within(screen.getByTestId(testid)).queryByRole("img");
}

// The waterfall carried a span's level in the bar's hue and nowhere else, so a
// reader who cannot separate the danger and muted hues saw two identical bars.
// These assertions read the glyph and its accessible name — never a class — so
// they fail if the fix ever collapses back onto color.
describe("run detail trace waterfall", () => {
  it("marks an errored span with a glyph and names it to assistive tech", () => {
    renderRun(RUN_OK, [INFO_EVENT, ERROR_EVENT]);

    const errored = within(
      screen.getByTestId("run-detail-trace-span-evt-error"),
    ).getByRole("img", { name: "error" });
    expect(errored).toHaveTextContent("✗");
  });

  it("leaves a non-errored span unmarked", () => {
    renderRun(RUN_OK, [INFO_EVENT, ERROR_EVENT]);

    expect(marker("run-detail-trace-span-evt-info")).toBeNull();
    expect(
      screen.queryByTestId("run-detail-trace-span-evt-info-marker"),
    ).toBeNull();
  });

  it("separates the ok and error markers by shape, not by hue", () => {
    renderRun(RUN_OK, [ERROR_EVENT]);

    const run = marker("run-detail-trace-bar");
    const errored = marker("run-detail-trace-span-evt-error");
    expect(run?.textContent).toBeTruthy();
    expect(errored?.textContent).toBeTruthy();
    expect(run?.textContent).not.toBe(errored?.textContent);
  });

  it("reports a failed run's own span as failed, never ok", () => {
    renderRun({ ...RUN_OK, status: "error" }, [ERROR_EVENT]);

    const bar = within(screen.getByTestId("run-detail-trace-bar"));
    expect(bar.queryByRole("img", { name: "ok" })).toBeNull();
    expect(bar.getByRole("img", { name: "error" })).toHaveTextContent("✗");
  });

  it("still reports a successful run's own span as ok", () => {
    renderRun(RUN_OK, [INFO_EVENT]);

    const bar = within(screen.getByTestId("run-detail-trace-bar"));
    expect(bar.getByRole("img", { name: "ok" })).toHaveTextContent("✓");
  });
});
