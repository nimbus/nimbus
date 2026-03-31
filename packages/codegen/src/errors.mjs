import path from "node:path";

function unsupportedError(filePath, detail) {
  return new Error(
    `${relativeForError(filePath)} requires Phase 4C runtime execution support (${detail}).`,
  );
}

function relativeForError(filePath) {
  return path.relative(process.cwd(), filePath);
}

export { relativeForError, unsupportedError };
