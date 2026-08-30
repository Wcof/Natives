import assert from 'node:assert/strict';
import { createFilesSession } from './files-session.js';

const session = createFilesSession();
session.navigate('/a', false);
session.navigate('/b');
session.navigate('/c');
assert.equal(session.moveHistory(-1), '/b');
session.entries = ['/b/a', '/b/b', '/b/c'].map((path) => ({ path }));
session.select(0);
session.select(2, { shiftKey: true });
assert.deepEqual([...session.selectedPaths], ['/b/a', '/b/b', '/b/c']);
session.select(1, { metaKey: true });
assert.deepEqual([...session.selectedPaths], ['/b/a', '/b/c']);
session.navigate('/d');
assert.equal(session.selectedPaths.size, 0);
assert.equal(session.pageOffset, 0);
console.log('files session test passed');
