import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Slideover } from "./slideover";

function Harness({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <>
      <button type="button" data-testid="opener">
        edit
      </button>
      <button type="button" data-testid="behind">
        behind the drawer
      </button>
      {open ? (
        <Slideover title="Edit document" onClose={onClose} testid="drawer">
          <textarea data-testid="editor" />
          <button type="button" data-testid="save">
            save
          </button>
        </Slideover>
      ) : null}
    </>
  );
}

function openDrawer(onClose = vi.fn()) {
  const { rerender } = render(<Harness open={false} onClose={onClose} />);
  const opener = screen.getByTestId("opener");
  opener.focus();
  expect(document.activeElement).toBe(opener);
  rerender(<Harness open onClose={onClose} />);
  return { onClose, opener, rerender };
}

describe("Slideover", () => {
  it("marks the panel as a modal dialog", () => {
    openDrawer();
    const panel = screen.getByTestId("drawer");
    expect(panel).toHaveAttribute("role", "dialog");
    expect(panel).toHaveAttribute("aria-modal", "true");
    expect(panel).toHaveAttribute("aria-label", "Edit document");
  });

  it("moves focus into the panel on open", () => {
    openDrawer();
    expect(document.activeElement).toBe(screen.getByTestId("drawer"));
  });

  it("keeps Tab inside the panel instead of reaching the page behind", () => {
    openDrawer();
    const panel = screen.getByTestId("drawer");
    const close = screen.getByLabelText("Close Edit document");
    const editor = screen.getByTestId("editor");
    const save = screen.getByTestId("save");

    // From the panel itself, Shift+Tab wraps to the last focusable child
    // rather than escaping backwards to the dismiss overlay.
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(save);

    // Forward from the last child wraps to the first.
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(close);

    // Focus that somehow lands outside is pulled back in.
    screen.getByTestId("behind").focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(close);

    expect(panel.contains(document.activeElement)).toBe(true);
    expect([close, editor, save]).toContain(document.activeElement);
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    openDrawer(onClose);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("restores focus to the opener when it closes", () => {
    const onClose = vi.fn();
    const { opener, rerender } = openDrawer(onClose);
    expect(document.activeElement).not.toBe(opener);
    rerender(<Harness open={false} onClose={onClose} />);
    expect(document.activeElement).toBe(opener);
  });
});
