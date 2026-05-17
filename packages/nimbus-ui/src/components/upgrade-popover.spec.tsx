import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { VersionInfo } from "../api/system";
import { UpgradePopover } from "./upgrade-popover";

function makeInfo(over: Partial<VersionInfo> = {}): VersionInfo {
  return {
    current: "0.1.40",
    latest: "0.1.41",
    available: true,
    url: "https://example.com/release",
    publishedAt: "2026-05-15T00:00:00Z",
    host: "localhost",
    checkStatus: "fresh",
    upgrade: {
      method: "brew",
      command: "brew upgrade nimbus/tap/nimbus",
      needsSudo: false,
      interactive: false,
      fallbackUrl: "https://example.com/install",
    },
    ...over,
  };
}

function renderPopover(
  opts: {
    info?: VersionInfo;
    isLocal?: boolean;
    hasDesktopBridge?: boolean;
    onUpdate?: () => void;
    onCopyCommand?: () => void;
  } = {},
) {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    info: opts.info ?? makeInfo(),
    isLocal: opts.isLocal ?? true,
    hasDesktopBridge: opts.hasDesktopBridge ?? true,
    onUpdate: opts.onUpdate ?? vi.fn(),
    onCopyCommand: opts.onCopyCommand ?? vi.fn(),
  };
  return {
    ...render(
      <UpgradePopover {...props} trigger={<span>version trigger</span>} />,
    ),
    props,
  };
}

describe("UpgradePopover", () => {
  it("renders the Update action when local + desktop bridge is present", () => {
    renderPopover({ isLocal: true, hasDesktopBridge: true });
    const popup = screen.getByTestId("upgrade-popover");
    expect(within(popup).getByTestId("upgrade-popover-update")).toBeTruthy();
    expect(within(popup).queryByTestId("upgrade-popover-copy")).toBeNull();
    expect(within(popup).getByText(/Update Nimbus to 0\.1\.41\?/)).toBeTruthy();
  });

  it("renders Copy command when local but no bridge is present", () => {
    renderPopover({ isLocal: true, hasDesktopBridge: false });
    const popup = screen.getByTestId("upgrade-popover");
    expect(within(popup).getByTestId("upgrade-popover-copy")).toBeTruthy();
    expect(within(popup).queryByTestId("upgrade-popover-update")).toBeNull();
  });

  it("forces Copy command in the remote-host branch even with bridge present", () => {
    renderPopover({
      isLocal: false,
      hasDesktopBridge: true,
      info: makeInfo({ host: "server.example.com" }),
    });
    const popup = screen.getByTestId("upgrade-popover");
    expect(within(popup).getByTestId("upgrade-popover-copy")).toBeTruthy();
    expect(within(popup).queryByTestId("upgrade-popover-update")).toBeNull();
    expect(
      within(popup).getByText(/Copy command to run on server\.example\.com\?/),
    ).toBeTruthy();
  });

  it("renders the fallback link when upgrade.command is null", () => {
    renderPopover({
      info: makeInfo({
        upgrade: {
          method: "source",
          command: null,
          needsSudo: false,
          interactive: false,
          fallbackUrl: "https://docs.example.com/install",
        },
      }),
    });
    const popup = screen.getByTestId("upgrade-popover");
    expect(within(popup).queryByTestId("upgrade-popover-update")).toBeNull();
    expect(within(popup).queryByTestId("upgrade-popover-copy")).toBeNull();
    const link = within(popup).getByTestId("upgrade-popover-fallback-link");
    expect(link.getAttribute("href")).toBe("https://docs.example.com/install");
  });

  it("invokes onUpdate when the Update button is clicked", async () => {
    const onUpdate = vi.fn();
    renderPopover({ onUpdate });
    await userEvent.click(screen.getByTestId("upgrade-popover-update"));
    expect(onUpdate).toHaveBeenCalledTimes(1);
  });

  it("invokes onCopyCommand when the Copy command button is clicked", async () => {
    const onCopyCommand = vi.fn();
    renderPopover({ hasDesktopBridge: false, onCopyCommand });
    await userEvent.click(screen.getByTestId("upgrade-popover-copy"));
    expect(onCopyCommand).toHaveBeenCalledTimes(1);
  });
});
