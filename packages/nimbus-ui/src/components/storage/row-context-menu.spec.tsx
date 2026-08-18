import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { RowContextMenu, type RowMenuItem } from "./row-context-menu";

function items(onSelect = vi.fn()): RowMenuItem[] {
  return [
    { id: "edit", label: "Edit document", onSelect },
    { id: "copy", label: "Copy _id", hint: "doc_abc…", onSelect: vi.fn() },
    { id: "delete", label: "Delete document", danger: true, onSelect: vi.fn() },
  ];
}

describe("RowContextMenu", () => {
  it("moves focus to the first item on open", () => {
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row actions"
        items={items()}
        onClose={vi.fn()}
        testid="row-menu"
      />,
    );
    expect(screen.getByTestId("row-menu-edit")).toHaveFocus();
  });

  it("runs the item and closes on select", async () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row actions"
        items={items(onSelect)}
        onClose={onClose}
        testid="row-menu"
      />,
    );

    await user.click(screen.getByTestId("row-menu-edit"));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  it("wraps arrow-key focus around the item list", async () => {
    const user = userEvent.setup();
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row actions"
        items={items()}
        onClose={vi.fn()}
        testid="row-menu"
      />,
    );

    await user.keyboard("{ArrowDown}");
    expect(screen.getByTestId("row-menu-copy")).toHaveFocus();
    await user.keyboard("{ArrowUp}{ArrowUp}");
    expect(screen.getByTestId("row-menu-delete")).toHaveFocus();
  });

  // One Escape must close exactly one thing. The slideover and the shell
  // keyboard contract both listen on `window`, so the menu takes the event in
  // the capture phase and stops it there.
  it("closes on Escape without letting the event reach the shell", () => {
    const onClose = vi.fn();
    const shell = vi.fn();
    window.addEventListener("keydown", shell);
    try {
      render(
        <RowContextMenu
          x={10}
          y={10}
          label="Row actions"
          items={items()}
          onClose={onClose}
          testid="row-menu"
        />,
      );
      fireEvent.keyDown(screen.getByTestId("row-menu-edit"), {
        key: "Escape",
      });
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(shell).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", shell);
    }
  });

  it("closes on an outside pointer press and on a scroll", () => {
    for (const fire of [
      () => fireEvent.pointerDown(document.body),
      () => fireEvent.scroll(document.body),
    ]) {
      const onClose = vi.fn();
      const view = render(
        <RowContextMenu
          x={10}
          y={10}
          label="Row actions"
          items={items()}
          onClose={onClose}
          testid="row-menu"
        />,
      );
      fire();
      expect(onClose).toHaveBeenCalled();
      view.unmount();
    }
  });

  // Dismissal must not key off `contextmenu`: in a real browser the menu is
  // mounted mid-dispatch of the right-click that opened it, and the same event
  // then reaches `window`. Measured in Chromium before this changed — the menu
  // opened and closed inside one event, so right-click appeared dead.
  it("survives the contextmenu event that opened it", () => {
    const onClose = vi.fn();
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row actions"
        items={items()}
        onClose={onClose}
        testid="row-menu"
      />,
    );

    fireEvent.contextMenu(document.body);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("row-menu")).toBeInTheDocument();
  });

  it("keeps a pointer press inside the menu from closing it early", () => {
    const onClose = vi.fn();
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row actions"
        items={items()}
        onClose={onClose}
        testid="row-menu"
      />,
    );

    fireEvent.pointerDown(screen.getByTestId("row-menu-edit"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("returns focus to the row that opened it", () => {
    const row = document.createElement("button");
    document.body.appendChild(row);
    try {
      const view = render(
        <RowContextMenu
          x={10}
          y={10}
          label="Row actions"
          items={items()}
          restoreFocus={row}
          onClose={vi.fn()}
          testid="row-menu"
        />,
      );
      expect(screen.getByTestId("row-menu-edit")).toHaveFocus();
      view.unmount();
      expect(row).toHaveFocus();
    } finally {
      row.remove();
    }
  });

  // A menu raised on the last row or against the right edge must stay on
  // screen; the position is a request, not a guarantee.
  it("clamps its position to the viewport", () => {
    render(
      <RowContextMenu
        x={window.innerWidth + 500}
        y={window.innerHeight + 500}
        label="Row actions"
        items={items()}
        onClose={vi.fn()}
        testid="row-menu"
      />,
    );
    const menu = screen.getByTestId("row-menu");
    expect(Number.parseFloat(menu.style.left)).toBeLessThanOrEqual(
      window.innerWidth,
    );
    expect(Number.parseFloat(menu.style.top)).toBeLessThanOrEqual(
      window.innerHeight,
    );
  });

  it("exposes an accessible menu with one menuitem per action", () => {
    render(
      <RowContextMenu
        x={10}
        y={10}
        label="Row 3 actions"
        items={items()}
        onClose={vi.fn()}
        testid="row-menu"
      />,
    );
    expect(
      screen.getByRole("menu", { name: "Row 3 actions" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("menuitem")).toHaveLength(3);
  });
});
