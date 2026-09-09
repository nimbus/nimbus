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

import { ServerInfoSection, TenantHeaderStrip } from "./-server-info";

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
    expect(unavailable.className).toContain("text-error");
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
    expect(updates.className).toContain("text-text-1");
    expect(updates).toHaveTextContent("up to date");
  });
});

describe("ServerInfoSection data directory", () => {
  it("shows the directory the engine opened as a copyable path", () => {
    render(
      <ServerInfoSection
        status={{
          details: {
            listenAddress: "127.0.0.1:3210",
            dataDir: "/srv/nimbus/data",
          },
        }}
        encryption={{ kind: "ok", value: { enabled: false } }}
      />,
    );
    expect(screen.getByTestId("settings-server-data-dir")).toHaveTextContent(
      "/srv/nimbus/data",
    );
    expect(screen.queryByTestId("settings-server-data-dir-missing")).toBeNull();
  });

  it("says the directory is not reported when the status row lacks it", () => {
    render(
      <ServerInfoSection
        status={status}
        encryption={{ kind: "ok", value: { enabled: false } }}
      />,
    );
    expect(
      screen.getByTestId("settings-server-data-dir-missing"),
    ).toHaveTextContent("not reported");
  });
});

describe("TenantHeaderStrip", () => {
  // The General page column is a flex column. The strip clips its overflow
  // for the rounded corners, and a clipping flex child may shrink below its
  // content: the first proof screenshot showed the values cut off.
  it("declines to shrink inside the page column", () => {
    render(<TenantHeaderStrip status={status} license={{ kind: "loading" }} />);
    expect(screen.getByTestId("settings-tenant-header").className).toContain(
      "shrink-0",
    );
  });

  it("repeats the license status only when it adds to the kind", () => {
    const { unmount } = render(
      <TenantHeaderStrip
        status={status}
        license={{
          kind: "ok",
          value: { kind: "community", status: "community" },
        }}
      />,
    );
    expect(screen.getByTestId("settings-license-kind")).toHaveTextContent(
      /^community$/,
    );
    unmount();
    render(
      <TenantHeaderStrip
        status={status}
        license={{ kind: "ok", value: { kind: "commercial", status: "trial" } }}
      />,
    );
    // The separator sits in its own span with a margin, so the text nodes
    // touch; the assertion reads the words, not the gap.
    expect(screen.getByTestId("settings-license-kind")).toHaveTextContent(
      /commercial\s*· trial/,
    );
  });
});
