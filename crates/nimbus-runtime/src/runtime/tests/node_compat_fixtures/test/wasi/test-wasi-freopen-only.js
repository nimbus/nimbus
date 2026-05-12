'use strict';
require('../common');
const { readFileSync } = require('fs');
const { testWasiPreview1 } = require('../common/wasi');

const checkoutEOL = readFileSync(__filename).includes('\r\n') ? '\r\n' : '\n';

testWasiPreview1(['freopen'], {}, { stdout: `hello from input2.txt${checkoutEOL}` });
