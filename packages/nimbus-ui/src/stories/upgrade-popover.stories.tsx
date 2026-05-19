import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import type { VersionInfo } from "../api/system";
import { UpgradePopover } from "../components/upgrade-popover";

const meta: Meta = {
  title: "Components/UpgradePopover",
};

export default meta;

type Story = StoryObj;

const HOMEBREW_INFO: VersionInfo = {
  current: "0.2.1",
  latest: "0.3.0",
  available: true,
  url: "https://github.com/nimbus/nimbus/releases/tag/v0.3.0",
  publishedAt: "2026-05-15T00:00:00Z",
  host: "localhost",
  checkStatus: "fresh",
  upgrade: {
    method: "brew",
    command: "brew upgrade nimbus",
    needsSudo: false,
    interactive: false,
    fallbackUrl: "https://github.com/nimbus/nimbus/blob/main/INSTALL.md",
  },
};

const REMOTE_INFO: VersionInfo = {
  ...HOMEBREW_INFO,
  host: "prod-east-1.internal",
};

const FALLBACK_INFO: VersionInfo = {
  ...HOMEBREW_INFO,
  upgrade: {
    ...HOMEBREW_INFO.upgrade,
    method: "unknown",
    command: null,
  },
};

function StoryFrame({
  info,
  isLocal,
  hasDesktopBridge,
}: {
  info: VersionInfo;
  isLocal: boolean;
  hasDesktopBridge: boolean;
}) {
  const [open, setOpen] = useState(true);
  return (
    <div className="flex items-center justify-center p-12">
      <UpgradePopover
        open={open}
        onOpenChange={setOpen}
        info={info}
        isLocal={isLocal}
        hasDesktopBridge={hasDesktopBridge}
        onUpdate={() => undefined}
        onCopyCommand={() => undefined}
        trigger={<span>{info.current} → {info.latest ?? "?"}</span>}
      />
    </div>
  );
}

export const LocalWithBridge: Story = {
  render: () => (
    <StoryFrame info={HOMEBREW_INFO} isLocal hasDesktopBridge />
  ),
};

export const LocalWithoutBridge: Story = {
  render: () => (
    <StoryFrame info={HOMEBREW_INFO} isLocal hasDesktopBridge={false} />
  ),
};

export const RemoteHost: Story = {
  render: () => (
    <StoryFrame
      info={REMOTE_INFO}
      isLocal={false}
      hasDesktopBridge={false}
    />
  ),
};

export const FallbackNoCommand: Story = {
  render: () => (
    <StoryFrame
      info={FALLBACK_INFO}
      isLocal
      hasDesktopBridge={false}
    />
  ),
};

export const ClosedTrigger: Story = {
  render: () => {
    function Closed() {
      const [open, setOpen] = useState(false);
      return (
        <div className="flex items-center justify-center p-12">
          <UpgradePopover
            open={open}
            onOpenChange={setOpen}
            info={HOMEBREW_INFO}
            isLocal
            hasDesktopBridge
            onUpdate={() => undefined}
            onCopyCommand={() => undefined}
            trigger={
              <span>
                {HOMEBREW_INFO.current} → {HOMEBREW_INFO.latest}
              </span>
            }
          />
        </div>
      );
    }
    return <Closed />;
  },
};
