// A run's stored error is `{ message, location? }`, where `location` is the
// `module:line` the server lifted from the runtime remap (see records.rs).
// These helpers read it back (with a string/JSON fallback for unstructured or
// older errors) so the run-detail view can link a failure to its source line.

export type ParsedRunError = { message: string; location?: string };

export function parseRunError(error: unknown): ParsedRunError {
  if (error && typeof error === "object") {
    const record = error as { message?: unknown; location?: unknown };
    const message =
      typeof record.message === "string"
        ? record.message
        : JSON.stringify(error, null, 2);
    const location =
      typeof record.location === "string" ? record.location : undefined;
    return { message, location };
  }
  return { message: typeof error === "string" ? error : String(error) };
}

/** The 1-based line from a `module:line` location, or undefined if malformed. */
export function locationLine(location: string): number | undefined {
  const line = Number.parseInt(location.slice(location.lastIndexOf(":") + 1), 10);
  return Number.isFinite(line) && line > 0 ? line : undefined;
}
