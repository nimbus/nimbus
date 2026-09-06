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
// `R` is the raw response body when a `select` maps it to a different `T`;
// without a `select`, `R` defaults to `T` and the body is used as-is.
export function useApiRead<T, R = T>(
  path: string,
  select: (result: ApiResult<R>) => LoadingValue<T> = defaultSelect as (
    result: ApiResult<R>,
  ) => LoadingValue<T>,
): LoadingValue<T> {
  const [value, setValue] = useState<LoadingValue<T>>({ kind: "loading" });
  const selectRef = useRef(select);
  selectRef.current = select;

  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    setValue({ kind: "loading" });
    void apiFetch<R>(path, { signal: controller.signal }).then((result) => {
      if (cancelled) return;
      setValue(selectRef.current(result));
    });
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [path]);

  return value;
}
