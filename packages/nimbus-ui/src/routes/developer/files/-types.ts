import type { ObjectSummary } from "../../../lib/api-mutations";

// The address holds the whole browsing state: the bucket, the prefix the
// table shows, and the object the sheet is open on. A reload or a shared
// link lands on the same view.
export type FilesSearch = {
  bucket?: string;
  prefix?: string;
  object?: string;
};

function nonEmpty(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

export function parseFilesSearch(search: Record<string, unknown>): FilesSearch {
  return {
    bucket: nonEmpty(search.bucket),
    prefix: nonEmpty(search.prefix),
    object: nonEmpty(search.object),
  };
}

// The same rule the server applies (`validate_bucket` in
// crates/nimbus-server/src/http/objects.rs): one to 63 bytes, no slash.
export function isValidBucketName(name: string): boolean {
  if (name.length === 0 || name.includes("/") || name.includes("\0")) {
    return false;
  }
  return new TextEncoder().encode(name).length <= 63;
}

// The table lists one prefix level: keys that continue past the next slash
// fold into one folder row, keys that end before it are object rows. The
// listing itself is flat; the folders are the console's reading of it.
export type FileRow =
  | {
      kind: "folder";
      id: string;
      name: string;
      prefix: string;
      count: number;
    }
  | { kind: "object"; id: string; name: string; object: ObjectSummary };

export function deriveRows(
  objects: readonly ObjectSummary[],
  prefix: string,
): FileRow[] {
  const folders = new Map<string, number>();
  const files: FileRow[] = [];
  for (const object of objects) {
    if (!object.key.startsWith(prefix)) continue;
    const rest = object.key.slice(prefix.length);
    if (rest.length === 0) continue;
    const slash = rest.indexOf("/");
    if (slash === -1) {
      files.push({
        kind: "object",
        id: object.key,
        name: rest,
        object,
      });
    } else {
      const name = rest.slice(0, slash);
      folders.set(name, (folders.get(name) ?? 0) + 1);
    }
  }
  const folderRows: FileRow[] = Array.from(folders, ([name, count]) => ({
    kind: "folder",
    id: `${prefix}${name}/`,
    name,
    prefix: `${prefix}${name}/`,
    count,
  }));
  folderRows.sort((a, b) => a.name.localeCompare(b.name));
  files.sort((a, b) => a.name.localeCompare(b.name));
  return [...folderRows, ...files];
}

// The crumbs above the table: the bucket, then one crumb per folder in the
// prefix, each addressing the prefix up to and including itself.
export function prefixSegments(
  prefix: string,
): { label: string; prefix: string }[] {
  const parts = prefix.split("/").filter((part) => part.length > 0);
  return parts.map((label, index) => ({
    label,
    prefix: `${parts.slice(0, index + 1).join("/")}/`,
  }));
}

export const TEXT_PREVIEW_MAX_BYTES = 256 * 1024;

export type PreviewKind = "image" | "text" | "none";

const TEXT_EXTENSIONS = new Set([
  "txt",
  "md",
  "json",
  "js",
  "mjs",
  "cjs",
  "ts",
  "tsx",
  "jsx",
  "css",
  "html",
  "htm",
  "xml",
  "svg",
  "yaml",
  "yml",
  "toml",
  "csv",
  "log",
  "sh",
  "sql",
  "rs",
  "py",
  "go",
]);

// What the sheet can show inline. An image renders through the browser; a
// text-like object under the size bound is read and shown as-is. Anything
// else offers the download only. The content type wins; the extension is
// the fallback for an object stored without one.
export function previewKind(
  contentType: string | null,
  key: string,
  size: number,
): PreviewKind {
  const type = (contentType ?? "").split(";")[0]?.trim().toLowerCase() ?? "";
  if (type.startsWith("image/")) return "image";
  const ext = key.split("/").pop()?.split(".").pop()?.toLowerCase() ?? "";
  if (!type && ["png", "jpg", "jpeg", "gif", "webp", "avif"].includes(ext)) {
    return "image";
  }
  if (size > TEXT_PREVIEW_MAX_BYTES) return "none";
  if (
    type.startsWith("text/") ||
    type === "application/json" ||
    type === "application/xml" ||
    type === "application/javascript" ||
    type === "application/x-yaml" ||
    type.endsWith("+json") ||
    type.endsWith("+xml")
  ) {
    return "text";
  }
  if (!type && TEXT_EXTENSIONS.has(ext)) return "text";
  return "none";
}

// The short type the table shows: `image/png` stays, a parameter such as
// `; charset=utf-8` goes, and a missing type reads as a dash.
export function shortContentType(contentType: string | null): string {
  if (!contentType) return "—";
  return contentType.split(";")[0]?.trim() || "—";
}
