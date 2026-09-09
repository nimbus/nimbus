import { useEffect, useRef, useState } from "react";

import { type ApiResult, apiFetch } from "../lib/api-mutations";
import type { LoadingValue } from "../shell/loading-value";

// Default projection: a successful body is the ok value, any failure is an
// error. Call sites that need to promote a specific status to a typed value
// (e.g. the Source tab mapping 404 → "missing") pass their own `select`.
function defaultSelect<T>(result: ApiResult<T>): LoadingValue<T> {
  return result.ok
    ? { kind: "ok", value: result.data }
    : { kind: "error", message: result.error };
}

// One-shot HTTP read for the console's non-reactive endpoints (call graph,
// module source, license/encryption/runtime diagnostics). Runs through the
// shared `apiFetch` core in an effect keyed on `path`, aborts and drops the
// in-flight read on unmount or a path change (no state update afterwards), and
// reports a `LoadingValue<T>` — the console's single loading vocabulary.
//
// `revision` re-runs the read for the same path; a page bumps it after a
// write it made, or on a poll tick while the resource is still moving.
//
// `R` is the raw response body when a `select` maps it to a different `T`;
// without a `select`, `R` defaults to `T` and the body is used as-is.
export function useApiRead<T, R = T>(
  path: string,
  select: (result: ApiResult<R>) => LoadingValue<T> = defaultSelect as (
    result: ApiResult<R>,
  ) => LoadingValue<T>,
  // Bump to read the same path again (after a write, or on a poll tick).
  revision = 0,
): LoadingValue<T> {
  const [value, setValue] = useState<LoadingValue<T>>({ kind: "loading" });
  const selectRef = useRef(select);
  selectRef.current = select;
  const readPath = useRef<string | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: revision is the re-read trigger
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    // A new path starts from loading. A re-read of the same path keeps the
    // last value on screen until the fresh one lands, so a poll or a
    // post-write refresh does not blink the page back to its skeleton.
    if (readPath.current !== path) {
      readPath.current = path;
      setValue({ kind: "loading" });
    }
    void apiFetch<R>(path, { signal: controller.signal }).then((result) => {
      if (cancelled) return;
      setValue(selectRef.current(result));
    });
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [path, revision]);

  return value;
}
