#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import process from "node:process";

function run(command, args) {
  return execFileSync(command, args, {
    cwd: process.cwd(),
    encoding: "utf8",
    maxBuffer: 128 * 1024 * 1024,
  }).trim();
}

function cargoMetadata(extraArgs) {
  const args = ["metadata", "--format-version", "1", ...extraArgs];
  return {
    command: `cargo ${args.join(" ")}`,
    metadata: JSON.parse(run("cargo", args)),
  };
}

function dependencyKind(kind) {
  return kind ?? "normal";
}

function workspaceEdges(metadata, acceptedKinds = null) {
  const workspaceIds = new Set(metadata.workspace_members);
  const names = new Map(
    metadata.packages.map((pkg) => [pkg.id, pkg.name]),
  );
  const edges = [];

  for (const node of metadata.resolve?.nodes ?? []) {
    if (!workspaceIds.has(node.id)) {
      continue;
    }
    for (const dependency of node.deps) {
      if (!workspaceIds.has(dependency.pkg)) {
        continue;
      }
      for (const dependencyMetadata of dependency.dep_kinds) {
        const kind = dependencyKind(dependencyMetadata.kind);
        if (acceptedKinds && !acceptedKinds.has(kind)) {
          continue;
        }
        edges.push({
          from: names.get(node.id),
          to: names.get(dependency.pkg),
          kind,
          target: dependencyMetadata.target ?? null,
        });
      }
    }
  }

  return edges.sort((left, right) =>
    JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
}

function declaredWorkspaceEdges(metadata) {
  const workspaceNames = new Set(
    metadata.packages
      .filter((pkg) => metadata.workspace_members.includes(pkg.id))
      .map((pkg) => pkg.name),
  );
  const edges = [];

  for (const pkg of metadata.packages) {
    if (!metadata.workspace_members.includes(pkg.id)) {
      continue;
    }
    for (const dependency of pkg.dependencies) {
      if (!workspaceNames.has(dependency.name)) {
        continue;
      }
      edges.push({
        from: pkg.name,
        to: dependency.name,
        kind: dependencyKind(dependency.kind),
        target: dependency.target ?? null,
        optional: dependency.optional,
        uses_default_features: dependency.uses_default_features,
        features: [...dependency.features].sort(),
      });
    }
  }

  return edges.sort((left, right) =>
    JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
}

function stronglyConnectedCycles(edges) {
  const adjacency = new Map();
  for (const { from, to } of edges) {
    if (!adjacency.has(from)) {
      adjacency.set(from, new Set());
    }
    if (!adjacency.has(to)) {
      adjacency.set(to, new Set());
    }
    adjacency.get(from).add(to);
  }

  let nextIndex = 0;
  const indices = new Map();
  const lowLinks = new Map();
  const stack = [];
  const onStack = new Set();
  const cycles = [];

  function visit(node) {
    indices.set(node, nextIndex);
    lowLinks.set(node, nextIndex);
    nextIndex += 1;
    stack.push(node);
    onStack.add(node);

    for (const dependency of adjacency.get(node) ?? []) {
      if (!indices.has(dependency)) {
        visit(dependency);
        lowLinks.set(
          node,
          Math.min(lowLinks.get(node), lowLinks.get(dependency)),
        );
      } else if (onStack.has(dependency)) {
        lowLinks.set(
          node,
          Math.min(lowLinks.get(node), indices.get(dependency)),
        );
      }
    }

    if (lowLinks.get(node) !== indices.get(node)) {
      return;
    }
    const component = [];
    while (stack.length > 0) {
      const member = stack.pop();
      onStack.delete(member);
      component.push(member);
      if (member === node) {
        break;
      }
    }
    const selfCycle =
      component.length === 1 &&
      (adjacency.get(component[0]) ?? new Set()).has(component[0]);
    if (component.length > 1 || selfCycle) {
      cycles.push(component.sort());
    }
  }

  for (const node of [...adjacency.keys()].sort()) {
    if (!indices.has(node)) {
      visit(node);
    }
  }

  return cycles.sort((left, right) =>
    JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
}

function profile(name, command, target, metadata, acceptedKinds = null) {
  const edges = workspaceEdges(metadata, acceptedKinds);
  return {
    name,
    command,
    target,
    edge_count: edges.length,
    cycles: stronglyConnectedCycles(edges),
    edges,
  };
}

const sourceHead = run("git", ["rev-parse", "HEAD"]);
const rustcVersion = run("rustc", ["-vV"]);
const host = rustcVersion
  .split("\n")
  .find((line) => line.startsWith("host: "))
  ?.slice("host: ".length);
if (!host) {
  throw new Error("rustc -vV did not report a host target");
}

const requestedTargets = [
  host,
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "aarch64-apple-darwin",
  "x86_64-pc-windows-msvc",
];
const targets = [...new Set(requestedTargets)];
const defaultHost = cargoMetadata(["--filter-platform", host]);
const allFeatureMetadata = new Map();
for (const target of targets) {
  allFeatureMetadata.set(
    target,
    cargoMetadata(["--all-features", "--filter-platform", target]),
  );
}

const profiles = [
  profile(
    "normal-default-host",
    defaultHost.command,
    host,
    defaultHost.metadata,
    new Set(["normal"]),
  ),
  profile(
    "dev-test-build-default-host",
    defaultHost.command,
    host,
    defaultHost.metadata,
  ),
];
for (const target of targets) {
  const captured = allFeatureMetadata.get(target);
  profiles.push(
    profile(
      `all-features-all-kinds-${target}`,
      captured.command,
      target,
      captured.metadata,
    ),
  );
}

const result = {
  schema_version: 1,
  source_head: sourceHead,
  rustc_host: host,
  targets,
  commands: [
    "git rev-parse HEAD",
    "rustc -vV",
    defaultHost.command,
    ...targets.map((target) => allFeatureMetadata.get(target).command),
  ],
  declared_workspace_edges: declaredWorkspaceEdges(defaultHost.metadata),
  profiles,
};

process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
