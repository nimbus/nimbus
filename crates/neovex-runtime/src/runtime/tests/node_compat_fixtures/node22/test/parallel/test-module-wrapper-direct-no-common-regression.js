'use strict';
const assert = require('assert');
const Module = require('module');

global.__neovexDirectNoCommonWrapperRegression = 0;

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__neovexDirectNoCommonWrapperRegression = ' +
  '(global.__neovexDirectNoCommonWrapperRegression || 0) + 1;';
Module.wrapper = patchedWrapper;

require('../fixtures/not-main-module.js');

assert.strictEqual(global.__neovexDirectNoCommonWrapperRegression, 1);

