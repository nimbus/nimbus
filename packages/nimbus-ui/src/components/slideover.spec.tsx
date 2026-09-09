import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { PanelHeader, Slideover } from "./slideover";

describe("Slideover", () => {
  it("is a modal dialog named after its title", () => {
    render(
      <>
        <button type="button" data-testid="outside">
          Outside
        </button>
        <Slideover title="Edit document" onClose={() => {}} testid="edit">
          <p>body</p>
        </Slideover>
      </>,
    );
    const dialog = screen.getByRole("dialog");
    // Base UI makes everything outside the panel inert instead of setting
    // aria-modal, so the page behind it must be hidden from assistive technology.
    expect(
      screen.getByTestId("outside").closest("[aria-hidden='true']"),
    ).not.toBeNull();
    expect(dialog).toHaveAccessibleName("Edit document");
    expect(screen.getByTestId("edit")).toBe(dialog);
    expect(dialog).toHaveTextContent("body");
  });

  it("closes on Escape and on the header close control", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <Slideover title="Edit document" onClose={onClose}>
        <input aria-label="value" />
      </Slideover>,
    );
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("button", { name: "Close Edit document" }),
    );
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("moves focus inside the panel on open", async () => {
    render(
      <Slideover title="Edit document" onClose={() => {}}>
        <input aria-label="value" />
      </Slideover>,
    );
    await waitFor(() =>
      expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(
        true,
      ),
    );
  });
});

describe("PanelHeader", () => {
  it("names its close control after the panel", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<PanelHeader title="Schema" onClose={onClose} />);
    expect(screen.getByRole("heading", { name: "Schema" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close Schema" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
