import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { LoadFailed } from "./load-failed";

describe("LoadFailed", () => {
  it("is an alert that names what is missing", () => {
    render(<LoadFailed what="machines" />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not load machines",
    );
  });

  it("quotes an Error message in mono and a string as is", () => {
    const { unmount } = render(
      <LoadFailed what="runs" error={new Error("502 from engine")} />,
    );
    expect(screen.getByText("502 from engine")).toHaveClass("font-mono");
    unmount();
    render(<LoadFailed what="runs" error="socket closed" />);
    expect(screen.getByText("socket closed")).toBeInTheDocument();
  });

  it("offers Try again only when a retry exists, and fires it", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<LoadFailed what="runs" />);
    expect(screen.queryByRole("button", { name: /try again/i })).toBeNull();
    unmount();
    const onRetry = vi.fn();
    render(<LoadFailed what="runs" onRetry={onRetry} />);
    await user.click(screen.getByRole("button", { name: /try again/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
