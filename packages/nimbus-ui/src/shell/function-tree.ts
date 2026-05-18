export type FunctionLeaf = {
  kind: "function";
  path: string;
  name: string;
  lastStatus?: string;
};

export type ModuleNode = {
  kind: "module";
  name: string;
  fullPath: string;
  functions: FunctionLeaf[];
};

export type FolderNode = {
  kind: "folder";
  name: string;
  fullPath: string;
  folders: FolderNode[];
  modules: ModuleNode[];
};

export type FunctionTree = {
  folders: FolderNode[];
  modules: ModuleNode[];
  count: number;
};

export type FunctionInput = {
  path?: string;
  lastStatus?: string;
};

type Parsed = {
  folders: string[];
  module: string;
  fn: string;
};

export function parseFunctionPath(path: string): Parsed {
  const colon = path.indexOf(":");
  const before = colon === -1 ? path : path.slice(0, colon);
  const after = colon === -1 ? "default" : path.slice(colon + 1);
  const parts = before.split("/").filter((p) => p.length > 0);
  const module = parts.pop() ?? "";
  return { folders: parts, module, fn: after };
}

export function buildFunctionTree(fns: FunctionInput[]): FunctionTree {
  const root: FunctionTree = { folders: [], modules: [], count: 0 };
  for (const fn of fns) {
    const path = fn.path;
    if (!path) continue;
    const parsed = parseFunctionPath(path);
    const folderChain = ensureFolderChain(root, parsed.folders);
    const moduleFullPath = [...parsed.folders, parsed.module]
      .filter((s) => s.length > 0)
      .join("/");
    const moduleNode = ensureModule(folderChain, parsed.module, moduleFullPath);
    moduleNode.functions.push({
      kind: "function",
      path,
      name: parsed.fn,
      lastStatus: fn.lastStatus,
    });
    root.count += 1;
  }
  sortTree(root);
  return root;
}

function ensureFolderChain(
  root: FunctionTree,
  folders: string[],
): { folders: FolderNode[]; modules: ModuleNode[] } {
  let cursor: { folders: FolderNode[]; modules: ModuleNode[] } = root;
  let prefix: string[] = [];
  for (const folder of folders) {
    prefix = [...prefix, folder];
    const fullPath = prefix.join("/");
    let next = cursor.folders.find((f) => f.name === folder);
    if (!next) {
      next = {
        kind: "folder",
        name: folder,
        fullPath,
        folders: [],
        modules: [],
      };
      cursor.folders.push(next);
    }
    cursor = next;
  }
  return cursor;
}

function ensureModule(
  container: { modules: ModuleNode[] },
  name: string,
  fullPath: string,
): ModuleNode {
  let mod = container.modules.find((m) => m.name === name);
  if (!mod) {
    mod = { kind: "module", name, fullPath, functions: [] };
    container.modules.push(mod);
  }
  return mod;
}

function sortTree(node: {
  folders: FolderNode[];
  modules: ModuleNode[];
}): void {
  node.folders.sort((a, b) => a.name.localeCompare(b.name));
  node.modules.sort((a, b) => a.name.localeCompare(b.name));
  for (const mod of node.modules) {
    mod.functions.sort((a, b) => a.name.localeCompare(b.name));
  }
  for (const folder of node.folders) {
    sortTree(folder);
  }
}

export function filterFunctionTree(
  tree: FunctionTree,
  needle: string,
): FunctionTree {
  const trimmed = needle.trim().toLowerCase();
  if (trimmed === "") return tree;
  return {
    folders: tree.folders
      .map((f) => filterFolder(f, trimmed))
      .filter((f): f is FolderNode => f !== null),
    modules: tree.modules
      .map((m) => filterModule(m, trimmed))
      .filter((m): m is ModuleNode => m !== null),
    count: 0, // recomputed below
  };
}

function filterFolder(folder: FolderNode, needle: string): FolderNode | null {
  const nameMatches = folder.name.toLowerCase().includes(needle);
  const filteredFolders = folder.folders
    .map((f) => filterFolder(f, needle))
    .filter((f): f is FolderNode => f !== null);
  const filteredModules = folder.modules
    .map((m) => filterModule(m, needle))
    .filter((m): m is ModuleNode => m !== null);
  if (nameMatches) {
    // include all children when the folder name matches
    return folder;
  }
  if (filteredFolders.length === 0 && filteredModules.length === 0) return null;
  return {
    kind: "folder",
    name: folder.name,
    fullPath: folder.fullPath,
    folders: filteredFolders,
    modules: filteredModules,
  };
}

function filterModule(mod: ModuleNode, needle: string): ModuleNode | null {
  const nameMatches = mod.name.toLowerCase().includes(needle);
  if (nameMatches) return mod;
  const filteredFns = mod.functions.filter(
    (fn) =>
      fn.name.toLowerCase().includes(needle) ||
      fn.path.toLowerCase().includes(needle),
  );
  if (filteredFns.length === 0) return null;
  return {
    kind: "module",
    name: mod.name,
    fullPath: mod.fullPath,
    functions: filteredFns,
  };
}

export function collectAllPaths(tree: FunctionTree): string[] {
  const out: string[] = [];
  const visit = (node: { folders: FolderNode[]; modules: ModuleNode[] }) => {
    for (const f of node.folders) {
      out.push(`folder:${f.fullPath}`);
      visit(f);
    }
    for (const m of node.modules) {
      out.push(`module:${m.fullPath}`);
    }
  };
  visit(tree);
  return out;
}
