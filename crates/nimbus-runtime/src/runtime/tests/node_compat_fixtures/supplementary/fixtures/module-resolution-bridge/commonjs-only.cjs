'use strict';

const path = require('node:path');

module.exports = {
  mode: 'cjs-default-bridge',
  requiredBuiltin: path.basename('/tmp/x.js'),
};
