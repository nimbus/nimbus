import { useMemo } from "react";

import type { FunctionDoc } from "../lib/types/function";
import { buildFunctionTree } from "./function-tree";
import { FunctionTreeView } from "./function-tree-view";
import { useSubPanelSearch } from "./sub-panel";

// The function tree as the Compute sub-panel, shared by the Compute page
// and every function page so the list keeps its shape from one to the next.
// The filter comes from the sub-panel's own search field.
export function FunctionSubPanel({
  functions,
}: {
  functions: FunctionDoc[] | undefined;
}) {
  const filter = useSubPanelSearch();
  const tree = useMemo(() => buildFunctionTree(functions ?? []), [functions]);
  if (functions === undefined) {
    return (
      <div className="px-3 py-3 text-xs text-text-3">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  return (
    <FunctionTreeView tree={tree} filter={filter} testidPrefix="sub-panel" />
  );
}
