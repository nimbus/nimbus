// A run's stored error is `{ message, location?, stack? }`, where `location`
// is the `module:line` the server lifted from the runtime remap and `stack`
// is the developer's own stack when the function threw (see
// crates/nimbus-system/src/records/run.rs). These helpers read it back (with
// a string/JSON fallback for unstructured errors) so the run-detail view can
// link a failure to its source line and show the frames.

export type ParsedRunError = {
  message: string;
  location?: string;
  stack?: string;
};

export function parseRunError(error: unknown): ParsedRunError {
  if (error && typeof error === "object") {
    const record = error as {
      message?: unknown;
      location?: unknown;
      stack?: unknown;
    };
    const message =
      typeof record.message === "string"
        ? record.message
        : JSON.stringify(error, null, 2);
    const location =
      typeof record.location === "string" ? record.location : undefined;
    const stack =
      typeof record.stack === "string" && record.stack.length > 0
        ? record.stack
        : undefined;
    return { message, location, stack };
  }
  return { message: typeof error === "string" ? error : String(error) };
}

/** The 1-based line from a `module:line` location, or undefined if malformed. */
export function locationLine(location: string): number | undefined {
  const line = Number.parseInt(
    location.slice(location.lastIndexOf(":") + 1),
    10,
  );
  return Number.isFinite(line) && line > 0 ? line : undefined;
}
