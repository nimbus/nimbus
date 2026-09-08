import { describe, expect, it } from "vitest";

import { decodeKeySegment, jobIdFromDocumentId } from "./-job-ids";
import { formatSchedule } from "./-types";

describe("decodeKeySegment", () => {
  it("keeps ASCII letters and digits", () => {
    expect(decodeKeySegment("job1")).toBe("job1");
  });

  it("turns ~xx pairs back into the bytes they spell", () => {
    expect(decodeKeySegment("job~2d1")).toBe("job-1");
    expect(decodeKeySegment("a~3ab~2fc")).toBe("a:b/c");
  });

  it("decodes multi-byte characters from their byte pairs", () => {
    expect(decodeKeySegment("~c3~a9")).toBe("é");
  });

  it("leaves a lone or malformed tilde alone", () => {
    expect(decodeKeySegment("x~")).toBe("x~");
    expect(decodeKeySegment("x~zz")).toBe("x~zz");
  });
});

describe("jobIdFromDocumentId", () => {
  it("reads the job id off the last segment", () => {
    expect(jobIdFromDocumentId("scheduled-job:acme:job~2d1")).toBe("job-1");
  });

  it("decodes a tenant that itself carries an encoded byte", () => {
    expect(jobIdFromDocumentId("scheduled-job:obs~2de2e:j9")).toBe("j9");
  });

  it("answers undefined for a document id of another kind", () => {
    expect(jobIdFromDocumentId("cron-job:acme:sweep")).toBeUndefined();
    expect(jobIdFromDocumentId("scheduled-job:acme")).toBeUndefined();
    expect(jobIdFromDocumentId("scheduled-job:acme:")).toBeUndefined();
  });
});

describe("formatSchedule", () => {
  it("says an interval the way an operator would", () => {
    expect(formatSchedule("interval:30s")).toBe("every 30s");
    expect(formatSchedule("interval:120s")).toBe("every 2m");
    expect(formatSchedule("interval:7200s")).toBe("every 2h");
  });

  it("passes an unknown shape through and dashes a missing one", () => {
    expect(formatSchedule("0 * * * *")).toBe("0 * * * *");
    expect(formatSchedule(undefined)).toBe("—");
  });
});
