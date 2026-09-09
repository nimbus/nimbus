import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LoadingCell } from "./loading-cell";

describe("LoadingCell", () => {
  it("renders the child for an ok value", () => {
    render(
      <LoadingCell value={{ kind: "ok", value: 42 }} testid="cell">
        {(n) => <b>{n * 2}</b>}
      </LoadingCell>,
    );
    expect(screen.getByText("84")).toBeInTheDocument();
    expect(screen.queryByTestId("cell-loading")).toBeNull();
  });

  it("shows a hidden placeholder while loading", () => {
    render(
      <LoadingCell value={{ kind: "loading" }} testid="cell">
        {() => "never"}
      </LoadingCell>,
    );
    const dot = screen.getByTestId("cell-loading");
    expect(dot).toHaveAttribute("aria-hidden", "true");
    expect(dot).toHaveTextContent("·");
    expect(screen.queryByText("never")).toBeNull();
  });

  it("names the offline state", () => {
    render(
      <LoadingCell value={{ kind: "offline" }} testid="cell">
        {() => "never"}
      </LoadingCell>,
    );
    expect(screen.getByTestId("cell-offline")).toHaveTextContent("offline");
  });

  it("prints the error message in the error colour", () => {
    render(
      <LoadingCell value={{ kind: "error", message: "boom" }} testid="cell">
        {() => "never"}
      </LoadingCell>,
    );
    const cell = screen.getByTestId("cell-error");
    expect(cell).toHaveTextContent("boom");
    expect(cell).toHaveAttribute("title", "boom");
    expect(cell.className).toContain("text-error");
  });
});
