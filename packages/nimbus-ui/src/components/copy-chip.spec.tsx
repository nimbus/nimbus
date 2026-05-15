import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { toastMock } = vi.hoisted(() => ({
  toastMock: Object.assign(vi.fn(), {
    error: vi.fn(),
  }),
}));

vi.mock("sonner", () => ({
  toast: toastMock,
}));

import { CopyChip } from "./copy-chip";

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  toastMock.mockClear();
  toastMock.error.mockClear();
});

describe("CopyChip", () => {
  it("renders the value when no children supplied", () => {
    render(<CopyChip label="bundle" value="abc123" />);
    expect(screen.getByRole("button")).toHaveTextContent("abc123");
  });

  it("renders custom children over the raw value", () => {
    render(
      <CopyChip label="bundle" value="abc123">
        copy
      </CopyChip>,
    );
    expect(screen.getByRole("button")).toHaveTextContent("copy");
  });

  it("exposes an accessible label tying value + intent", () => {
    render(<CopyChip label="bundle" value="abc123" />);
    expect(screen.getByRole("button")).toHaveAttribute(
      "aria-label",
      "Copy bundle: abc123",
    );
  });

  it("writes to the clipboard and toasts on click", async () => {
    render(<CopyChip label="bundle" value="abc123" testid="chip" />);
    fireEvent.click(screen.getByTestId("chip"));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("abc123");
    });
    expect(toastMock).toHaveBeenCalledWith("Copied bundle", {
      description: "abc123",
    });
    await waitFor(() => {
      expect(screen.getByTestId("chip")).toHaveAttribute("data-copied", "true");
    });
    await waitFor(
      () => {
        expect(screen.getByTestId("chip")).not.toHaveAttribute("data-copied");
      },
      { timeout: 2000 },
    );
  });

  it("toasts an error when the clipboard call rejects", async () => {
    writeText.mockRejectedValueOnce(new Error("denied"));
    render(<CopyChip label="bundle" value="abc123" />);
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith("Failed to copy bundle");
    });
  });
});
