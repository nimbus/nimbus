// Client-side TypeScript type-info extraction (FSV8). Uses the official TS
// Compiler API LanguageService to produce per-identifier hover info for one
// module — the editor-grade tooltip text (`getQuickInfoAtPosition`). Run by
// `nimbus deploy` where the project's `typescript` + types closure exist.
//
// Input:  NIMBUS_TYPEINFO_TARGET = absolute path to the module to analyze.
//         `typescript` is resolved from the process cwd (the app dir).
// Output: JSON array of { name, line, col, hover } on stdout.

import ts from "typescript";
import { readFileSync } from "node:fs";

const target = process.env.NIMBUS_TYPEINFO_TARGET;
if (!target) {
  process.stderr.write("NIMBUS_TYPEINFO_TARGET is required\n");
  process.exit(2);
}

const options = {
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ESNext,
  strict: true,
  noEmit: true,
  skipLibCheck: true,
  allowJs: true,
};

const host = {
  getScriptFileNames: () => [target],
  getScriptVersion: () => "1",
  getScriptSnapshot: (fileName) => {
    try {
      return ts.ScriptSnapshot.fromString(readFileSync(fileName, "utf8"));
    } catch {
      return undefined;
    }
  },
  getCurrentDirectory: () => process.cwd(),
  getCompilationSettings: () => options,
  getDefaultLibFileName: (o) => ts.getDefaultLibFilePath(o),
  fileExists: ts.sys.fileExists,
  readFile: ts.sys.readFile,
  readDirectory: ts.sys.readDirectory,
  directoryExists: ts.sys.directoryExists,
  getDirectories: ts.sys.getDirectories,
};

const service = ts.createLanguageService(host, ts.createDocumentRegistry());
const program = service.getProgram();
const source = program && program.getSourceFile(target);
if (!source) {
  process.stderr.write(`could not load ${target}\n`);
  process.exit(1);
}

const hints = [];
const seen = new Set();
const visit = (node) => {
  if (ts.isIdentifier(node)) {
    const pos = node.getStart(source);
    const info = service.getQuickInfoAtPosition(target, pos);
    if (info && info.displayParts) {
      const hover = ts.displayPartsToString(info.displayParts);
      const { line, character } = source.getLineAndCharacterOfPosition(pos);
      const key = `${line}:${character}`;
      if (hover && !seen.has(key)) {
        seen.add(key);
        hints.push({ name: node.text, line: line + 1, col: character + 1, hover });
      }
    }
  }
  ts.forEachChild(node, visit);
};
visit(source);
process.stdout.write(JSON.stringify(hints));
