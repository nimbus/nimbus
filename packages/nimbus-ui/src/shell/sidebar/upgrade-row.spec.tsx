import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { snapshotRef, openPopover, closePopover } = vi.hoisted(() => ({
  snapshotRef: {
    current: { state: "hidden", info: null, targetLatest: null } as {
      state: string;
      info: unknown;
      targetLatest: string | null;
    },
  },
  openPopover: vi.fn(),
  closePopover: vi.fn(),
}));

vi.mock("../../hooks/use-staleness", () => ({
  useStalenessContext: () => ({
    snapshot: snapshotRef.current,
    isLocal: false,
    hasDesktopBridge: false,
    openPopover,
    closePopover,
    startUpgrade: vi.fn(),
    copyCommand: vi.fn(),
  }),
}));

import type { VersionInfo } from "../../api/system";
import { statePalette } from "../../components/state-dot";
import {
  UPGRADE_TONE_KINDS,
  UpgradeRow,
  type UpgradeTone,
} from "./upgrade-row";

beforeEach(() => {
  snapshotRef.current = { state: "hidden", info: null, targetLatest: null };
  openPopover.mockClear();
  closePopover.mockClear();
});

// The available/confirming rows mount UpgradePopover, which reads the whole
// VersionInfo, so the fixture has to be complete rather than just `latest`.
const VERSION_INFO: VersionInfo = {
  current: "0.1.0",
  latest: "0.2.0",
  available: true,
  url: "https://example.invalid/releases/v0.2.0",
  publishedAt: "2026-08-01T00:00:00Z",
  host: "localhost",
  checkStatus: "fresh",
  upgrade: {
    method: "brew",
    command: "brew upgrade nimbus",
    needsSudo: false,
    interactive: false,
    fallbackUrl: "https://example.invalid/INSTALL.md",
  },
};

function showUpgrade(state: string, current = "0.1.0") {
  snapshotRef.current = {
    state,
    info: { ...VERSION_INFO, current },
    targetLatest: "0.2.0",
  };
}

const UPGRADE_STATES: Array<[UpgradeTone, string, string]> = [
  ["available", "available", "sidebar-upgrade-available"],
  ["upgrading", "upgrading", "sidebar-upgrade-upgrading"],
  ["upgraded", "upgraded", "sidebar-upgrade-upgraded"],
];

describe("UpgradeRow", () => {
  it("renders nothing while the console is current", () => {
    const { container } = render(<UpgradeRow collapsed={false} />);
    expect(container.innerHTML).toBe("");
  });

  it("offers the update as a popover trigger named for the version", () => {
    showUpgrade("available");
    render(<UpgradeRow collapsed={false} />);
    const trigger = screen.getByTestId("upgrade-popover-trigger");
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-label", "Update to 0.2.0");
    expect(trigger.textContent).toBe("Update to 0.2.0");
    expect(screen.queryByTestId("upgrade-popover")).toBeNull();
  });

  it("asks the staleness owner to open the popover instead of holding open state", () => {
    showUpgrade("available");
    render(<UpgradeRow collapsed={false} />);
    fireEvent.click(screen.getByTestId("upgrade-popover-trigger"));
    expect(openPopover).toHaveBeenCalledTimes(1);
  });

  it("shows the popover while the owner is confirming", () => {
    showUpgrade("confirming");
    render(<UpgradeRow collapsed={false} />);
    expect(screen.getByTestId("upgrade-popover")).toBeInTheDocument();
  });

  it("reports the running upgrade as a status, not a control", () => {
    showUpgrade("upgrading");
    render(<UpgradeRow collapsed={false} />);
    const row = screen.getByTestId("sidebar-upgrade-upgrading");
    expect(row).toHaveAttribute("role", "status");
    expect(row.textContent).toBe("Updating to 0.2.0…");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("names the version now running once the upgrade lands", () => {
    showUpgrade("upgraded", "0.2.0");
    render(<UpgradeRow collapsed={false} />);
    expect(screen.getByTestId("sidebar-upgrade-upgraded").textContent).toBe(
      "Updated to 0.2.0",
    );
  });

  it("keeps the trigger in the rail with its label on the button", () => {
    showUpgrade("available");
    render(<UpgradeRow collapsed={true} />);
    const trigger = screen.getByTestId("upgrade-popover-trigger");
    expect(trigger).toHaveAttribute("aria-label", "Update to 0.2.0");
    expect(trigger.textContent).toBe("");
  });

  describe("UpgradeDot", () => {
    it.each(
      UPGRADE_STATES,
    )("takes the %s colour from the shared state palette, not a private copy", (tone, snapshotState, testid) => {
      showUpgrade(snapshotState);
      render(<UpgradeRow collapsed={false} />);
      const slot = screen.getByTestId(testid);
      const dot = slot.querySelector("[data-state]") as HTMLElement;
      const kind = UPGRADE_TONE_KINDS[tone];
      expect(dot.dataset.state).toBe(kind);
      expect(dot.style.background).toBe(`var(${statePalette[kind].token})`);
    });

    it.each(
      UPGRADE_STATES,
    )("never animates the %s dot: the sidebar is always on screen", (_tone, snapshotState) => {
      showUpgrade(snapshotState);
      const { container } = render(<UpgradeRow collapsed={false} />);
      expect(container.innerHTML).not.toMatch(/animate-/);
    });

    // Drift lock: every tone must name a real entry in the shared table, so a
    // renamed or deleted StateKind fails here instead of silently degrading to
    // the unknown-state glyph the way a private table would.
    it("maps every tone to a live state-palette entry", () => {
      const tones = Object.keys(UPGRADE_TONE_KINDS) as UpgradeTone[];
      expect(tones).toHaveLength(3);
      for (const tone of tones) {
        const entry = statePalette[UPGRADE_TONE_KINDS[tone]];
        expect(entry).toBeDefined();
        expect(entry.glyph).not.toBe("question");
      }
    });
  });
});
