import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  type FileEntry,
  type FileKind,
  type ProjectBadge,
  type SearchResult,
  type ContentSearchResult,
  type GitStatus,
  type GitFileStatus,
  type DiskUsageItem,
  type PathCandidate,
  FILE_KINDS,
  PROJECT_BADGES,
  detectFileKind,
  detectProjectBadge,
} from './file';

describe('FileTypes', () => {
  // ── FileKind ──

  it('should have all expected file kinds', () => {
    const expected: FileKind[] = ['text', 'image', 'video', 'audio', 'pdf', 'archive', 'other'];
    assert.deepEqual([...FILE_KINDS].sort(), expected.sort());
  });

  it('should detect text files by extension', () => {
    assert.equal(detectFileKind('readme.md'), 'text');
    assert.equal(detectFileKind('main.ts'), 'text');
    assert.equal(detectFileKind('index.js'), 'text');
    assert.equal(detectFileKind('style.css'), 'text');
    assert.equal(detectFileKind('package.json'), 'text');
    assert.equal(detectFileKind('Dockerfile'), 'text');
    assert.equal(detectFileKind('Makefile'), 'text');
    assert.equal(detectFileKind('requirements.txt'), 'text');
    assert.equal(detectFileKind('CHANGELOG'), 'text');
  });

  it('should detect image files by extension', () => {
    assert.equal(detectFileKind('photo.jpg'), 'image');
    assert.equal(detectFileKind('photo.jpeg'), 'image');
    assert.equal(detectFileKind('photo.png'), 'image');
    assert.equal(detectFileKind('photo.gif'), 'image');
    assert.equal(detectFileKind('photo.webp'), 'image');
    assert.equal(detectFileKind('photo.svg'), 'image');
    assert.equal(detectFileKind('photo.ico'), 'image');
    assert.equal(detectFileKind('photo.bmp'), 'image');
  });

  it('should detect video files by extension', () => {
    assert.equal(detectFileKind('video.mp4'), 'video');
    assert.equal(detectFileKind('video.mov'), 'video');
    assert.equal(detectFileKind('video.avi'), 'video');
    assert.equal(detectFileKind('video.mkv'), 'video');
    assert.equal(detectFileKind('video.webm'), 'video');
  });

  it('should detect audio files by extension', () => {
    assert.equal(detectFileKind('audio.mp3'), 'audio');
    assert.equal(detectFileKind('audio.wav'), 'audio');
    assert.equal(detectFileKind('audio.flac'), 'audio');
    assert.equal(detectFileKind('audio.ogg'), 'audio');
    assert.equal(detectFileKind('audio.m4a'), 'audio');
  });

  it('should detect pdf files by extension', () => {
    assert.equal(detectFileKind('doc.pdf'), 'pdf');
  });

  it('should detect archive files by extension', () => {
    assert.equal(detectFileKind('archive.zip'), 'archive');
    assert.equal(detectFileKind('archive.tar.gz'), 'archive');
    assert.equal(detectFileKind('archive.rar'), 'archive');
    assert.equal(detectFileKind('archive.7z'), 'archive');
    assert.equal(detectFileKind('archive.tar'), 'archive');
    assert.equal(detectFileKind('archive.gz'), 'archive');
    assert.equal(detectFileKind('archive.bz2'), 'archive');
  });

  it('should detect other files by extension', () => {
    assert.equal(detectFileKind('binary.bin'), 'other');
    assert.equal(detectFileKind('database.db'), 'other');
    assert.equal(detectFileKind('unknown.xyz'), 'other');
  });

  // ── ProjectBadge ──

  it('should have all expected project badges', () => {
    const expected: ProjectBadge[] = ['node', 'web', 'python', 'rust', 'go', 'git'];
    assert.deepEqual([...PROJECT_BADGES].sort(), expected.sort());
  });

  it('should detect node project from package.json', () => {
    const files = ['package.json', 'readme.md'];
    assert.equal(detectProjectBadge('/some/project', files), 'node');
  });

  it('should detect web project from index.html', () => {
    const files = ['index.html', 'style.css'];
    assert.equal(detectProjectBadge('/web/project', files), 'web');
  });

  it('should detect python project from requirements.txt', () => {
    const files = ['requirements.txt', 'main.py'];
    assert.equal(detectProjectBadge('/py/project', files), 'python');
  });

  it('should detect rust project from Cargo.toml', () => {
    const files = ['Cargo.toml', 'src/main.rs'];
    assert.equal(detectProjectBadge('/rust/project', files), 'rust');
  });

  it('should detect go project from go.mod', () => {
    const files = ['go.mod', 'main.go'];
    assert.equal(detectProjectBadge('/go/project', files), 'go');
  });

  it('should detect git project from .git dir', () => {
    const files = ['readme.md'];
    assert.equal(detectProjectBadge('/git/project', files, true), 'git');
  });

  it('should prefer node badge over web badge', () => {
    const files = ['package.json', 'index.html'];
    assert.equal(detectProjectBadge('/project', files), 'node');
  });

  it('should return undefined when no badge matches', () => {
    const files = ['random.bin'];
    assert.equal(detectProjectBadge('/empty/project', files, false), undefined);
  });

  // ── Data construction ──

  it('should construct a valid FileEntry', () => {
    const entry: FileEntry = {
      name: 'test.ts',
      path: '/project/src/test.ts',
      isDir: false,
      kind: 'text',
      hidden: false,
      size: 1024,
      mtime: 1700000000000,
      btime: 1690000000000,
    };
    assert.equal(entry.name, 'test.ts');
    assert.equal(entry.kind, 'text');
  });

  it('should construct a FileEntry with optional fields', () => {
    const entry: FileEntry = {
      name: 'my-project',
      path: '/project/my-project',
      isDir: true,
      kind: 'other',
      hidden: false,
      size: 4096,
      mtime: 1700000000000,
      btime: 1690000000000,
      projectBadge: 'node',
      symlink: '/real/path',
    };
    assert.equal(entry.projectBadge, 'node');
    assert.equal(entry.symlink, '/real/path');
  });

  it('should construct a valid SearchResult', () => {
    const result: SearchResult = {
      path: '/project/file.ts',
      name: 'file.ts',
      score: 0.85,
      isDir: false,
      mtime: 1700000000000,
      matchRanges: [[0, 4]],
    };
    assert.equal(result.score, 0.85);
    assert.deepEqual(result.matchRanges, [[0, 4]]);
  });

  it('should construct a valid ContentSearchResult', () => {
    const result: ContentSearchResult = {
      path: '/project/file.ts',
      name: 'file.ts',
      line: 10,
      preview: '  const x = 42;',
      matchStart: 10,
      matchEnd: 12,
    };
    assert.equal(result.line, 10);
    assert.equal(result.preview, '  const x = 42;');
  });

  it('should construct a valid GitStatus', () => {
    const status: GitStatus = {
      root: '/project',
      branch: 'main',
      files: [
        { path: 'src/file.ts', status: 'M' },
        { path: 'new.ts', status: '??' },
      ],
    };
    assert.equal(status.branch, 'main');
    assert.equal(status.files.length, 2);
  });

  it('should construct a GitFileStatus with rename info', () => {
    const file: GitFileStatus = {
      path: 'src/new.ts',
      status: 'R',
      oldPath: 'src/old.ts',
    };
    assert.equal(file.status, 'R');
    assert.equal(file.oldPath, 'src/old.ts');
  });

  it('should construct a valid DiskUsageItem', () => {
    const item: DiskUsageItem = {
      name: 'node_modules',
      path: '/project/node_modules',
      isDir: true,
      size: 1024000,
      sizeFormatted: '1.0 MB',
    };
    assert.equal(item.sizeFormatted, '1.0 MB');
  });

  it('should construct a valid PathCandidate', () => {
    const candidate: PathCandidate = {
      path: '/usr/local/bin',
      exists: true,
    };
    assert.equal(candidate.exists, true);
  });
});
