import { CategoryChip } from "../../../components/category-chip";
import { CopyChip } from "../../../components/copy-chip";
import { StateChip } from "../../../components/state-chip";
import { RelativeTime, Uptime } from "../../../components/time";
import { UpgradePopover } from "../../../components/upgrade-popover";
import { useStalenessContext } from "../../../hooks/use-staleness";
import type { LoadingValue } from "../../../shell/loading-value";
import { Cell, Definition, DefinitionList, PageSection } from "./primitives";
import type {
  EncryptionStatus,
  LicenseSnapshot,
  SystemStatusDoc,
} from "./types";

export function TenantHeaderStrip({
  status,
  license,
}: {
  status: SystemStatusDoc | undefined;
  license: LoadingValue<LicenseSnapshot>;
}) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const licenseSnap = license.kind === "ok" ? license.value : null;
  const licenseLabel =
    license.kind === "loading"
      ? "loading…"
      : licenseSnap === null
        ? "unavailable"
        : (licenseSnap.kind ?? "developer");
  const licenseStatus = licenseSnap?.status ?? null;
  const usageNow = licenseSnap?.usage?.monthly_active_users ?? null;
  const usageLimit = licenseSnap
    ? (licenseSnap.monthly_active_user_limit ??
      licenseSnap.usage?.limit ??
      null)
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
  encryption: LoadingValue<EncryptionStatus>;
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
  const encryptionSnap = encryption.kind === "ok" ? encryption.value : null;
  const encryptionEnabled: "loading" | "error" | boolean =
    encryption.kind === "loading"
      ? "loading"
      : encryptionSnap === null
        ? "error"
        : (encryptionSnap.enabled ?? false);
  const encryptedFamilies = encryptionSnap?.encrypted_families ?? [];
  return (
    <PageSection
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
        {/*
          Encryption at rest is a configuration flag, not a lifecycle. A state
          dot asserts "this thing is in this state right now", so spending the
          connected/offline vocabulary on a boolean setting drains the meaning
          out of the dots that do report health. The value reads as plain
          on/off, and the families it covers are categories, so they take the
          categorical pill.
        */}
        <Definition label="Encryption">
          {encryptionEnabled === "loading" ? (
            <span className="font-mono text-xs text-muted">loading…</span>
          ) : encryptionEnabled === "error" ? (
            <span
              className="font-mono text-xs text-danger"
              data-testid="settings-encryption-unavailable"
            >
              unavailable
            </span>
          ) : encryptionEnabled ? (
            <span
              className="inline-flex flex-wrap items-center gap-1.5 font-mono text-xs text-default"
              data-testid="settings-encryption-enabled"
            >
              on
              {encryptedFamilies.map((family) => (
                <CategoryChip key={family} value={family} />
              ))}
            </span>
          ) : (
            <span
              className="font-mono text-xs text-muted"
              data-testid="settings-encryption-off"
            >
              off
            </span>
          )}
        </Definition>
        <Definition label="Updates">
          <UpdatesValue />
        </Definition>
      </DefinitionList>
    </PageSection>
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

  // Version freshness is not a lifecycle state either, so it takes neither
  // StateChip's closed vocabulary nor a state dot. It reads as the plain
  // sentence it is, instead of being the one chip in the console whose label
  // is tinted.
  if (state === "upgraded") {
    return (
      <span
        data-testid="settings-updates-upgraded"
        className="font-mono text-xs text-default"
      >
        Updated to {info.latest}
      </span>
    );
  }

  if (state === "hidden" || !info.available || !info.latest) {
    return (
      <span
        data-testid="settings-updates-current"
        className="font-mono text-xs text-default"
      >
        up to date
      </span>
    );
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
              style={{ background: "var(--nimbus-brand)" }}
            />
            {info.latest} available — Update
          </span>
        }
      />
    </span>
  );
}
