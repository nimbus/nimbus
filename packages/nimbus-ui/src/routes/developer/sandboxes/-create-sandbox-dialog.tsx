import { useState } from "react";
import { toast } from "sonner";

import { ConfirmDialog } from "../../../components/confirm-dialog";
import { Select } from "../../../components/select";
import { sandboxes as sandboxApi } from "../../../lib/api-mutations";
import {
  SANDBOX_BACKENDS,
  SANDBOX_PROFILES,
  type SandboxBackend,
  type SandboxCreateRequest,
  type SandboxProfile,
  type SandboxResource,
} from "../../../lib/types/sandbox";

const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}$/;

export type SandboxDraft = {
  profile: SandboxProfile;
  backend: SandboxBackend;
  id: string;
  displayName: string;
  image: string;
  // One argument per line; the first is the program.
  command: string;
};

export const DEFAULT_DRAFT: SandboxDraft = {
  profile: "worker",
  backend: "krun",
  id: "",
  displayName: "",
  image: "",
  command: "",
};

// Turn the form into the create request, or name the field at fault. The
// request shape is `SandboxCreateRequest` (camelCase) with the snake_case
// root discriminators the spec routes use.
export function draftToRequest(
  draft: SandboxDraft,
): { ok: true; request: SandboxCreateRequest } | { ok: false; error: string } {
  const id = draft.id.trim();
  if (!ID_PATTERN.test(id)) {
    return {
      ok: false,
      error:
        "The id needs 1 to 63 lowercase letters, digits, or hyphens, and starts with a letter or digit.",
    };
  }
  const image = draft.image.trim();
  if (image.length === 0) {
    return {
      ok: false,
      error:
        "Name the OCI image to run, for example docker.io/library/alpine:3.20.",
    };
  }
  const argv = draft.command
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (argv.length === 0) {
    return {
      ok: false,
      error: "Give the process to run, one argument per line.",
    };
  }
  const displayName = draft.displayName.trim();
  return {
    ok: true,
    request: {
      id,
      profile: draft.profile,
      spec: {
        owner: {
          kind: "standalone",
          ...(displayName ? { displayName } : {}),
        },
        backend: draft.backend,
        root: {
          kind: "oci_image",
          source: { kind: "reference", reference: image },
        },
        process: { argv },
      },
    },
  };
}

// The create form: a profile picker, a backend, the id and display name,
// the image, and the command. Create posts the request and hands the new
// resource back; the dialog stays open with the server's refusal when it
// says no.
export function CreateSandboxDialog({
  open,
  tenant,
  onCreated,
  onCancel,
}: {
  open: boolean;
  tenant: string;
  onCreated: (sandbox: SandboxResource) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState<SandboxDraft>(DEFAULT_DRAFT);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);

  const patch = (change: Partial<SandboxDraft>) =>
    setDraft((prev) => ({ ...prev, ...change }));

  const close = () => {
    setDraft(DEFAULT_DRAFT);
    setError(undefined);
    onCancel();
  };

  const create = async () => {
    const compiled = draftToRequest(draft);
    if (!compiled.ok) {
      setError(compiled.error);
      return;
    }
    setBusy(true);
    setError(undefined);
    const result = await sandboxApi.create(tenant, compiled.request);
    setBusy(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    toast.success(`Sandbox ${compiled.request.id} created`);
    setDraft(DEFAULT_DRAFT);
    onCreated(result.data);
  };

  return (
    <ConfirmDialog
      open={open}
      title="New sandbox"
      description="A standalone sandbox in this tenant: an OCI image and a process to run in it. The runtime pulls the image and reports the sandbox as ready when the process is up."
      confirmLabel="Create sandbox"
      busy={busy}
      error={error}
      onConfirm={() => void create()}
      onCancel={close}
      testid="sandbox-create-dialog"
    >
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <Select
            label="profile"
            value={draft.profile}
            options={SANDBOX_PROFILES.map((profile) => ({
              value: profile,
              label: PROFILE_LABELS[profile],
            }))}
            onChange={(profile) => patch({ profile })}
            testid="sandbox-create-profile"
          />
          <Select
            label="backend"
            value={draft.backend}
            options={SANDBOX_BACKENDS.map((backend) => ({
              value: backend,
              label: BACKEND_LABELS[backend],
            }))}
            onChange={(backend) => patch({ backend })}
            testid="sandbox-create-backend"
          />
        </div>
        <Field
          htmlFor="sandbox-create-id"
          label="Id"
          hint="lowercase, digits, hyphens"
        >
          <input
            value={draft.id}
            onChange={(event) => patch({ id: event.target.value })}
            placeholder="scratch-1"
            className={INPUT_CLASS}
            id="sandbox-create-id"
            data-testid="sandbox-create-id"
          />
        </Field>
        <Field
          htmlFor="sandbox-create-name"
          label="Display name"
          hint="optional"
        >
          <input
            value={draft.displayName}
            onChange={(event) => patch({ displayName: event.target.value })}
            placeholder="scratch shell"
            className={INPUT_CLASS}
            id="sandbox-create-name"
            data-testid="sandbox-create-name"
          />
        </Field>
        <Field
          htmlFor="sandbox-create-image"
          label="Image"
          hint="OCI reference"
        >
          <input
            value={draft.image}
            onChange={(event) => patch({ image: event.target.value })}
            placeholder="docker.io/library/alpine:3.20"
            className={INPUT_CLASS}
            id="sandbox-create-image"
            data-testid="sandbox-create-image"
          />
        </Field>
        <Field
          htmlFor="sandbox-create-command"
          label="Command"
          hint="one argument per line"
        >
          <textarea
            value={draft.command}
            onChange={(event) => patch({ command: event.target.value })}
            placeholder={"/bin/sh\n-c\nwhile true; do date; sleep 5; done"}
            rows={3}
            className={`${INPUT_CLASS} h-auto py-1`}
            id="sandbox-create-command"
            data-testid="sandbox-create-command"
          />
        </Field>
      </div>
    </ConfirmDialog>
  );
}

function Field({
  htmlFor,
  label,
  hint,
  children,
}: {
  htmlFor: string;
  label: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label
        htmlFor={htmlFor}
        className="flex items-baseline gap-2 text-xs font-medium text-text-3"
      >
        {label}
        <span className="font-normal">{hint}</span>
      </label>
      {children}
    </div>
  );
}

const PROFILE_LABELS: Record<SandboxProfile, string> = {
  worker: "worker (headless)",
  desktop: "desktop (with a display)",
};

const BACKEND_LABELS: Record<SandboxBackend, string> = {
  krun: "krun (microVM)",
  container: "container",
};

const INPUT_CLASS =
  "h-[26px] w-full rounded-xs border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1 placeholder:text-text-3 focus-visible:border-accent";
