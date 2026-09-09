// The scheduler names a job document `scheduled-job:{tenant}:{jobId}`
// (crates/nimbus-system/src/keys.rs). Each segment keeps ASCII letters and
// digits and spells every other byte as `~` plus two lowercase hex digits,
// so a job id `job-1` is stored as `job~2d1`. The cancel route wants the
// job id back, so the console reverses the last segment here.

const DOC_PREFIX = "scheduled-job:";

export function decodeKeySegment(segment: string): string {
  const bytes: number[] = [];
  let i = 0;
  while (i < segment.length) {
    const ch = segment[i];
    const hex = segment.slice(i + 1, i + 3);
    if (ch === "~" && hex.length === 2 && /^[0-9a-f]{2}$/.test(hex)) {
      bytes.push(Number.parseInt(hex, 16));
      i += 3;
    } else {
      bytes.push(...new TextEncoder().encode(ch as string));
      i += 1;
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

// The job id the scheduler knows the document by, or undefined when the
// id is not a scheduled-job document id.
export function jobIdFromDocumentId(documentId: string): string | undefined {
  if (!documentId.startsWith(DOC_PREFIX)) return undefined;
  const rest = documentId.slice(DOC_PREFIX.length);
  const cut = rest.indexOf(":");
  if (cut < 0) return undefined;
  const segment = rest.slice(cut + 1);
  return segment ? decodeKeySegment(segment) : undefined;
}
