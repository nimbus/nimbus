'use strict';
const assert = require('assert');
const Module = require('module');

global.__nimbusSpawnRequireWrapperCounter = 0;

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusSpawnRequireWrapperCounter = ' +
  '(global.__nimbusSpawnRequireWrapperCounter || 0) + 1';

Module.wrapper = patchedWrapper;

assert.strictEqual(Module.wrapper, patchedWrapper);
assert.match(
  Module.wrap(''),
  /__nimbusSpawnRequireWrapperCounter/,
);

require('./not-main-module.js');

assert.strictEqual(global.__nimbusSpawnRequireWrapperCounter, 1);
