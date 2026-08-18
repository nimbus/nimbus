import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";

import { ScrollRegion } from "./scroll-region";

/**
 * The contract is keyboard reach, so the assertions are about the tab stop and
 * the accessible name rather than about classes. A read-only list — schedules,
 * routes, the event log — has no focusable descendant, so without its own tab
 * stop everything below the fold is mouse-only.
 */
describe("ScrollRegion", () => {
  it("is a named region with its own tab stop", () => {
    render(
      <ScrollRegion label="Scheduled jobs">
        <p>row</p>
      </ScrollRegion>,
    );
    const region = screen.getByRole("region", { name: "Scheduled jobs" });
    expect(region).toHaveAttribute("tabindex", "0");
  });

  it("scrolls, and leaves sizing to the caller", () => {
    render(
      <ScrollRegion label="Routes" className="h-full">
        <p>row</p>
      </ScrollRegion>,
    );
    const region = screen.getByRole("region", { name: "Routes" });
    // `overflow-auto` is the component's; `h-full` is the caller's. Both have
    // to survive the merge — `overflow-auto` alone resolves to `height: auto`
    // under a non-flex panel and never scrolls.
    expect(region.className).toContain("overflow-auto");
    expect(region.className).toContain("h-full");
  });

  it("forwards a ref, so a caller can still measure the box", () => {
    const ref = createRef<HTMLElement>();
    render(
      <ScrollRegion label="Routes" ref={ref} data-testid="routes-scroll">
        <p>row</p>
      </ScrollRegion>,
    );
    expect(ref.current).toBe(screen.getByTestId("routes-scroll"));
  });
});
