import { Download, Link2, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { CopyChip } from "../../../components/copy-chip";
import { RelativeTime } from "../../../components/time";
import { Button } from "../../../components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "../../../components/ui/sheet";
import {
  type ObjectSummary,
  objects as objectApi,
} from "../../../lib/api-mutations";
import { formatAbsoluteTime, formatBytes } from "../../../lib/format";
import { previewKind, shortContentType } from "./-types";

const TESTID = "files-sheet";

// ObjectSheet is the right-side detail for one object: the facts the row
// cannot fit, a preview when the bytes are an image or text, the download,
// the link, and the delete. The link is the console's own object route, so
// it needs a console session; the sheet says so next to the copy control.
export function ObjectSheet({
  tenant,
  bucket,
  objectKey,
  objects,
  loading,
  onClose,
  onDelete,
}: {
  tenant: string;
  bucket: string;
  objectKey: string | undefined;
  objects: readonly ObjectSummary[] | undefined;
  loading: boolean;
  onClose: () => void;
  onDelete: (object: ObjectSummary) => void;
}) {
  const object = objectKey
    ? objects?.find((o) => o.key === objectKey)
    : undefined;
  return (
    <Sheet
      open={objectKey !== undefined}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <SheetContent
        className="w-full sm:max-w-md"
        data-testid={TESTID}
        aria-label={object ? `Object ${object.key}` : "Object"}
      >
        {object ? (
          <ObjectBody
            tenant={tenant}
            bucket={bucket}
            object={object}
            onDelete={onDelete}
          />
        ) : objectKey ? (
          <Missing loading={loading} />
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function ObjectBody({
  tenant,
  bucket,
  object,
  onDelete,
}: {
  tenant: string;
  bucket: string;
  object: ObjectSummary;
  onDelete: (object: ObjectSummary) => void;
}) {
  const name = object.key.split("/").pop() || object.key;
  const path = objectApi.url(tenant, bucket, object.key);
  const link =
    typeof window !== "undefined" && window.location
      ? `${window.location.origin}${path}`
      : path;
  const copyLink = async () => {
    try {
      await navigator.clipboard.writeText(link);
      toast("Copied link", { description: link });
    } catch {
      toast.error("Failed to copy link");
    }
  };
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle
          className="min-w-0 truncate font-mono text-sm text-text-1"
          title={object.key}
        >
          {name}
        </SheetTitle>
        <SheetDescription className="flex min-w-0 items-center gap-2">
          <span className="truncate font-mono text-xs text-text-3">
            {bucket}/{object.key}
          </span>
        </SheetDescription>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto px-4">
        <dl className="grid grid-cols-[6rem_1fr] gap-x-3 gap-y-1.5 text-xs">
          <Fact label="Size">
            <span data-testid={`${TESTID}-size`}>
              {formatBytes(object.size)}
            </span>
          </Fact>
          <Fact label="Type">
            <span data-testid={`${TESTID}-type`}>
              {shortContentType(object.contentType)}
            </span>
          </Fact>
          <Fact label="Modified">
            <span title={formatAbsoluteTime(object.lastModifiedMillis)}>
              <RelativeTime epochMs={object.lastModifiedMillis} />
            </span>
          </Fact>
          <Fact label="ETag">
            <CopyChip
              label="etag"
              value={object.etag}
              testid={`${TESTID}-etag`}
            >
              {object.etag}
            </CopyChip>
          </Fact>
        </dl>
        <Preview tenant={tenant} bucket={bucket} object={object} />
        <p className="text-xs text-text-3">
          The link is this console&apos;s object route. It answers for a signed
          in console session only; hand out S3 credentials for anything else.
        </p>
      </div>
      <SheetFooter className="flex-row flex-wrap justify-end gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onDelete(object)}
          data-testid={`${TESTID}-delete`}
        >
          <Trash2 data-icon="inline-start" />
          Delete
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void copyLink()}
          data-testid={`${TESTID}-copy-link`}
        >
          <Link2 data-icon="inline-start" />
          Copy link
        </Button>
        <Button
          size="sm"
          render={
            // biome-ignore lint/a11y/useAnchorContent: the button renders the anchor's content
            <a
              href={objectApi.url(tenant, bucket, object.key, {
                download: true,
              })}
              download={name}
            />
          }
          data-testid={`${TESTID}-download`}
        >
          <Download data-icon="inline-start" />
          Download
        </Button>
      </SheetFooter>
    </>
  );
}

// The preview reads the object once per key. A text object arrives through
// the same route the download uses; an image is the browser's own render of
// it, which the route sandboxes so a stored page cannot script the console.
function Preview({
  tenant,
  bucket,
  object,
}: {
  tenant: string;
  bucket: string;
  object: ObjectSummary;
}) {
  const kind = previewKind(object.contentType, object.key, object.size);
  const [text, setText] = useState<
    { key: string; body: string } | { key: string; error: string } | undefined
  >(undefined);

  useEffect(() => {
    if (kind !== "text") return;
    let live = true;
    void objectApi.readText(tenant, bucket, object.key).then((result) => {
      if (!live) return;
      setText(
        result.ok
          ? { key: object.key, body: result.data }
          : { key: object.key, error: result.error },
      );
    });
    return () => {
      live = false;
    };
  }, [kind, tenant, bucket, object.key]);

  if (kind === "image") {
    return (
      <section
        className="flex flex-col gap-1.5"
        data-testid={`${TESTID}-preview`}
      >
        <h3 className="text-xs font-medium text-text-3">Preview</h3>
        <div className="flex items-center justify-center overflow-hidden rounded-md border border-border-2 bg-bg-raised p-2">
          <img
            src={objectApi.url(tenant, bucket, object.key)}
            alt={object.key}
            className="max-h-64 max-w-full object-contain"
            data-testid={`${TESTID}-preview-image`}
          />
        </div>
      </section>
    );
  }
  if (kind === "text") {
    const current = text?.key === object.key ? text : undefined;
    return (
      <section
        className="flex flex-col gap-1.5"
        data-testid={`${TESTID}-preview`}
      >
        <h3 className="text-xs font-medium text-text-3">Preview</h3>
        {current === undefined ? (
          <p className="text-xs text-text-3">Reading…</p>
        ) : "error" in current ? (
          <p className="text-xs text-error">{current.error}</p>
        ) : (
          <pre
            className="m-0 max-h-64 overflow-auto rounded-md border border-border-2 bg-bg-raised p-3 font-mono text-xs whitespace-pre-wrap text-text-1"
            data-testid={`${TESTID}-preview-text`}
          >
            {current.body}
          </pre>
        )}
      </section>
    );
  }
  return (
    <p className="text-xs text-text-3" data-testid={`${TESTID}-no-preview`}>
      No inline preview for this type. Download it to open it.
    </p>
  );
}

function Missing({ loading }: { loading: boolean }) {
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle>{loading ? "Reading…" : "Not in this listing"}</SheetTitle>
        <SheetDescription>
          {loading
            ? "The object list is still loading."
            : "The key the address names is not in this listing. It may have been deleted, or it sits under another prefix."}
        </SheetDescription>
      </SheetHeader>
      <div className="flex-1" data-testid={`${TESTID}-missing`} />
    </>
  );
}

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt className="font-medium text-text-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate font-mono text-text-1">{children}</dd>
    </>
  );
}
