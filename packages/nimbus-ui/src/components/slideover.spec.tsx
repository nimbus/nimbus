import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
        // The arrow is deliberate: every caller writes the handler inline
        // (`onClose={() => setEditing(null)}`), so the drawer is handed a
        // different function on every render of the route behind it.
        <Slideover
          title="Edit document"
          onClose={() => onClose()}
          testid="drawer"
        >
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

  it("does not leak the tab order into the page behind", async () => {
    const user = userEvent.setup();
    openDrawer();
    const panel = screen.getByTestId("drawer");

    // Sequential navigation, not `focus()`: the DOM focuses anything it is
    // asked to, so only Tab answers whether the page behind is still
    // reachable. Six stops is two full cycles of the drawer's three controls.
    for (let i = 0; i < 6; i += 1) {
      await user.tab();
      expect(panel.contains(document.activeElement)).toBe(true);
    }
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    openDrawer(onClose);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the caret in the editor when the route re-renders", () => {
    const { onClose, rerender } = openDrawer();
    const editor = screen.getByTestId("editor");
    editor.focus();

    // The route behind the drawer re-renders on every push from its live
    // table subscription — another tab's write, a function's write, the
    // operator's own patch — and each render hands the drawer a new
    // `onClose`. That must not move the caret out of the JSON being typed.
    rerender(<Harness open onClose={onClose} />);

    expect(document.activeElement).toBe(editor);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes through the newest onClose after a re-render", () => {
    const stale = vi.fn();
    const { rerender } = openDrawer(stale);
    const current = vi.fn();
    rerender(<Harness open onClose={current} />);

    // The handler is held in a ref rather than an effect dependency, so it
    // has to be refreshed on every render or Escape would call the closure
    // captured when the drawer opened.
    fireEvent.keyDown(window, { key: "Escape" });
    expect(current).toHaveBeenCalledTimes(1);
    expect(stale).not.toHaveBeenCalled();
  });

  it("restores focus to the opener when it closes", () => {
    const onClose = vi.fn();
    const { opener, rerender } = openDrawer(onClose);
    expect(document.activeElement).not.toBe(opener);
    rerender(<Harness open={false} onClose={onClose} />);
    expect(document.activeElement).toBe(opener);
  });
});
