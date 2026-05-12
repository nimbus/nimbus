'use strict';
require('../common');
const fixtures = require('../common/fixtures');
const childProcess = require('child_process');

childProcess.execFileSync(
  process.execPath,
  [fixtures.path('module-wrapper-spawn-require-check.js')],
  { stdio: 'pipe' },
);
