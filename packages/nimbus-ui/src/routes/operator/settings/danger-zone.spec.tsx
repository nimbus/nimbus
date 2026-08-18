import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

import { DangerZoneSection } from "./danger-zone";

describe("DangerZoneSection copy", () => {
  // A placeholder is an HTML attribute, so it cannot hold a <code> element.
  // The command is worded as plain prose instead of markdown.
  it("states the token command as plain text, with no markdown backticks", () => {
    render(<DangerZoneSection />);
    fireEvent.click(screen.getByTestId("settings-rotate-open"));

    expect(screen.getByTestId("settings-rotate-token")).toHaveAttribute(
      "placeholder",
      "Paste the token printed by nimbus token show",
    );
  });

  it("leaks no backticks anywhere in the section or its dialogs", () => {
    render(<DangerZoneSection />);
    expect(
      screen.getByTestId("settings-danger-zone").textContent,
    ).not.toContain("`");

    fireEvent.click(screen.getByTestId("settings-rotate-open"));
    const rotate = screen.getByTestId("settings-rotate-dialog");
    expect(rotate.textContent).not.toContain("`");
    expect(
      screen.getByTestId("settings-rotate-token").getAttribute("placeholder"),
    ).not.toContain("`");

    fireEvent.click(screen.getByTestId("settings-shutdown-open"));
    const shutdown = screen.getByTestId("settings-shutdown-dialog");
    expect(shutdown.textContent).not.toContain("`");
    // The shutdown copy already marked its command up correctly — keep it that
    // way so the two dialogs stay consistent.
    expect(
      Array.from(shutdown.querySelectorAll("code")).map((el) => el.textContent),
    ).toEqual(["nimbus start", "nimbus start"]);
  });
});
