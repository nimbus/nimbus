import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { CopyButton } from "./copy-button";

const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: { error: (...args: unknown[]) => toastError(...args) },
}));

function mount(ui: React.ReactElement) {
  return render(<TooltipProvider>{ui}</TooltipProvider>);
}

let writeText: ReturnType<typeof vi.fn>;

// user-event replaces `navigator.clipboard` with its own stub inside
// `setup()`, so the mock the assertions read must be installed after it.
function setup(options?: Parameters<typeof userEvent.setup>[0]) {
  const user = userEvent.setup(options);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
  return user;
}

beforeEach(() => {
  writeText = vi.fn().mockResolvedValue(undefined);
  toastError.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("CopyButton", () => {
  it("names itself after what it copies", () => {
    mount(<CopyButton text="nimbus deploy" label="command" />);
    expect(
      screen.getByRole("button", { name: "Copy command" }),
    ).toBeInTheDocument();
  });

  it("copies the text and reports Copied where a screen reader hears it", async () => {
    const user = setup();
    mount(<CopyButton text="abc123" label="run id" testid="copy" />);
    await user.click(screen.getByTestId("copy"));
    expect(writeText).toHaveBeenCalledWith("abc123");
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    expect(screen.getByTestId("copy")).toHaveAttribute("data-copied", "true");
  });

  it("resolves a text function at click time", async () => {
    const user = setup();
    mount(<CopyButton text={() => "later"} testid="copy" />);
    await user.click(screen.getByTestId("copy"));
    expect(writeText).toHaveBeenCalledWith("later");
  });

  it("returns to the label after two seconds", async () => {
    // A synthetic click keeps user-event's own timed waits out of the fake
    // clock; the act flushes the resolved clipboard promise.
    vi.useFakeTimers();
    mount(<CopyButton text="x" label="id" testid="copy" />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("copy"));
    });
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(screen.getByRole("status")).toHaveTextContent("id");
    expect(screen.getByTestId("copy")).not.toHaveAttribute("data-copied");
  });

  it("reports a copy the browser refused", async () => {
    writeText.mockRejectedValueOnce(new Error("denied"));
    const user = setup();
    mount(<CopyButton text="x" label="token" testid="copy" />);
    await user.click(screen.getByTestId("copy"));
    expect(toastError).toHaveBeenCalledWith(
      "Failed to copy token",
      expect.objectContaining({ description: expect.any(String) }),
    );
    expect(screen.getByTestId("copy")).not.toHaveAttribute("data-copied");
  });
});
