'use strict';
require('../common');
const fixtures = require('../common/fixtures');
const { execFileSync } = require('child_process');

execFileSync(process.execPath, [
  fixtures.path('module-wrapper-spawn-node-shape-check.js'),
], { stdio: 'pipe' });
