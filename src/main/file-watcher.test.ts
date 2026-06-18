import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { shouldIgnoreEvent, getFilePriority, type FileChangeEvent } from './file-watcher';

function makeEvent(path: string, type: FileChangeEvent['type']): FileChangeEvent {
  return { path, type, timestamp: Date.now() };
}

describe('FileWatcher', () => {
  describe('shouldIgnoreEvent', () => {
    it('should ignore hidden files starting with dot', () => {
      assert.equal(shouldIgnoreEvent(makeEvent('/project/.env', 'modify'), {}), true);
      assert.equal(shouldIgnoreEvent(makeEvent('/project/.git/config', 'modify'), {}), true);
    });

    it('should not ignore regular files', () => {
      assert.equal(shouldIgnoreEvent(makeEvent('/project/src/main.ts', 'modify'), {}), false);
    });

    it('should ignore SQLite sidecar files', () => {
      assert.equal(shouldIgnoreEvent(makeEvent('/project/data.db-journal', 'modify'), {}), true);
      assert.equal(shouldIgnoreEvent(makeEvent('/project/data.db-wal', 'modify'), {}), true);
      assert.equal(shouldIgnoreEvent(makeEvent('/project/data.db-shm', 'modify'), {}), true);
    });

    it('should ignore node_modules and .git paths', () => {
      assert.equal(shouldIgnoreEvent(makeEvent('/project/node_modules/pkg/index.js', 'modify'), {}), true);
      assert.equal(shouldIgnoreEvent(makeEvent('/project/.git/HEAD', 'modify'), {}), true);
    });

    it('should respect 3-second suppression window for active files', () => {
      const context = {
        activeFilePath: '/project/src/main.ts',
        lastUserAccessTime: Date.now() - 1000, // 1 second ago
      };
      assert.equal(shouldIgnoreEvent(makeEvent('/project/src/main.ts', 'modify'), context), true);
    });

    it('should not suppress after 3-second window', () => {
      const context = {
        activeFilePath: '/project/src/main.ts',
        lastUserAccessTime: Date.now() - 5000, // 5 seconds ago
      };
      assert.equal(shouldIgnoreEvent(makeEvent('/project/src/main.ts', 'modify'), context), false);
    });

    it('should not suppress when no active file', () => {
      assert.equal(shouldIgnoreEvent(makeEvent('/project/src/main.ts', 'modify'), {}), false);
    });
  });

  describe('getFilePriority', () => {
    it('should return 1 for HTML/MD files (highest)', () => {
      assert.equal(getFilePriority('index.html'), 1);
      assert.equal(getFilePriority('readme.md'), 1);
      assert.equal(getFilePriority('doc.mdx'), 1);
    });

    it('should return 2 for code files', () => {
      assert.equal(getFilePriority('main.ts'), 2);
      assert.equal(getFilePriority('app.tsx'), 2);
      assert.equal(getFilePriority('server.py'), 2);
      assert.equal(getFilePriority('main.rs'), 2);
    });

    it('should return 3 for other files', () => {
      assert.equal(getFilePriority('image.png'), 3);
      assert.equal(getFilePriority('data.json'), 3);
      assert.equal(getFilePriority('style.css'), 3);
    });
  });
});
