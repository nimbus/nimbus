#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(scriptPath), "..");

const excludedPrefixes = [
  "docs/plans/archive/",
  "docs/plans/proof/",
  "crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/",
  "target/",
  "node_modules/",
  "dist/",
];

const externalSchemes = new Set([
  "http:",
  "https:",
  "mailto:",
  "tel:",
  "data:",
  "app:",
  "chrome:",
  "vscode:",
]);

function runGitLsFiles() {
  const result = spawnSync(
    "git",
    [
      "ls-files",
      "--cached",
      "--modified",
      "--others",
      "--exclude-standard",
      "*.md",
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
    },
  );

  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || "git ls-files failed");
  }

  const files = result.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((file) => fs.existsSync(path.join(repoRoot, file)));

  return [...new Set(files)];
}

function isExcluded(file) {
  return excludedPrefixes.some((prefix) => file.startsWith(prefix));
}

function normalizeRepoPath(file) {
  return file.split(path.sep).join("/");
}

function stripMarkdownNoise(markdown) {
  let text = markdown.replace(/```[\s\S]*?```/g, "");
  text = text.replace(/~~~[\s\S]*?~~~/g, "");
  text = text.replace(/`[^`\n]*`/g, "");
  return text;
}

function parseDestination(rawDestination) {
  const trimmed = rawDestination.trim();
  if (trimmed.length === 0) {
    return "";
  }

  if (trimmed.startsWith("<")) {
    const closeIndex = trimmed.indexOf(">");
    if (closeIndex > 0) {
      return trimmed.slice(1, closeIndex).trim();
    }
  }

  const firstWhitespace = trimmed.search(/\s/);
  if (firstWhitespace === -1) {
    return trimmed;
  }
  return trimmed.slice(0, firstWhitespace).trim();
}

function extractInlineLinks(markdown) {
  const links = [];
  const pattern = /!?\[[^\]\n]*(?:\][^\[\]\n]*)*\]\(([^)\n]+)\)/g;

  for (const match of markdown.matchAll(pattern)) {
    const destination = parseDestination(match[1]);
    if (destination) {
      links.push({ destination, offset: match.index ?? 0 });
    }
  }

  return links;
}

function extractReferenceDefinitions(markdown) {
  const links = [];
  const pattern = /^[ \t]{0,3}\[[^\]\n]+\]:[ \t]*(\S.*)$/gm;

  for (const match of markdown.matchAll(pattern)) {
    const destination = parseDestination(match[1]);
    if (destination) {
      links.push({ destination, offset: match.index ?? 0 });
    }
  }

  return links;
}

function lineForOffset(text, offset) {
  return text.slice(0, offset).split("\n").length;
}

function splitFragment(destination) {
  const hashIndex = destination.indexOf("#");
  if (hashIndex === -1) {
    return { target: destination, fragment: "" };
  }

  return {
    target: destination.slice(0, hashIndex),
    fragment: destination.slice(hashIndex + 1),
  };
}

function isExternalOrRoute(destination) {
  try {
    const parsed = new URL(destination);
    return externalSchemes.has(parsed.protocol);
  } catch {
    // Relative paths are not URL-parseable without a base.
  }

  if (destination.startsWith("//")) {
    return true;
  }

  // Absolute paths in docs usually describe HTTP routes, not repo files.
  return destination.startsWith("/");
}

function decodePathSegment(target, sourceFile, line) {
  try {
    return decodeURIComponent(target);
  } catch (error) {
    throw new Error(
      `${sourceFile}:${line}: link target is not valid URI encoding: ${target}`,
    );
  }
}

function slugifyHeading(text) {
  return text
    .trim()
    .toLowerCase()
    .replace(/<[^>]*>/g, "")
    .replace(/[`*_~]/g, "")
    .replace(/[^\p{Letter}\p{Number}\s-]/gu, "")
    .replace(/\s+/g, "-")
    .replace(/^-|-$/g, "");
}

function anchorSetForMarkdown(markdown) {
  const anchors = new Set();
  const counts = new Map();
  const headingPattern = /^(#{1,6})[ \t]+(.+?)\s*#*\s*$/gm;

  for (const match of markdown.matchAll(headingPattern)) {
    const baseSlug = slugifyHeading(match[2]);
    if (!baseSlug) {
      continue;
    }
    const currentCount = counts.get(baseSlug) ?? 0;
    counts.set(baseSlug, currentCount + 1);
    anchors.add(currentCount === 0 ? baseSlug : `${baseSlug}-${currentCount}`);
  }

  return anchors;
}

function shouldValidateAnchor(targetPath) {
  return targetPath.endsWith(".md") || targetPath.endsWith(".mdx");
}

function validateDocsRefs() {
  const trackedMarkdown = runGitLsFiles().filter((file) => !isExcluded(file));
  const markdownByFile = new Map();
  const anchorCache = new Map();
  const failures = [];

  for (const file of trackedMarkdown) {
    const absolutePath = path.join(repoRoot, file);
    markdownByFile.set(file, fs.readFileSync(absolutePath, "utf8"));
  }

  for (const [sourceFile, originalMarkdown] of markdownByFile) {
    const markdown = stripMarkdownNoise(originalMarkdown);
    const links = [
      ...extractInlineLinks(markdown),
      ...extractReferenceDefinitions(markdown),
    ];

    for (const link of links) {
      if (isExternalOrRoute(link.destination)) {
        continue;
      }

      const line = lineForOffset(markdown, link.offset);
      const { target, fragment } = splitFragment(link.destination);
      const decodedTarget = decodePathSegment(target, sourceFile, line);
      const sourceDir = path.dirname(path.join(repoRoot, sourceFile));
      const absoluteTarget = decodedTarget
        ? path.resolve(sourceDir, decodedTarget)
        : path.join(repoRoot, sourceFile);
      const relativeTarget = normalizeRepoPath(path.relative(repoRoot, absoluteTarget));

      if (!absoluteTarget.startsWith(repoRoot + path.sep)) {
        failures.push(
          `${sourceFile}:${line}: link escapes repository root: ${link.destination}`,
        );
        continue;
      }

      if (decodedTarget && !fs.existsSync(absoluteTarget)) {
        failures.push(
          `${sourceFile}:${line}: missing local link target ${link.destination}`,
        );
        continue;
      }

      if (!fragment || !shouldValidateAnchor(relativeTarget)) {
        continue;
      }

      const normalizedFragment = decodeURIComponent(fragment)
        .trim()
        .toLowerCase();
      if (!normalizedFragment) {
        continue;
      }

      if (!anchorCache.has(relativeTarget)) {
        const targetMarkdown = fs.readFileSync(absoluteTarget, "utf8");
        anchorCache.set(relativeTarget, anchorSetForMarkdown(targetMarkdown));
      }

      if (!anchorCache.get(relativeTarget).has(normalizedFragment)) {
        failures.push(
          `${sourceFile}:${line}: missing anchor #${fragment} in ${relativeTarget}`,
        );
      }
    }
  }

  if (failures.length > 0) {
    console.error("docs reference validation failed:");
    for (const failure of failures) {
      console.error(`  - ${failure}`);
    }
    console.error(`\n${failures.length} broken reference(s) found.`);
    process.exit(1);
  }

  console.log(
    `docs reference validation: pass (${trackedMarkdown.length} working-tree Markdown files)`,
  );
}

validateDocsRefs();
