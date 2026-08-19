import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ConfirmDialog } from "./confirm-dialog";

function Harness({
  open,
  busy,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <>
      <button type="button" data-testid="opener">
        delete
      </button>
      <button type="button" data-testid="behind">
        start machine-01
      </button>
      <ConfirmDialog
        open={open}
        title='Delete tenant "acme"?'
        description={<p>This action cannot be undone.</p>}
        confirmLabel="Delete"
        danger
        busy={busy}
        // The arrows are deliberate: every caller writes the handlers inline
        // (`onCancel={() => setConfirmTenant(null)}`), so the dialog is handed
        // new functions on every render of the list behind it, and those lists
        // are live subscriptions that re-render on their own.
        onCancel={() => onCancel()}
        onConfirm={() => onConfirm()}
        testid="confirm"
      />
    </>
  );
}

function openDialog() {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  const view = render(
    <Harness
      open={false}
      busy={false}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />,
  );
  const opener = screen.getByTestId("opener");
  opener.focus();
  expect(document.activeElement).toBe(opener);

  const show = (busy = false) => {
    view.rerender(
      <Harness open busy={busy} onCancel={onCancel} onConfirm={onConfirm} />,
    );
  };
  const close = () => {
    view.rerender(
      <Harness
        open={false}
        busy={false}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
  };

  show();
  return { onCancel, onConfirm, opener, show, close };
}

describe("ConfirmDialog", () => {
  it("renders nothing until it is open", () => {
    const onCancel = vi.fn();
    render(
      <Harness
        open={false}
        busy={false}
        onCancel={onCancel}
        onConfirm={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("confirm")).toBeNull();
  });

  it("marks the panel as a modal dialog", () => {
    openDialog();
    const panel = screen.getByTestId("confirm");
    expect(panel).toHaveAttribute("role", "dialog");
    expect(panel).toHaveAttribute("aria-modal", "true");
    expect(panel).toHaveAttribute("aria-label", 'Delete tenant "acme"?');
  });

  it("opens with focus on the least destructive action", () => {
    openDialog();
    // Every caller of this dialog deletes something — a tenant, a machine, a
    // schema, a page of documents — so the confirm button is never the safe
    // place to leave the caret.
    expect(document.activeElement).toBe(screen.getByTestId("confirm-cancel"));
  });

  it("does not put a destructive action under a reflex Enter", async () => {
    const user = userEvent.setup();
    const { onCancel, onConfirm } = openDialog();

    await user.keyboard("{Enter}");

    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("restores focus to the opener when it closes", () => {
    const { opener, close } = openDialog();
    expect(document.activeElement).not.toBe(opener);
    close();
    expect(document.activeElement).toBe(opener);
  });

  it("keeps Tab inside the dialog instead of reaching the page behind", async () => {
    const user = userEvent.setup();
    openDialog();
    const panel = screen.getByTestId("confirm");

    // Sequential navigation, not `focus()`: the DOM focuses anything it is
    // asked to, so only Tab answers whether the rows behind the scrim are
    // still reachable — and reaching them means Enter fires that row's
    // Start/Stop/Delete while the modal is on screen.
    for (let i = 0; i < 6; i += 1) {
      await user.tab();
      expect(panel.contains(document.activeElement)).toBe(true);
    }

    await user.tab({ shift: true });
    expect(panel.contains(document.activeElement)).toBe(true);
  });

  it("keeps focus where the operator put it when the list behind re-renders", () => {
    const { onCancel, show } = openDialog();
    // Deliberately not the button the dialog opens on: the assertion has to
    // fail when the effect re-runs and resets focus, and resetting focus onto
    // the element it already chose would look identical to leaving it alone.
    const confirm = screen.getByTestId("confirm-confirm");
    confirm.focus();

    // A push from the tenants subscription re-renders the list and hands the
    // dialog a new `onCancel`. Pulling the caret off the button the operator
    // moved to means the next Enter answers a different question than the one
    // they aimed at.
    show();

    expect(document.activeElement).toBe(confirm);
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("keeps both actions reachable while the work runs", async () => {
    const user = userEvent.setup();
    const { show } = openDialog();
    const confirm = screen.getByTestId("confirm-confirm");
    const cancel = screen.getByTestId("confirm-cancel");
    confirm.focus();

    show(true);

    // Frozen, not removed from the tab order: `disabled` on the element that
    // currently holds focus hands focus to <body>, which left the operator
    // with a "Working…" dialog on screen, no focus ring anywhere, and Tab
    // restarting from the top of the page behind it.
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    expect(cancel).toHaveAttribute("aria-disabled", "true");
    expect(confirm).not.toBeDisabled();
    expect(cancel).not.toBeDisabled();
    expect(document.activeElement).toBe(confirm);

    await user.tab();
    expect(document.activeElement).toBe(screen.getByLabelText("Dismiss"));
    await user.tab();
    expect(document.activeElement).toBe(cancel);
  });

  it("says the work is running", () => {
    const { show } = openDialog();
    expect(screen.getByTestId("confirm-confirm")).toHaveTextContent("Delete");
    show(true);
    expect(screen.getByTestId("confirm-confirm")).toHaveTextContent("Working…");
  });

  it("cancels through Escape, the backdrop and the header when idle", () => {
    const { onCancel } = openDialog();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByLabelText("Close dialog"));
    expect(onCancel).toHaveBeenCalledTimes(2);

    fireEvent.click(screen.getByLabelText("Dismiss"));
    expect(onCancel).toHaveBeenCalledTimes(3);

    fireEvent.click(screen.getByTestId("confirm-cancel"));
    expect(onCancel).toHaveBeenCalledTimes(4);
  });

  it("refuses every exit while the work runs", () => {
    const { onCancel, show } = openDialog();
    show(true);

    // The caller's loop does not stop when the dialog closes: the storage
    // bulk delete keeps deleting. Dismissing here reads as "cancelled" and
    // takes the only indicator that work is in flight off the screen.
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByLabelText("Close dialog"));
    fireEvent.click(screen.getByLabelText("Dismiss"));
    fireEvent.click(screen.getByTestId("confirm-cancel"));

    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByTestId("confirm")).toBeInTheDocument();
  });

  it("runs the confirmed action once", () => {
    const { onConfirm, show } = openDialog();

    fireEvent.click(screen.getByTestId("confirm-confirm"));
    expect(onConfirm).toHaveBeenCalledTimes(1);

    // The button keeps its tab stop while busy, so refusing the second press
    // is the handler's job rather than the `disabled` attribute's.
    show(true);
    fireEvent.click(screen.getByTestId("confirm-confirm"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});
