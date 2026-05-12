'use strict';

const path = require('node:path');

module.exports = {
  mode: 'package-main-resolution',
  dirnameBasename: path.basename(__dirname),
};
