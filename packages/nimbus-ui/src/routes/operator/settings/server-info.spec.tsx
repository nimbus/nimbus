import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../hooks/use-staleness", () => ({
  useStalenessContext: () => ({
    snapshot: { state: "hidden", info: { available: false, latest: null } },
    openPopover: vi.fn(),
    closePopover: vi.fn(),
    startUpgrade: vi.fn(),
    copyCommand: vi.fn(),
  }),
}));

import { ServerInfoSection } from "./server-info";

const status = { details: { listenAddress: "127.0.0.1:3210" } };

describe("ServerInfoSection state vocabulary", () => {
  it("renders encryption as a plain flag with categorical families, not a state dot", () => {
    render(
      <ServerInfoSection
        status={status}
        encryption={{
          kind: "ok",
          value: { enabled: true, encrypted_families: ["documents", "blobs"] },
        }}
      />,
    );

    const row = screen.getByTestId("settings-encryption-enabled");
    // Encryption at rest is a configuration flag. Spending the connection
    // vocabulary on a boolean setting drains the meaning out of the dots that
    // do report a lifecycle, so this row carries none.
    expect(row.querySelector("[data-state]")).toBeNull();
    expect(row).toHaveTextContent("on");
    expect(
      within(row)
        .getAllByText(/documents|blobs/)
        .map((chip) => chip.getAttribute("data-category")),
    ).toEqual(["documents", "blobs"]);
  });

  it("renders the off, loading, and unavailable flags without state dots", () => {
    const { rerender } = render(
      <ServerInfoSection
        status={status}
        encryption={{ kind: "ok", value: { enabled: false } }}
      />,
    );
    const off = screen.getByTestId("settings-encryption-off");
    expect(off.querySelector("[data-state]")).toBeNull();
    expect(off).toHaveTextContent("off");

    rerender(
      <ServerInfoSection status={status} encryption={{ kind: "loading" }} />,
    );
    expect(
      screen.getByText("loading…").querySelector("[data-state]"),
    ).toBeNull();

    rerender(
      <ServerInfoSection
        status={status}
        encryption={{ kind: "error", message: "nope" }}
      />,
    );
    const unavailable = screen.getByTestId("settings-encryption-unavailable");
    expect(unavailable.querySelector("[data-state]")).toBeNull();
    expect(unavailable.className).toContain("text-danger");
  });

  it("renders version freshness as plain text, not a tinted chip or a state dot", () => {
    render(
      <ServerInfoSection
        status={status}
        encryption={{ kind: "ok", value: { enabled: false } }}
      />,
    );

    const updates = screen.getByTestId("settings-updates-current");
    expect(updates.querySelector("[data-state]")).toBeNull();
    expect(updates.className).toContain("text-default");
    expect(updates).toHaveTextContent("up to date");
  });
});
