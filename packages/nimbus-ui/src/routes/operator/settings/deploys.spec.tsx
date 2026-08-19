import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { formatAbsoluteTime } from "../../../lib/format";
import { DeploysSection } from "./-deploys";

describe("DeploysSection copy", () => {
  // The section description was authored as markdown and dropped into a text
  // node, so the user read a literal ` around the command. Commands are marked
  // up as <code>.
  it("marks the deploy command up as <code> and leaks no backticks", () => {
    render(<DeploysSection bundles={[]} functions={[]} />);

    const section = screen.getByTestId("settings-deploys");
    expect(section.textContent).not.toContain("`");

    const description = section.querySelector("header p");
    expect(description?.textContent).toContain(
      "Trigger new deploys with nimbus deploy from the CLI.",
    );

    const command = description?.querySelector("code");
    expect(command?.textContent).toBe("nimbus deploy");
    // A wrapped multi-word command renders as two separate boxed fragments.
    expect(command?.className).toContain("whitespace-nowrap");
  });

  it("leaks no backticks in the no-bundles empty state", () => {
    render(<DeploysSection bundles={[]} functions={[]} />);

    const empty = screen.getByTestId("settings-deploys-empty");
    expect(empty.textContent).not.toContain("`");
    expect(empty.querySelector("code")?.textContent).toBe("nimbus deploy");
  });
});

describe("DeploysSection history timestamps", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  // The active bundle is normally the newest bundle, so its `_creationTime` is
  // on screen twice: once in the panel and once in the history row below it.
  // The row formatted the value itself, once, at render -- so the panel walked
  // on to "1m ago" while the row still read "just now", and the row had no
  // hover tooltip. Both go through `RelativeTime` now.
  it("re-ticks the history row in step with the active panel", () => {
    vi.useFakeTimers();
    const created = Date.now();
    render(
      <DeploysSection
        bundles={[
          {
            _id: "bundle1",
            _creationTime: created,
            sha256: "a".repeat(64),
            status: "active",
          },
        ]}
        functions={[]}
      />,
    );

    const historyTime = screen
      .getByTestId("settings-deploys-history")
      .querySelector("time");
    const activeTime = screen
      .getByTestId("settings-deploys-active")
      .querySelector("time");
    expect(historyTime?.textContent).toBe("just now");
    expect(activeTime?.textContent).toBe("just now");

    act(() => {
      vi.advanceTimersByTime(60_000);
    });

    expect(activeTime?.textContent).toBe("1m ago");
    expect(historyTime?.textContent).toBe("1m ago");
  });

  it("gives the history row the absolute time on hover", () => {
    const created = Date.UTC(2026, 4, 15, 9, 30, 0);
    render(
      <DeploysSection
        bundles={[
          { _id: "bundle1", _creationTime: created, sha256: "b".repeat(64) },
        ]}
        functions={[]}
      />,
    );

    const historyTime = screen
      .getByTestId("settings-deploys-history")
      .querySelector("time");
    expect(historyTime?.getAttribute("title")).toBe(
      formatAbsoluteTime(created),
    );
    expect(historyTime?.getAttribute("datetime")).toBe(
      new Date(created).toISOString(),
    );
  });

  it("still renders an em dash for a bundle with no creation time", () => {
    render(
      <DeploysSection
        bundles={[{ _id: "bundle1", sha256: "c".repeat(64) }]}
        functions={[]}
      />,
    );

    const history = screen.getByTestId("settings-deploys-history");
    expect(history.querySelector("time")).toBeNull();
    expect(history.textContent).toContain("—");
  });
});
