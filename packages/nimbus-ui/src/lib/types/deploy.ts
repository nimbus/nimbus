// Deploy history as `GET /api/admin/deploys` reports it
// (`DeployHistory` in crates/nimbus-compute/src/deploy.rs). One row per
// bundle activation on this server, newest first: a `nimbus deploy`, a
// rollback from this console, or the bundle the server started with.

export const ACTIVATION_KINDS = ["deploy", "rollback", "startup"] as const;
export type ActivationKind = (typeof ACTIVATION_KINDS)[number];

export type DeployActivationFunction = { path: string; kind: string };

export type DeployActivation = {
  // The bundle's provenance hash: the SHA-256 the runtime recomputes and
  // checks before every invocation, so two rows with one hash ran the
  // same code.
  sha256: string;
  // The server's activation counter; 0 is the bundle it started with.
  generation: number;
  activatedAt: number;
  // `deploy-admin` for a CLI deploy, `operator[:method]` for a rollback,
  // `server` for the startup row.
  actor: string;
  sourceRef: string;
  kind: string;
  silo?: string;
  // The server kept the bundle's files, so a rollback can re-activate it
  // through the same integrity check a deploy passes. A startup row is
  // never retained.
  retained: boolean;
  functions: DeployActivationFunction[];
};

export type DeployHistory = {
  // The hash of the bundle serving requests now, or null before the first
  // deploy.
  active: string | null;
  activations: DeployActivation[];
};

export type RollbackResponse = {
  activated: boolean;
  generation: number;
  previousGeneration: number;
  sha256: string;
};

export type FunctionPathDiff = {
  added: string[];
  removed: string[];
  kept: string[];
};

// The function paths that appear, vanish, or stay when `to` replaces
// `from`. Paths sort so two renders of the same pair read the same.
export function diffFunctionPaths(
  from: DeployActivationFunction[],
  to: DeployActivationFunction[],
): FunctionPathDiff {
  const before = new Set(from.map((fn) => fn.path));
  const after = new Set(to.map((fn) => fn.path));
  const sorted = (paths: Iterable<string>) => [...paths].sort();
  return {
    added: sorted([...after].filter((path) => !before.has(path))),
    removed: sorted([...before].filter((path) => !after.has(path))),
    kept: sorted([...after].filter((path) => before.has(path))),
  };
}

// A rollback needs the bundle's retained files and must change something:
// the active bundle is already what a rollback to it would activate.
export function canRollBack(
  activation: DeployActivation,
  active: string | null,
): boolean {
  return activation.retained && activation.sha256 !== active;
}
