'use strict';

module.exports = {
  hasRequire: typeof require === 'function',
  dirname: __dirname,
  filename: __filename,
  requiredValue: require('./cjs-required-value.cjs'),
  requiredBuiltinJoin: require('node:path').join('a', 'b'),
};
