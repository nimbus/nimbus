'use strict';
const assert = require('assert');
const Module = require('module');

global.__nimbusSpawnWrapCallCounter = 0;

let wrapCallCount = 0;
const originalWrap = Module.wrap;
Module.wrap = function wrappedModuleWrap(script) {
  wrapCallCount += 1;
  return originalWrap(script);
};

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusSpawnWrapCallCounter = ' +
  '(global.__nimbusSpawnWrapCallCounter || 0) + 1';

Module.wrapper = patchedWrapper;

require('./not-main-module.js');

assert.strictEqual(wrapCallCount, 1);
assert.strictEqual(global.__nimbusSpawnWrapCallCounter, 1);
