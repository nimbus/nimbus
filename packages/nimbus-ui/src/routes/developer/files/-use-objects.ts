import { useCallback, useEffect, useState } from "react";

import {
  type ObjectBucket,
  type ObjectListing,
  objects as objectApi,
} from "../../../lib/api-mutations";

// Object storage has no live query; the console reads it on demand and
// again after every write it makes. `version` is the reload handle the
// page bumps after an upload or a delete.
export type ObjectRead<T> = {
  data: T | undefined;
  error: string | undefined;
  loading: boolean;
};

export const LIST_LIMIT = 1000;

export function useBuckets(
  tenant: string | null,
  version: number,
): ObjectRead<ObjectBucket[]> {
  const [state, setState] = useState<ObjectRead<ObjectBucket[]>>({
    data: undefined,
    error: undefined,
    loading: tenant !== null,
  });
  // biome-ignore lint/correctness/useExhaustiveDependencies: version is the reload handle; a bump re-reads on purpose
  useEffect(() => {
    if (tenant === null) {
      setState({ data: undefined, error: undefined, loading: false });
      return;
    }
    let live = true;
    setState((prev) => ({ ...prev, loading: true }));
    void objectApi.buckets(tenant).then((result) => {
      if (!live) return;
      if (result.ok) {
        setState({
          data: result.data.buckets,
          error: undefined,
          loading: false,
        });
      } else {
        setState({ data: undefined, error: result.error, loading: false });
      }
    });
    return () => {
      live = false;
    };
  }, [tenant, version]);
  return state;
}

export function useObjectListing(
  tenant: string | null,
  bucket: string | undefined,
  prefix: string,
  version: number,
): ObjectRead<ObjectListing> {
  const [state, setState] = useState<ObjectRead<ObjectListing>>({
    data: undefined,
    error: undefined,
    loading: tenant !== null && bucket !== undefined,
  });
  // biome-ignore lint/correctness/useExhaustiveDependencies: version is the reload handle; a bump re-reads on purpose
  useEffect(() => {
    if (tenant === null || bucket === undefined) {
      setState({ data: undefined, error: undefined, loading: false });
      return;
    }
    let live = true;
    setState((prev) => ({ ...prev, loading: true }));
    void objectApi.list(tenant, bucket, prefix, LIST_LIMIT).then((result) => {
      if (!live) return;
      if (result.ok) {
        setState({ data: result.data, error: undefined, loading: false });
      } else {
        setState({ data: undefined, error: result.error, loading: false });
      }
    });
    return () => {
      live = false;
    };
  }, [tenant, bucket, prefix, version]);
  return state;
}

export function useReloadVersion(): [number, () => void] {
  const [version, setVersion] = useState(0);
  const reload = useCallback(() => setVersion((v) => v + 1), []);
  return [version, reload];
}
