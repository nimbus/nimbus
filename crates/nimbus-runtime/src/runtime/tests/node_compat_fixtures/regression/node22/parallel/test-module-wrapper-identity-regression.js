'use strict';
const assert = require('assert');
const moduleBare = require('module');
const moduleScheme = require('node:module');

assert.strictEqual(moduleBare, moduleScheme);

const originalWrapper = moduleBare.wrapper;
const patchedWrapper = { ...originalWrapper };
patchedWrapper[0] += 'global.__nimbusModuleWrapperIdentity = 1;';
moduleScheme.wrapper = patchedWrapper;

assert.strictEqual(moduleBare.wrapper, patchedWrapper);
assert.match(moduleBare.wrap(''), /__nimbusModuleWrapperIdentity = 1/);

moduleBare.wrapper = originalWrapper;
delete global.__nimbusModuleWrapperIdentity;
