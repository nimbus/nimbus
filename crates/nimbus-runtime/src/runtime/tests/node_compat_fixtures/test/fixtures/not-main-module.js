const assert = require('assert');
assert.notStrictEqual(module, require.main);
assert.notStrictEqual(module, process.mainModule);
