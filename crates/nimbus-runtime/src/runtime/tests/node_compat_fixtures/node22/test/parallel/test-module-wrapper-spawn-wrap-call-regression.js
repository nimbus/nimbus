'use strict';
require('../common');
const fixtures = require('../common/fixtures');
const childProcess = require('child_process');

childProcess.execFileSync(
  process.execPath,
  [fixtures.path('module-wrapper-spawn-wrap-call-check.js')],
  { stdio: 'pipe' },
);
