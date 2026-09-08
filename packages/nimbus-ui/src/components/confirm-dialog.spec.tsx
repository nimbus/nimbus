import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ConfirmDialog } from "./confirm-dialog";

function mount(
  overrides: Partial<React.ComponentProps<typeof ConfirmDialog>> = {},
) {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  const utils = render(
    <>
      <button type="button" data-testid="opener">
        Delete
      </button>
      <ConfirmDialog
        open
        title="Delete machine"
        description="This removes m-1 from the fleet."
        confirmLabel="Delete"
        danger
        onConfirm={onConfirm}
        onCancel={onCancel}
        testid="confirm"
        {...overrides}
      />
    </>,
  );
  return { ...utils, onConfirm, onCancel };
}

describe("ConfirmDialog", () => {
  it("renders nothing while closed", () => {
    mount({ open: false });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("is a modal dialog with the title, the description, and the no-undo line", () => {
    mount();
    const dialog = screen.getByRole("dialog");
    // Base UI makes everything outside the dialog inert instead of setting
    // aria-modal, so the opener must be hidden from assistive technology.
    expect(
      screen.getByTestId("opener").closest("[aria-hidden='true']"),
    ).not.toBeNull();
    expect(dialog).toHaveAccessibleName("Delete machine");
    expect(screen.getByTestId("confirm-description")).toHaveTextContent(
      "This removes m-1 from the fleet.",
    );
    expect(dialog).toHaveTextContent("There is no undo.");
  });

  it("starts with focus on Cancel so Enter on an unread dialog does nothing", async () => {
    const user = userEvent.setup();
    const { onConfirm, onCancel } = mount();
    await waitFor(() =>
      expect(screen.getByTestId("confirm-cancel")).toHaveFocus(),
    );
    await user.keyboard("{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels on Escape, on the close control, and on Cancel", async () => {
    const user = userEvent.setup();
    const { onCancel } = mount();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(screen.getByTestId("confirm-cancel"));
    expect(onCancel).toHaveBeenCalledTimes(3);
  });

  it("confirms once per click and paints the destructive button when danger", async () => {
    const user = userEvent.setup();
    const { onConfirm } = mount();
    const confirm = screen.getByTestId("confirm-confirm");
    expect(confirm).toHaveAttribute("data-variant", "destructive");
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("refuses every dismissal while busy and keeps both buttons focusable", async () => {
    const user = userEvent.setup();
    const { onCancel, onConfirm } = mount({ busy: true });
    const cancel = screen.getByTestId("confirm-cancel");
    const confirm = screen.getByTestId("confirm-confirm");
    expect(cancel).toHaveAttribute("aria-disabled", "true");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    expect(cancel).not.toBeDisabled();
    expect(confirm).toHaveTextContent("Working…");
    await user.keyboard("{Escape}");
    await user.click(cancel);
    await user.click(confirm);
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("keeps Confirm inert until the typed phrase matches, then confirms on Enter too", async () => {
    const user = userEvent.setup();
    const { onConfirm } = mount({ typedConfirmation: { phrase: "shutdown" } });
    const confirm = screen.getByTestId("confirm-confirm");
    const field = screen.getByTestId("confirm-typed");
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "Type shutdown to confirm",
    );
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    expect(confirm).not.toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();

    await user.type(field, "shut");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    await user.type(field, "down");
    expect(confirm).toHaveAttribute("aria-disabled", "false");
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.type(field, "{Enter}");
    expect(onConfirm).toHaveBeenCalledTimes(2);
  });

  it("keeps Confirm inert while the caller's own gate is closed", async () => {
    const user = userEvent.setup();
    const { onConfirm } = mount({ confirmDisabled: true });
    const confirm = screen.getByTestId("confirm-confirm");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    expect(confirm).toHaveTextContent("Delete");
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("shows a refused write in the error strip", () => {
    mount({ error: "machine is still draining" });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "machine is still draining",
    );
  });
});
