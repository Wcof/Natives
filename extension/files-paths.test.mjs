import assert from 'node:assert/strict';
import { fileUri, normalizePathInput, parentAndName, pathParts } from './files-paths.js';

assert.deepEqual(pathParts('/Users/demo/My File'), ['Users', 'demo', 'My File']);
assert.equal(fileUri('/Users/demo/My File'), 'file:///Users/demo/My%20File');
assert.equal(normalizePathInput('"/Users/demo/My File"'), '/Users/demo/My File');
assert.equal(normalizePathInput('/Users/demo/My\\ File'), '/Users/demo/My File');
assert.deepEqual(parentAndName('/Users/demo/file.txt'), { parent: '/Users/demo', name: 'file.txt' });
assert.deepEqual(parentAndName('/file.txt'), { parent: '/', name: 'file.txt' });

console.log('files path helpers test passed');
