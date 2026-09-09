import { describe, expect, it } from "vitest";

import type { ObjectSummary } from "../../../lib/api-mutations";
import {
  deriveRows,
  isValidBucketName,
  parseFilesSearch,
  prefixSegments,
  previewKind,
  shortContentType,
  TEXT_PREVIEW_MAX_BYTES,
} from "./-types";

function object(key: string, size = 1): ObjectSummary {
  return {
    bucket: "assets",
    key,
    size,
    contentType: null,
    etag: "e",
    lastModifiedMillis: 1,
  };
}

describe("deriveRows", () => {
  it("folds keys under the next slash into folders, folders first, sorted", () => {
    const rows = deriveRows(
      [
        object("zeta.txt"),
        object("docs/b.md"),
        object("docs/a.md"),
        object("img/logo.png"),
        object("alpha.txt"),
      ],
      "",
    );
    expect(rows.map((r) => `${r.kind}:${r.name}`)).toEqual([
      "folder:docs",
      "folder:img",
      "object:alpha.txt",
      "object:zeta.txt",
    ]);
    expect(rows[0]).toMatchObject({ prefix: "docs/", count: 2 });
  });

  it("lists one level under a prefix and skips the prefix itself", () => {
    const rows = deriveRows(
      [object("docs/"), object("docs/a.md"), object("docs/sub/c.md")],
      "docs/",
    );
    expect(rows.map((r) => r.id)).toEqual(["docs/sub/", "docs/a.md"]);
  });
});

describe("parseFilesSearch", () => {
  it("keeps non-empty strings only", () => {
    expect(
      parseFilesSearch({ bucket: "a", prefix: "", object: 3 as unknown }),
    ).toEqual({ bucket: "a", prefix: undefined, object: undefined });
  });
});

describe("isValidBucketName", () => {
  it("applies the server rule", () => {
    expect(isValidBucketName("assets")).toBe(true);
    expect(isValidBucketName("")).toBe(false);
    expect(isValidBucketName("a/b")).toBe(false);
    expect(isValidBucketName("x".repeat(63))).toBe(true);
    expect(isValidBucketName("x".repeat(64))).toBe(false);
  });
});

describe("prefixSegments", () => {
  it("addresses each folder by its full prefix", () => {
    expect(prefixSegments("docs/sub/")).toEqual([
      { label: "docs", prefix: "docs/" },
      { label: "sub", prefix: "docs/sub/" },
    ]);
    expect(prefixSegments("")).toEqual([]);
  });
});

describe("previewKind", () => {
  it("previews images by type or extension", () => {
    expect(previewKind("image/png", "x", 10)).toBe("image");
    expect(previewKind(null, "a/logo.webp", 10)).toBe("image");
  });

  it("previews text under the size bound and refuses it above", () => {
    expect(previewKind("text/plain; charset=utf-8", "a.txt", 10)).toBe("text");
    expect(previewKind("application/json", "a", 10)).toBe("text");
    expect(previewKind(null, "notes.md", 10)).toBe("text");
    expect(previewKind("text/plain", "a.txt", TEXT_PREVIEW_MAX_BYTES + 1)).toBe(
      "none",
    );
    expect(previewKind("application/octet-stream", "a.bin", 10)).toBe("none");
  });
});

describe("shortContentType", () => {
  it("drops parameters and dashes a missing type", () => {
    expect(shortContentType("text/plain; charset=utf-8")).toBe("text/plain");
    expect(shortContentType(null)).toBe("—");
  });
});
