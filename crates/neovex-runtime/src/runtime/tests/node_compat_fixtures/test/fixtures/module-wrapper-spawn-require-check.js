'use strict';
const assert = require('assert');
const Module = require('module');

global.__neovexSpawnRequireWrapperCounter = 0;

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__neovexSpawnRequireWrapperCounter = ' +
  '(global.__neovexSpawnRequireWrapperCounter || 0) + 1';

Module.wrapper = patchedWrapper;

assert.strictEqual(Module.wrapper, patchedWrapper);
assert.match(
  Module.wrap(''),
  /__neovexSpawnRequireWrapperCounter/,
);

require('./not-main-module.js');

assert.strictEqual(global.__neovexSpawnRequireWrapperCounter, 1);
