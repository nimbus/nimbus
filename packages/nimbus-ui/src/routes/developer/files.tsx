import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { Ellipsis, File, Folder, FolderPlus, Upload } from "lucide-react";
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";

import {
  Breadcrumb,
  type BreadcrumbSegment,
} from "../../components/breadcrumb";
import { ConfirmDialog } from "../../components/confirm-dialog";
import {
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { DropZone } from "../../components/files/drop-zone";
import {
  type UploadItem,
  UploadQueue,
} from "../../components/files/upload-queue";
import { LoadFailed } from "../../components/load-failed";
import { uploadObjectCommand } from "../../components/onboarding/next-action";
import { PageHeader } from "../../components/page-header";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../components/storage/row-context-menu";
import { RelativeTime } from "../../components/time";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { useServerUrl } from "../../hooks/use-server-url";
import {
  type ObjectBucket,
  type ObjectSummary,
  objects as objectApi,
} from "../../lib/api-mutations";
import { formatBytes } from "../../lib/format";
import {
  type SubPanelSpec,
  useContributeSubPanel,
  useSubPanelSearch,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";
import { ObjectSheet } from "./files/-object-sheet";
import {
  deriveRows,
  type FileRow,
  type FilesSearch,
  isValidBucketName,
  parseFilesSearch,
  prefixSegments,
  shortContentType,
} from "./files/-types";
import {
  LIST_LIMIT,
  useBuckets,
  useObjectListing,
  useReloadVersion,
} from "./files/-use-objects";

export const Route = createFileRoute("/developer/files")({
  validateSearch: parseFilesSearch,
  component: FilesPage,
});

// A row menu anchored on one row of the table.
type MenuState = RowAnchor & { row: FileRow };

function FilesPage() {
  const search = useSearch({ from: "/developer/files" });
  const navigate = useNavigate();
  const tenant = useUiStore((s) => s.activeTenant);
  const serverUrl = useServerUrl();
  const bucket = search.bucket;
  const prefix = search.prefix ?? "";

  const [version, reload] = useReloadVersion();
  const buckets = useBuckets(tenant, version);
  const listing = useObjectListing(tenant, bucket, prefix, version);

  const setSearch = useCallback(
    (patch: Partial<FilesSearch>) =>
      navigate({
        to: "/developer/files",
        search: (prev) => ({ ...parseFilesSearch(prev), ...patch }),
        replace: true,
      }),
    [navigate],
  );

  // An address without a bucket lands on the first one the tenant holds,
  // so the page opens on a listing instead of a chooser.
  useEffect(() => {
    if (bucket !== undefined || buckets.data === undefined) return;
    const first = buckets.data[0];
    if (first) void setSearch({ bucket: first.bucket });
  }, [bucket, buckets.data, setSearch]);

  const rows = useMemo(
    () => (listing.data ? deriveRows(listing.data.objects, prefix) : []),
    [listing.data, prefix],
  );

  const spec = useMemo<SubPanelSpec>(() => {
    const shown = bucketsShown(buckets.data, bucket);
    return {
      kind: "dynamic",
      title: "Files",
      search: { placeholder: "Filter buckets", rows: shown.length },
      children: (
        <BucketsSubPanel
          buckets={shown}
          selected={bucket}
          loading={buckets.loading}
          tenant={tenant}
        />
      ),
    };
  }, [buckets.data, buckets.loading, bucket, tenant]);
  useContributeSubPanel(spec);

  // Uploads: one strip item per file, keyed by the object key it lands on.
  // A drop with the same name replaces the earlier receipt because the
  // object it wrote is the one being replaced.
  const [uploads, setUploads] = useState<UploadItem[]>([]);
  const fileInput = useRef<HTMLInputElement>(null);
  const startUploads = useCallback(
    (files: File[]) => {
      if (tenant === null || bucket === undefined) return;
      for (const file of files) {
        const key = `${prefix}${file.name}`;
        setUploads((prev) => [
          ...prev.filter((item) => item.id !== key),
          {
            id: key,
            name: file.name,
            loaded: 0,
            total: file.size,
            status: "uploading",
          },
        ]);
        const patch = (change: Partial<UploadItem>) =>
          setUploads((prev) =>
            prev.map((item) =>
              item.id === key ? { ...item, ...change } : item,
            ),
          );
        void objectApi
          .upload(tenant, bucket, key, file, {
            contentType: file.type || undefined,
            onProgress: (progress) =>
              patch({ loaded: progress.loaded, total: progress.total }),
          })
          .then((result) => {
            if (result.ok) {
              patch({ status: "done", loaded: file.size, total: file.size });
              toast.success(`Uploaded ${file.name}`);
              reload();
            } else {
              patch({ status: "error", error: result.error });
              toast.error(`Upload of ${file.name} failed`, {
                description: result.error,
              });
            }
          });
      }
    },
    [tenant, bucket, prefix, reload],
  );
  const openChooser = useCallback(() => fileInput.current?.click(), []);
  const dismissUpload = useCallback(
    (id: string) => setUploads((prev) => prev.filter((item) => item.id !== id)),
    [],
  );

  // Row actions.
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<ObjectSummary | null>(
    null,
  );
  const [deleteState, setDeleteState] = useState<{
    busy: boolean;
    error?: string;
  }>({ busy: false });
  const [newBucket, setNewBucket] = useState<{ open: boolean; name: string }>({
    open: false,
    name: "",
  });
  const newBucketId = useId();

  const openRow = useCallback(
    (row: FileRow) =>
      row.kind === "folder"
        ? setSearch({ prefix: row.prefix, object: undefined })
        : setSearch({ object: row.object.key }),
    [setSearch],
  );

  const copyLink = useCallback(
    async (object: ObjectSummary) => {
      if (tenant === null || bucket === undefined) return;
      const link = `${window.location.origin}${objectApi.url(tenant, bucket, object.key)}`;
      try {
        await navigator.clipboard.writeText(link);
        toast("Copied link", { description: link });
      } catch {
        toast.error("Failed to copy link");
      }
    },
    [tenant, bucket],
  );

  const download = useCallback(
    (object: ObjectSummary) => {
      if (tenant === null || bucket === undefined) return;
      const anchor = document.createElement("a");
      anchor.href = objectApi.url(tenant, bucket, object.key, {
        download: true,
      });
      anchor.download = object.key.split("/").pop() || object.key;
      anchor.rel = "noopener";
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
    },
    [tenant, bucket],
  );

  const menuItems = useCallback(
    (state: MenuState): RowMenuItem[] => {
      const row = state.row;
      if (row.kind === "folder") {
        return [
          {
            id: "open",
            label: "Open folder",
            onSelect: () => void openRow(row),
          },
        ];
      }
      const object = row.object;
      return [
        { id: "open", label: "Open", onSelect: () => void openRow(row) },
        { id: "download", label: "Download", onSelect: () => download(object) },
        {
          id: "copy-link",
          label: "Copy link",
          onSelect: () => void copyLink(object),
        },
        {
          id: "delete",
          label: "Delete",
          danger: true,
          onSelect: () => setConfirmDelete(object),
        },
      ];
    },
    [openRow, download, copyLink],
  );

  const deleteConfirmed = useCallback(async () => {
    if (confirmDelete === null || tenant === null || bucket === undefined) {
      return;
    }
    const object = confirmDelete;
    setDeleteState({ busy: true });
    const result = await objectApi.remove(tenant, bucket, object.key);
    if (result.ok) {
      const name = object.key.split("/").pop() || object.key;
      setDeleteState({ busy: false });
      setConfirmDelete(null);
      toast.success(`Deleted ${name}`);
      if (search.object === object.key) void setSearch({ object: undefined });
      reload();
    } else {
      setDeleteState({ busy: false, error: result.error });
    }
  }, [confirmDelete, tenant, bucket, search.object, setSearch, reload]);

  const createBucket = useCallback(() => {
    const name = newBucket.name.trim();
    if (!isValidBucketName(name)) return;
    setNewBucket({ open: false, name: "" });
    void setSearch({ bucket: name, prefix: undefined, object: undefined });
  }, [newBucket.name, setSearch]);

  const columns = useMemo(
    () => fileColumns((row, anchor) => setMenu({ ...anchor, row })),
    [],
  );

  const crumbs = useMemo<BreadcrumbSegment[]>(() => {
    if (bucket === undefined) return [];
    const segments = prefixSegments(prefix);
    return [
      {
        label: bucket,
        href: "/developer/files",
        search: { bucket },
        active: segments.length === 0,
      },
      ...segments.map((segment, index) => ({
        label: segment.label,
        href: "/developer/files",
        search: { bucket, prefix: segment.prefix },
        active: index === segments.length - 1,
      })),
    ];
  }, [bucket, prefix]);

  const settledEmpty = listing.data !== undefined && rows.length === 0;
  const canUpload = tenant !== null && bucket !== undefined;

  let body: React.ReactNode;
  if (tenant === null) {
    body = (
      <Panel>
        <EmptyState
          title="Select a tenant"
          body="Pick a tenant from the sidebar selector to browse its buckets and objects."
          testid="files-empty-tenant"
        />
      </Panel>
    );
  } else if (buckets.error !== undefined) {
    body = (
      <LoadFailed
        what="buckets"
        error={buckets.error}
        onRetry={reload}
        testid="files-load-failed"
      />
    );
  } else if (
    bucket === undefined &&
    buckets.data !== undefined &&
    buckets.data.length === 0
  ) {
    body = (
      <Panel>
        <EmptyState
          mascot="empty"
          title="No files yet"
          body="A bucket exists as soon as it holds an object. Name one with New bucket and drop a file on the listing, or put the first object through the API or the S3 listener."
          cta={{
            label: "New bucket",
            onClick: () => setNewBucket({ open: true, name: "" }),
          }}
          snippet={uploadObjectCommand({ serverUrl, tenant })}
          testid="files-empty-buckets"
        />
      </Panel>
    );
  } else {
    body = (
      <>
        {crumbs.length > 0 ? (
          <Breadcrumb segments={crumbs} testid="files-breadcrumb" />
        ) : null}
        <UploadQueue
          items={uploads}
          onDismiss={dismissUpload}
          testid="files-uploads"
        />
        <DropZone
          onFiles={startUploads}
          disabled={!canUpload}
          label={`Drop files to upload to ${bucket ?? "the bucket"}${prefix ? `/${prefix}` : ""}`}
          testid="files-drop-zone"
          className="flex min-h-0 flex-1 flex-col"
        >
          <Panel>
            {listing.error !== undefined ? (
              <LoadFailed
                what="objects"
                error={listing.error}
                onRetry={reload}
                testid="files-load-failed"
                className="m-4"
              />
            ) : settledEmpty ? (
              <EmptyState
                title={
                  prefix ? `Nothing under ${prefix}` : `No objects in ${bucket}`
                }
                body={
                  prefix
                    ? "No key in this bucket starts with this prefix. Drop a file here to write the first one, or step back up the breadcrumb."
                    : "Drop a file on this panel or use Upload; each file becomes one object keyed by its name. Whole objects up to 16 MiB go through the console."
                }
                cta={{ label: "Upload a file", onClick: openChooser }}
                testid="files-empty-prefix"
              />
            ) : (
              <DataTable
                columns={columns}
                data={rows}
                getRowId={(row) => row.id}
                ariaLabel={`Objects in ${bucket ?? "bucket"}`}
                loading={listing.loading || listing.data === undefined}
                onRowActivate={openRow}
                onRowContextMenu={(row, anchor) => setMenu({ ...anchor, row })}
                rowTestid={(row) => `files-row-${row.id}`}
                testid="files-table"
                className="h-full"
              />
            )}
          </Panel>
        </DropZone>
        {listing.data?.truncated ? (
          <p
            className="shrink-0 text-xs text-text-3"
            data-testid="files-truncated"
          >
            Showing the first {LIST_LIMIT} keys under this prefix. Open a folder
            to narrow the listing and see the rest.
          </p>
        ) : null}
      </>
    );
  }

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-files"
    >
      <PageHeader
        title="Files"
        subtitle="Objects the tenant stores, one bucket at a time. Uploads and downloads ride the console session."
        trailing={
          <div className="flex items-center gap-2">
            <input
              ref={fileInput}
              type="file"
              multiple
              className="hidden"
              data-testid="files-upload-input"
              onChange={(event) => {
                const files = Array.from(event.currentTarget.files ?? []);
                event.currentTarget.value = "";
                if (files.length > 0) startUploads(files);
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setNewBucket({ open: true, name: "" })}
              disabled={tenant === null}
              data-testid="files-new-bucket"
            >
              <FolderPlus data-icon="inline-start" />
              New bucket
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={openChooser}
              disabled={!canUpload}
              data-testid="files-upload"
            >
              <Upload data-icon="inline-start" />
              Upload
            </Button>
          </div>
        }
      />

      {body}

      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for ${menu.row.name}`}
          items={menuItems(menu)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="files-row-menu"
        />
      ) : null}

      {tenant !== null && bucket !== undefined ? (
        <ObjectSheet
          tenant={tenant}
          bucket={bucket}
          objectKey={search.object}
          objects={listing.data?.objects}
          loading={listing.loading}
          onClose={() => void setSearch({ object: undefined })}
          onDelete={setConfirmDelete}
        />
      ) : null}

      <ConfirmDialog
        open={confirmDelete !== null}
        title={
          confirmDelete
            ? `Delete ${confirmDelete.key.split("/").pop() || confirmDelete.key}?`
            : "Delete object?"
        }
        description={
          confirmDelete
            ? `${bucket}/${confirmDelete.key} (${formatBytes(confirmDelete.size)}) is removed from the bucket.`
            : undefined
        }
        confirmLabel="Delete object"
        danger
        busy={deleteState.busy}
        error={deleteState.error}
        onConfirm={() => void deleteConfirmed()}
        onCancel={() => {
          setConfirmDelete(null);
          setDeleteState({ busy: false });
        }}
        testid="files-delete"
      />

      <ConfirmDialog
        open={newBucket.open}
        title="New bucket"
        description="A bucket is a name; it exists once the first object lands in it. The listing opens on the name so the first upload goes there."
        confirmLabel="Open bucket"
        confirmDisabled={!isValidBucketName(newBucket.name.trim())}
        onConfirm={createBucket}
        onCancel={() => setNewBucket({ open: false, name: "" })}
        testid="files-new-bucket-dialog"
      >
        <div className="flex flex-col gap-1 text-xs text-text-3">
          <label htmlFor={newBucketId}>Bucket name</label>
          <Input
            id={newBucketId}
            value={newBucket.name}
            autoComplete="off"
            spellCheck={false}
            placeholder="assets"
            onChange={(event) =>
              setNewBucket({ open: true, name: event.target.value })
            }
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                createBucket();
              }
            }}
            data-testid="files-new-bucket-name"
            className="font-mono"
          />
          {newBucket.name.trim().length > 0 &&
          !isValidBucketName(newBucket.name.trim()) ? (
            <span className="text-error">
              A bucket name cannot contain a slash and is at most 63 bytes.
            </span>
          ) : (
            <span>
              One to 63 bytes, no slash. The S3 listener sees the same name.
            </span>
          )}
        </div>
      </ConfirmDialog>
    </section>
  );
}

// The panel around the listing: the bordered surface every page puts its
// table in, sized to fill what the page leaves it.
function Panel({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
      {children}
    </div>
  );
}

// The sub-panel lists what the server reports plus the bucket the address
// names, so a bucket opened through New bucket has a row before its first
// object lands.
function bucketsShown(
  buckets: ObjectBucket[] | undefined,
  selected: string | undefined,
): ObjectBucket[] {
  const known = buckets ?? [];
  if (selected === undefined || known.some((b) => b.bucket === selected)) {
    return known;
  }
  return [{ bucket: selected, objectCount: 0, totalBytes: 0 }, ...known];
}

function BucketsSubPanel({
  buckets,
  selected,
  loading,
  tenant,
}: {
  buckets: ObjectBucket[];
  selected: string | undefined;
  loading: boolean;
  tenant: string | null;
}) {
  const filter = useSubPanelSearch().trim().toLowerCase();
  const filtered = filter
    ? buckets.filter((b) => b.bucket.toLowerCase().includes(filter))
    : buckets;
  if (tenant === null) {
    return (
      <div
        className="px-3 py-6 text-xs text-text-3"
        data-testid="files-drawer-note"
      >
        Select a tenant to see its buckets.
      </div>
    );
  }
  if (buckets.length === 0) {
    return (
      <div
        className="px-3 py-6 text-xs text-text-3"
        data-testid="files-drawer-note"
      >
        {loading ? (
          "Reading buckets…"
        ) : (
          <>
            <p>No buckets yet.</p>
            <p className="mt-2">
              A bucket appears here once it holds an object. Use New bucket on
              the page to name the first one.
            </p>
          </>
        )}
      </div>
    );
  }
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        No buckets match the filter.
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {filtered.map((b) => {
        const active = b.bucket === selected;
        return (
          <li key={b.bucket}>
            <Link
              to="/developer/files"
              search={{ bucket: b.bucket }}
              data-testid={`sub-panel-item-bucket-${b.bucket}`}
              data-active={active || undefined}
              className={
                active
                  ? "flex h-8 items-center gap-2 rounded-md bg-bg-raised px-2 text-sm text-text-1"
                  : "flex h-8 items-center gap-2 rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1"
              }
            >
              <Folder className="size-3.5 shrink-0" aria-hidden />
              <span className="flex-1 truncate font-mono text-xs">
                {b.bucket}
              </span>
              <span className="tabular text-xs text-text-3">
                {b.objectCount}
              </span>
              <span className="tabular text-xs text-text-3">
                {formatBytes(b.totalBytes)}
              </span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

const fileCol = dataColumns<FileRow>();

function fileColumns(onMenu: (row: FileRow, anchor: RowAnchor) => void) {
  return [
    fileCol.accessor("name", {
      header: "Name",
      size: 300,
      cell: (ctx) => {
        const row = ctx.row.original;
        return (
          <span className="flex min-w-0 items-center gap-2">
            {row.kind === "folder" ? (
              <Folder className="size-3.5 shrink-0 text-text-3" aria-hidden />
            ) : (
              <File className="size-3.5 shrink-0 text-text-3" aria-hidden />
            )}
            <span
              className="truncate font-mono text-xs text-text-1"
              title={row.kind === "folder" ? row.prefix : row.object.key}
            >
              {row.name}
            </span>
            {row.kind === "folder" ? (
              <span className="shrink-0 text-xs text-text-3">
                {row.count} {row.count === 1 ? "item" : "items"}
              </span>
            ) : null}
          </span>
        );
      },
    }),
    fileCol.display({
      id: "size",
      header: () => <span className="block text-right">Size</span>,
      size: 96,
      cell: (ctx) => {
        const row = ctx.row.original;
        return (
          <span className="block text-right font-mono text-xs tabular text-text-3">
            {row.kind === "object" ? formatBytes(row.object.size) : "—"}
          </span>
        );
      },
    }),
    fileCol.display({
      id: "type",
      header: "Type",
      size: 160,
      cell: (ctx) => {
        const row = ctx.row.original;
        return (
          <span className="block truncate font-mono text-xs text-text-3">
            {row.kind === "object"
              ? shortContentType(row.object.contentType)
              : "folder"}
          </span>
        );
      },
    }),
    fileCol.display({
      id: "modified",
      header: "Modified",
      size: 120,
      cell: (ctx) => {
        const row = ctx.row.original;
        return row.kind === "object" ? (
          <RelativeTime epochMs={row.object.lastModifiedMillis} />
        ) : (
          <span className="tabular text-text-3">—</span>
        );
      },
    }),
    fileCol.display({
      id: "actions",
      header: "",
      size: 40,
      enableSorting: false,
      enableResizing: false,
      cell: (ctx) => {
        const row = ctx.row.original;
        return (
          <span className="flex justify-end">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label={`Actions for ${row.name}`}
              data-testid={`files-row-actions-${row.id}`}
              onClick={(event) => {
                event.stopPropagation();
                const rect = event.currentTarget.getBoundingClientRect();
                onMenu(row, {
                  x: rect.left,
                  y: rect.bottom + 2,
                  element: event.currentTarget,
                });
              }}
            >
              <Ellipsis />
            </Button>
          </span>
        );
      },
    }),
  ];
}
