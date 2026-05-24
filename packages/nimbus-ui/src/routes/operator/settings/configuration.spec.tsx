import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { defaultRuntimeDiagnostics } from "../../../test/handlers";
import { ConfigurationSection } from "./configuration";

describe("ConfigurationSection runtime diagnostics", () => {
  it("renders the default V8 lane and fail-closed Bun/JSC lane contract", () => {
    render(
      <ConfigurationSection
        diagnostics={defaultRuntimeDiagnostics}
        license={{
          kind: "community",
          status: "active",
          warnings: [],
        }}
        status={{
          details: {
            adapters: {
              convex: true,
              native: true,
              ui: true,
            },
            authProvider: "admin-local",
          },
        }}
      />,
    );

    expect(screen.getByText("Runtime lanes")).toBeInTheDocument();
    expect(screen.getByTestId("settings-runtime-memory-enforcement"))
      .toHaveTextContent("v8_isolate_heap_limit");

    const defaultLane = screen.getByTestId("settings-runtime-lane-default");
    expect(within(defaultLane).getByText("default")).toBeInTheDocument();
    expect(within(defaultLane).getByText("default lane")).toBeInTheDocument();
    expect(within(defaultLane).getByText("v8")).toBeInTheDocument();
    expect(within(defaultLane).getByText("linked")).toBeInTheDocument();
    expect(within(defaultLane).getByText("lazy")).toBeInTheDocument();
    expect(within(defaultLane).getByText("v8_isolate_heap_limit"))
      .toBeInTheDocument();

    const bunLane = screen.getByTestId("settings-runtime-lane-bun_jsc");
    expect(within(bunLane).getAllByText("bun_jsc").length).toBeGreaterThan(0);
    expect(within(bunLane).getByText("not_linked")).toBeInTheDocument();
    expect(within(bunLane).getByText("lazy")).toBeInTheDocument();
    expect(within(bunLane).getByText("outer_quota_required"))
      .toBeInTheDocument();
  });
});
