import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const hasRequire = typeof require === 'function';
export const hasDirname = typeof __dirname !== 'undefined';
export const hasFilename = typeof __filename !== 'undefined';
export const metaFilename = fileURLToPath(import.meta.url);
export const metaDirname = path.dirname(metaFilename);
