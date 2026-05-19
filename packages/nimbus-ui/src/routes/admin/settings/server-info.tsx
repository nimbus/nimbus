import { CopyChip } from "../../../components/copy-chip";
import { StateChip } from "../../../components/state-chip";
import { StateDot } from "../../../components/state-dot";
import { RelativeTime, Uptime } from "../../../components/time";
import { UpgradePopover } from "../../../components/upgrade-popover";
import { useStalenessContext } from "../../../hooks/use-staleness";
import { Cell, Definition, DefinitionList, SectionCard } from "./primitives";
import type {
  AsyncSnapshot,
  EncryptionStatus,
  LicenseSnapshot,
  SystemStatusDoc,
} from "./types";

export function TenantHeaderStrip({
  status,
  license,
}: {
  status: SystemStatusDoc | undefined;
  license: AsyncSnapshot<LicenseSnapshot>;
}) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const licenseLabel =
    license === "loading"
      ? "loading…"
      : license === "error"
        ? "unavailable"
        : (license.kind ?? "developer");
  const licenseStatus =
    license === "loading" || license === "error"
      ? null
      : (license.status ?? null);
  const usageNow =
    license !== "loading" && license !== "error"
      ? (license.usage?.monthly_active_users ?? null)
      : null;
  const usageLimit =
    license !== "loading" && license !== "error"
      ? (license.monthly_active_user_limit ?? license.usage?.limit ?? null)
      : null;
  const usageLabel =
    usageNow === null
      ? "—"
      : usageLimit
        ? `${usageNow} / ${usageLimit} MAU`
        : `${usageNow} MAU`;
  return (
    <div
      data-testid="settings-tenant-header"
      className="grid grid-cols-2 gap-px overflow-hidden rounded-md border border-app bg-surface-2 md:grid-cols-4"
    >
      <Cell label="Active tenant">
        <CopyChip
          label="active tenant"
          value="_nimbus"
          testid="settings-tenant"
        />
      </Cell>
      <Cell label="Storage backend">
        <span className="font-mono text-xs text-default">{storageBackend}</span>
      </Cell>
      <Cell label="License">
        <span
          className="font-mono text-xs text-default"
          data-testid="settings-license-kind"
        >
          {licenseLabel}
          {licenseStatus ? (
            <span className="ml-1 text-muted">· {licenseStatus}</span>
          ) : null}
        </span>
      </Cell>
      <Cell label="Usage">
        <span
          className="font-mono text-xs text-default tabular"
          data-testid="settings-usage"
        >
          {usageLabel}
        </span>
      </Cell>
    </div>
  );
}

export function ServerInfoSection({
  status,
  encryption,
}: {
  status: SystemStatusDoc | undefined;
  encryption: AsyncSnapshot<EncryptionStatus>;
}) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const listenAddress =
    typeof details.listenAddress === "string"
      ? details.listenAddress
      : typeof details.address === "string"
        ? details.address
        : "—";
  const activeOrigin =
    typeof details.activeOrigin === "string"
      ? details.activeOrigin
      : typeof window !== "undefined"
        ? window.location.origin
        : "—";
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const encryptionEnabled =
    encryption === "loading" || encryption === "error"
      ? encryption
      : (encryption.enabled ?? false);
  const encryptedFamilies =
    encryption === "loading" || encryption === "error"
      ? []
      : (encryption.encrypted_families ?? []);
  return (
    <SectionCard
      title="Server"
      testid="settings-server-info"
      description="Version, uptime, listen address, storage backend, encryption, and health."
    >
      <DefinitionList>
        <Definition label="Health">
          <StateChip state={status?.health ?? "unknown"} />
        </Definition>
        <Definition label="Version">
          <CopyChip
            label="version"
            value={status?.version ?? "—"}
            testid="settings-server-version"
          />
        </Definition>
        <Definition label="Uptime">
          {typeof status?.startedAt === "number" ? (
            <Uptime startedAtMs={status.startedAt} />
          ) : (
            <span className="tabular text-muted">—</span>
          )}
        </Definition>
        <Definition label="Started">
          {typeof status?.startedAt === "number" ? (
            <RelativeTime epochMs={status.startedAt} />
          ) : (
            <span className="tabular text-muted">—</span>
          )}
        </Definition>
        <Definition label="Listen address">
          <CopyChip
            label="listen address"
            value={listenAddress}
            testid="settings-server-listen"
          />
        </Definition>
        <Definition label="Active origin">
          <CopyChip
            label="active origin"
            value={activeOrigin}
            testid="settings-server-origin"
          />
        </Definition>
        <Definition label="Storage backend">
          <span className="font-mono text-xs text-default">
            {storageBackend}
          </span>
        </Definition>
        <Definition label="Encryption">
          {encryptionEnabled === "loading" ? (
            <span className="inline-flex items-center gap-1.5 text-muted">
              <StateDot state="reconnecting" />
              loading…
            </span>
          ) : encryptionEnabled === "error" ? (
            <span
              className="inline-flex items-center gap-1.5 text-danger"
              data-testid="settings-encryption-unavailable"
            >
              <StateDot state="offline" />
              unavailable
            </span>
          ) : encryptionEnabled ? (
            <span
              className="inline-flex items-center gap-1.5 font-mono text-xs text-default"
              data-testid="settings-encryption-enabled"
            >
              <StateDot state="connected" />
              on
              {encryptedFamilies.length > 0 ? (
                <span className="ml-1 text-muted">
                  · {encryptedFamilies.join(", ")}
                </span>
              ) : null}
            </span>
          ) : (
            <span
              className="inline-flex items-center gap-1.5 font-mono text-xs text-muted"
              data-testid="settings-encryption-off"
            >
              <StateDot state="offline" />
              off
            </span>
          )}
        </Definition>
        <Definition label="Updates">
          <UpdatesValue />
        </Definition>
      </DefinitionList>
    </SectionCard>
  );
}

function UpdatesValue() {
  const staleness = useStalenessContext();
  const { snapshot, openPopover, closePopover, startUpgrade, copyCommand } =
    staleness;
  const { state, info } = snapshot;

  if (!info) {
    return <span className="text-muted">loading…</span>;
  }

  if (state === "upgrading") {
    return (
      <span
        data-testid="settings-updates-upgrading"
        className="font-mono text-xs text-default"
      >
        Updating to {info.latest}…
      </span>
    );
  }

  if (state === "upgraded") {
    return (
      <StateChip state="ok" className="data-[state=ok]:text-success" showDot />
    );
  }

  if (state === "hidden" || !info.available || !info.latest) {
    return <StateChip state="ok" showDot />;
  }

  const open = state === "confirming";
  return (
    <span
      data-testid="settings-updates-available"
      className="inline-flex items-center gap-2"
    >
      <UpgradePopover
        open={open}
        onOpenChange={(next) => {
          if (next) openPopover();
          else closePopover();
        }}
        info={info}
        isLocal={staleness.isLocal}
        hasDesktopBridge={staleness.hasDesktopBridge}
        onUpdate={startUpgrade}
        onCopyCommand={copyCommand}
        trigger={
          <span className="inline-flex items-center gap-1.5 font-mono text-xs text-default">
            <span
              aria-hidden
              className="inline-block size-2 rounded-full"
              style={{ background: "var(--color-brand)" }}
            />
            {info.latest} available — Update
          </span>
        }
      />
    </span>
  );
}
