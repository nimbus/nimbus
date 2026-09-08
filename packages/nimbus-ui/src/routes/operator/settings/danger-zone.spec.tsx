import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { rotateToken, shutdown, toast } = vi.hoisted(() => ({
  rotateToken: vi.fn(),
  shutdown: vi.fn(),
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

vi.mock("sonner", () => ({ toast }));
vi.mock("../../../lib/api-mutations", () => ({
  system: { rotateToken, shutdown },
}));

import { DangerZoneSection, SHUTDOWN_PHRASE } from "./-danger-zone";

beforeEach(() => {
  rotateToken.mockReset();
  shutdown.mockReset();
});

describe("DangerZoneSection copy", () => {
  // A placeholder is an HTML attribute, so it cannot hold a <code> element.
  // The command is worded as plain prose instead of markdown.
  it("states the token command as plain text, with no markdown backticks", async () => {
    const user = userEvent.setup();
    render(<DangerZoneSection />);
    await user.click(screen.getByTestId("settings-rotate-open"));

    expect(screen.getByTestId("settings-rotate-token")).toHaveAttribute(
      "placeholder",
      "Paste the token printed by nimbus token show",
    );
  });

  it("leaks no backticks anywhere in the section or its dialogs", async () => {
    const user = userEvent.setup();
    render(<DangerZoneSection />);
    expect(
      screen.getByTestId("settings-danger-zone").textContent,
    ).not.toContain("`");

    await user.click(screen.getByTestId("settings-rotate-open"));
    const rotate = screen.getByTestId("settings-rotate-dialog");
    expect(rotate.textContent).not.toContain("`");
    expect(
      screen.getByTestId("settings-rotate-token").getAttribute("placeholder"),
    ).not.toContain("`");
    await user.click(screen.getByTestId("settings-rotate-dialog-cancel"));

    await user.click(screen.getByTestId("settings-shutdown-open"));
    const shutdownDialog = screen.getByTestId("settings-shutdown-dialog");
    expect(shutdownDialog.textContent).not.toContain("`");
    // The commands and the typed phrase are marked up as code, so the two
    // dialogs stay consistent with each other.
    expect(
      Array.from(shutdownDialog.querySelectorAll("code")).map(
        (el) => el.textContent,
      ),
    ).toEqual(["nimbus start", "nimbus start", SHUTDOWN_PHRASE]);
  });
});

// Both writes reach past this browser, so each one runs through
// ConfirmDialog with a typed proof: the bearer for rotation, the phrase for
// shutdown. Confirm stays inert until the proof is present.
describe("DangerZoneSection typed confirmation", () => {
  it("keeps shutdown inert until the phrase is typed, then reports acceptance on the page", async () => {
    const user = userEvent.setup();
    shutdown.mockResolvedValue({ ok: true, data: { accepted: true } });
    render(<DangerZoneSection />);
    await user.click(screen.getByTestId("settings-shutdown-open"));

    const dialog = screen.getByTestId("settings-shutdown-dialog");
    expect(dialog).toHaveAccessibleName("Shut down server");
    const confirm = screen.getByTestId("settings-shutdown-dialog-confirm");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    await user.click(confirm);
    expect(shutdown).not.toHaveBeenCalled();

    await user.type(
      screen.getByTestId("settings-shutdown-dialog-typed"),
      SHUTDOWN_PHRASE,
    );
    expect(confirm).toHaveAttribute("aria-disabled", "false");
    await user.click(confirm);
    expect(shutdown).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(
        screen.getByTestId("settings-shutdown-accepted"),
      ).toHaveTextContent("Shutdown accepted"),
    );
    expect(screen.queryByTestId("settings-shutdown-dialog")).toBeNull();
  });

  it("keeps rotation inert until a bearer is pasted and shows the new generation on the page", async () => {
    const user = userEvent.setup();
    rotateToken.mockResolvedValue({ ok: true, data: { generation: 3 } });
    render(<DangerZoneSection />);
    await user.click(screen.getByTestId("settings-rotate-open"));

    const confirm = screen.getByTestId("settings-rotate-dialog-confirm");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    await user.click(confirm);
    expect(rotateToken).not.toHaveBeenCalled();

    const field = screen.getByTestId("settings-rotate-token");
    expect(field).toHaveAttribute("type", "password");
    await user.type(field, "  nimbus_at_example  ");
    expect(confirm).toHaveAttribute("aria-disabled", "false");
    await user.click(confirm);
    expect(rotateToken).toHaveBeenCalledWith("nimbus_at_example");
    await waitFor(() =>
      expect(screen.getByTestId("settings-rotate-result")).toHaveTextContent(
        "generation 3",
      ),
    );
    expect(screen.queryByTestId("settings-rotate-dialog")).toBeNull();
    expect(toast.success).toHaveBeenCalled();
  });

  it("keeps a refused rotation open with the server's reason in the error strip", async () => {
    const user = userEvent.setup();
    rotateToken.mockResolvedValue({ ok: false, error: "unauthorized" });
    render(<DangerZoneSection />);
    await user.click(screen.getByTestId("settings-rotate-open"));
    await user.type(screen.getByTestId("settings-rotate-token"), "stale");
    await user.click(screen.getByTestId("settings-rotate-dialog-confirm"));

    const dialog = await screen.findByTestId("settings-rotate-dialog");
    expect(
      dialog.querySelector("[data-slot='dialog-error']"),
    ).toHaveTextContent("unauthorized");
    expect(screen.queryByTestId("settings-rotate-result")).toBeNull();
  });
});
