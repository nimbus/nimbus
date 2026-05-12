'use strict';
const assert = require('assert');
const Module = require('module');

global.__neovexNodeShapeWrapperCounter = 0;

Module.wrapper = [
  '(function (exports, require, module, __filename, __dirname) { global.__neovexNodeShapeWrapperCounter = (global.__neovexNodeShapeWrapperCounter || 0) + 1',
  '\n});',
];

require('./not-main-module.js');

assert.strictEqual(global.__neovexNodeShapeWrapperCounter, 1);
