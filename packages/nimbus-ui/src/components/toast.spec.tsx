import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TRANSIENT_TOAST_MS, toast } from "./toast";
import { Toaster } from "./toaster";

// The stack measures each toast in an animation frame, so the clock owns
// those too; see __root.spec.tsx for the same arrangement.
beforeEach(() => {
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
      "requestAnimationFrame",
      "cancelAnimationFrame",
    ],
  });
});

afterEach(() => {
  act(() => {
    toast.close();
    vi.runOnlyPendingTimers();
  });
  vi.useRealTimers();
});

function settle(ms = 0) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
  act(() => {
    vi.advanceTimersByTime(500);
  });
}

// The stack announces every toast a second time through a live region, so a
// text query has to pick the copy inside a toast root.
function toastNamed(text: string): HTMLElement | null {
  for (const node of screen.queryAllByText(text)) {
    const root = node.closest<HTMLElement>('[data-slot="toast"]');
    if (root) return root;
  }
  return null;
}

describe("toast", () => {
  it("draws an icon for a success and an error, none for a message", () => {
    render(<Toaster />);
    act(() => {
      toast.success("Started machine-01");
      toast.error("Start failed");
      toast.message("Copied id");
    });
    settle();

    expect(
      toastNamed("Started machine-01")?.querySelector(
        '[data-slot="toast-icon"]',
      ),
    ).not.toBeNull();
    expect(
      toastNamed("Start failed")?.querySelector('[data-slot="toast-icon"]'),
    ).not.toBeNull();
    expect(
      toastNamed("Copied id")?.querySelector('[data-slot="toast-icon"]'),
    ).toBeNull();
  });

  it("holds an error and lets a confirmation expire", () => {
    render(<Toaster />);
    act(() => {
      toast.success("Started machine-01");
      toast.error("Start failed");
    });
    settle(TRANSIENT_TOAST_MS + 1000);

    expect(toastNamed("Started machine-01")).toBeNull();
    expect(toastNamed("Start failed")).not.toBeNull();
  });

  it("keeps a lifetime the caller named, on either kind", () => {
    render(<Toaster />);
    act(() => {
      toast.error("Delete failed", { timeout: 1000 });
      toast.message("Update available", { timeout: 0 });
    });
    settle(TRANSIENT_TOAST_MS + 1000);

    expect(toastNamed("Delete failed")).toBeNull();
    expect(toastNamed("Update available")).not.toBeNull();
  });

  it("runs the action once, closes the toast, and skips onDismiss", () => {
    const onClick = vi.fn();
    const onDismiss = vi.fn();
    render(<Toaster />);
    act(() => {
      toast.message("Nimbus 0.1.46 available", {
        action: { label: "Update", onClick },
        onDismiss,
        timeout: 0,
      });
    });
    settle();

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Update" }));
    });
    settle(1000);

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onDismiss).not.toHaveBeenCalled();
    expect(toastNamed("Nimbus 0.1.46 available")).toBeNull();
  });

  it("runs onDismiss when the operator closes the toast instead", () => {
    const onDismiss = vi.fn();
    render(<Toaster />);
    act(() => {
      toast.message("Nimbus 0.1.46 available", {
        action: { label: "Update", onClick: vi.fn() },
        onDismiss,
        timeout: 0,
      });
    });
    settle();

    act(() => {
      fireEvent.click(screen.getByLabelText("Close toast"));
    });
    settle(1000);

    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(toastNamed("Nimbus 0.1.46 available")).toBeNull();
  });

  it("runs onDismiss when the clock closes the toast", () => {
    const onDismiss = vi.fn();
    render(<Toaster />);
    act(() => {
      toast.success("Started machine-01", { onDismiss });
    });
    settle(TRANSIENT_TOAST_MS + 1000);

    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("closes one toast by id and every toast with no id", () => {
    render(<Toaster />);
    let first = "";
    act(() => {
      first = toast.error("write 1 failed");
      toast.error("write 2 failed");
    });
    settle();

    act(() => {
      toast.close(first);
    });
    settle(1000);
    expect(toastNamed("write 1 failed")).toBeNull();
    expect(toastNamed("write 2 failed")).not.toBeNull();

    act(() => {
      toast.close();
    });
    settle(1000);
    expect(toastNamed("write 2 failed")).toBeNull();
  });
});
