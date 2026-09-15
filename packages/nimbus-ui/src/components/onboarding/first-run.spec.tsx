import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { FirstRun, firstRunComplete } from "./first-run";

function renderPanel(progress: { functions: number; runs: number }) {
  return render(
    <TooltipProvider>
      <FirstRun progress={progress} />
    </TooltipProvider>,
  );
}

function doneFlags(): Record<string, string | null> {
  return Object.fromEntries(
    ["install", "dev", "run"].map((id) => [
      id,
      screen.getByTestId(`first-run-step-${id}`).getAttribute("data-done"),
    ]),
  );
}

describe("FirstRun step transitions", () => {
  it("starts with every step open and the two commands ready to copy", () => {
    renderPanel({ functions: 0, runs: 0 });

    expect(doneFlags()).toEqual({
      install: "false",
      dev: "false",
      run: "false",
    });
    expect(screen.getByTestId("first-run-progress")).toHaveTextContent(
      "0 of 3 steps done",
    );
    expect(screen.getByTestId("first-run-step-install")).toHaveTextContent(
      "brew install nimbus/tap/nimbus",
    );
    expect(screen.getByTestId("first-run-step-dev")).toHaveTextContent(
      "nimbus init convex my-app && cd my-app && nimbus dev",
    );
    expect(
      screen.getAllByRole("button", { name: "Copy command" }),
    ).toHaveLength(2);
  });

  it("closes install and dev together on the first deployed function", () => {
    renderPanel({ functions: 1, runs: 0 });

    expect(doneFlags()).toEqual({
      install: "true",
      dev: "true",
      run: "false",
    });
    expect(screen.getByTestId("first-run-progress")).toHaveTextContent(
      "2 of 3 steps done",
    );
    // A closed step retires its command; nothing is left to copy.
    expect(
      screen.queryAllByRole("button", { name: "Copy command" }),
    ).toHaveLength(0);
  });

  it("closes the run step on the first recorded run", () => {
    renderPanel({ functions: 1, runs: 1 });

    expect(doneFlags()).toEqual({ install: "true", dev: "true", run: "true" });
    expect(screen.getByTestId("first-run-progress")).toHaveTextContent(
      "3 of 3 steps done",
    );
  });

  it("does not let a run close the deploy steps on its own", () => {
    // A run recorded against a function that has since been removed still
    // counts as a run; the deploy steps stay open until a function exists.
    renderPanel({ functions: 0, runs: 3 });

    expect(doneFlags()).toEqual({
      install: "false",
      dev: "false",
      run: "true",
    });
  });
});

describe("firstRunComplete", () => {
  it("is true only when a function exists and has run", () => {
    expect(firstRunComplete({ functions: 0, runs: 0 })).toBe(false);
    expect(firstRunComplete({ functions: 1, runs: 0 })).toBe(false);
    expect(firstRunComplete({ functions: 0, runs: 1 })).toBe(false);
    expect(firstRunComplete({ functions: 1, runs: 1 })).toBe(true);
  });
});
