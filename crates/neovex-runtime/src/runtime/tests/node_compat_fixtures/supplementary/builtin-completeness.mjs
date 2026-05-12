import assert from 'node:assert';
import * as fsBare from 'fs';
import * as fsNode from 'node:fs';
import * as pathBare from 'path';
import * as pathNode from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const fsRequired = require('fs');
const fsNodeRequired = require('node:fs');
const pathRequired = require('path');
const pathNodeRequired = require('node:path');
const nestedRequire = require('node:module').createRequire(import.meta.url);
const currentModuleUrl = new URL(import.meta.url);

assert.strictEqual(typeof fsBare.readFileSync, 'function');
assert.strictEqual(typeof fsNode.readFileSync, 'function');
assert.strictEqual(typeof pathBare.join, 'function');
assert.strictEqual(typeof pathNode.join, 'function');

const sourceViaBareImport = fsBare.readFileSync(currentModuleUrl, 'utf8');
const sourceViaNodeImport = fsNode.readFileSync(currentModuleUrl, 'utf8');
const sourceViaBareRequire = fsRequired.readFileSync(currentModuleUrl, 'utf8');
const sourceViaNodeRequire = fsNodeRequired.readFileSync(currentModuleUrl, 'utf8');
const sourceViaNestedRequire = nestedRequire('fs').readFileSync(currentModuleUrl, 'utf8');

assert.ok(sourceViaBareImport.includes('builtin-completeness'));
assert.strictEqual(sourceViaBareImport, sourceViaNodeImport);
assert.strictEqual(sourceViaBareImport, sourceViaBareRequire);
assert.strictEqual(sourceViaBareImport, sourceViaNodeRequire);
assert.strictEqual(sourceViaBareImport, sourceViaNestedRequire);

assert.strictEqual(pathBare.join('a', 'b'), 'a/b');
assert.strictEqual(pathNode.join('a', 'b'), 'a/b');
assert.strictEqual(pathRequired.join('a', 'b'), 'a/b');
assert.strictEqual(pathNodeRequired.join('a', 'b'), 'a/b');
assert.strictEqual(nestedRequire('path').join('a', 'b'), 'a/b');

if (typeof process.getBuiltinModule === 'function') {
  const builtinFs = process.getBuiltinModule('fs');
  const builtinPath = process.getBuiltinModule('path');
  assert.strictEqual(
    builtinFs?.readFileSync(currentModuleUrl, 'utf8'),
    sourceViaBareImport,
  );
  assert.strictEqual(builtinPath?.join('a', 'b'), 'a/b');
}
