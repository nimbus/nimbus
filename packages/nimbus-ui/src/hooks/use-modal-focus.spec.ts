import { fireEvent, renderHook } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useModalFocus } from "./use-modal-focus";

/**
 * Contract cases the two callers cannot reach.
 *
 * `Slideover` and `ConfirmDialog` cover the everyday path — open, trap, close,
 * restore — in their own specs. What is left here is the behaviour neither of
 * them can produce: a closed modal, a panel with nothing focusable inside it,
 * and an opener the caller grays out while its work runs.
 */

function addButton(label: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = label;
  document.body.append(button);
  return button;
}

function mountPanel(): {
  panel: HTMLDivElement;
  ref: { current: HTMLElement };
} {
  const panel = document.createElement("div");
  // Panels take programmatic focus the same way the two components do.
  panel.tabIndex = -1;
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.textContent = "cancel";
  panel.append(cancel);
  document.body.append(panel);
  return { panel, ref: { current: panel } };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("useModalFocus", () => {
  it("installs nothing while it is closed", () => {
    const { ref } = mountPanel();
    const opener = addButton("delete tenant");
    opener.focus();
    const onEscape = vi.fn();

    renderHook(() => useModalFocus({ open: false, panelRef: ref, onEscape }));

    // A closed dialog that still answered Escape would cancel whatever the
    // operator is doing on the page behind it, and one that still moved focus
    // would take the caret out of the field they are typing in.
    expect(document.activeElement).toBe(opener);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onEscape).not.toHaveBeenCalled();
  });

  it("keeps Tab on a panel that has nothing focusable inside it", async () => {
    const user = userEvent.setup();
    const panel = document.createElement("div");
    panel.tabIndex = -1;
    document.body.append(panel);
    const behind = addButton("start machine-01");

    renderHook(() =>
      useModalFocus({
        open: true,
        panelRef: { current: panel },
        onEscape: vi.fn(),
      }),
    );

    // A dialog whose body has not rendered its controls yet is still modal.
    // Without this branch Tab walks straight to the row behind the scrim and
    // Enter fires that row's action.
    expect(document.activeElement).toBe(panel);
    await user.tab();
    expect(document.activeElement).toBe(panel);
    expect(document.activeElement).not.toBe(behind);
  });

  it("hands focus back to an opener that is only aria-disabled", () => {
    const { panel, ref } = mountPanel();
    const opener = addButton("delete tenant");
    opener.focus();

    const { rerender } = renderHook(
      ({ open }: { open: boolean }) =>
        useModalFocus({ open, panelRef: ref, onEscape: vi.fn() }),
      { initialProps: { open: true } },
    );
    expect(document.activeElement).not.toBe(opener);

    // The caller grays out the row control for as long as the delete runs.
    // `aria-disabled` says so without taking the element out of the focus
    // order of the document, so the restore below still has a target.
    opener.setAttribute("aria-disabled", "true");
    // React has already removed the panel by the time the cleanup runs, so the
    // caret has nowhere to sit unless the restore finds its target.
    panel.remove();
    rerender({ open: false });

    expect(document.activeElement).toBe(opener);
  });

  it("cannot restore focus to an opener the caller disabled", () => {
    const { panel, ref } = mountPanel();
    const opener = addButton("delete tenant");
    opener.focus();

    const { rerender } = renderHook(
      ({ open }: { open: boolean }) =>
        useModalFocus({ open, panelRef: ref, onEscape: vi.fn() }),
      { initialProps: { open: true } },
    );

    // The reason the routes use `aria-disabled` on a row control they gray
    // out. A disabled element cannot take focus, so the restore is a silent
    // no-op and the operator is left on <body>: no focus ring anywhere and
    // Tab starting again from the top of the page.
    opener.disabled = true;
    panel.remove();
    rerender({ open: false });

    expect(document.activeElement).not.toBe(opener);
    expect(document.activeElement).toBe(document.body);
  });
});
