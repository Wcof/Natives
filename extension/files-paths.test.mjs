import assert from 'node:assert/strict';
import { fileUri, normalizePathInput, parentAndName, parseOpenMarkdownUrl, pathParts } from './files-paths.js';

assert.deepEqual(pathParts('/Users/demo/My File'), ['Users', 'demo', 'My File']);
assert.equal(fileUri('/Users/demo/My File'), 'file:///Users/demo/My%20File');
assert.equal(normalizePathInput('"/Users/demo/My File"'), '/Users/demo/My File');
assert.equal(normalizePathInput('/Users/demo/My\\ File'), '/Users/demo/My File');
assert.deepEqual(parentAndName('/Users/demo/file.txt'), { parent: '/Users/demo', name: 'file.txt' });
assert.deepEqual(parentAndName('/file.txt'), { parent: '/', name: 'file.txt' });

// normalizePathInput: file URLs, percent-encoding, Chinese paths, Windows drive letters
assert.equal(normalizePathInput('file:///Users/demo/notes.md'), '/Users/demo/notes.md');
assert.equal(normalizePathInput('file:///Users/demo/My%20Folder/%E4%B8%AD%E6%96%87%E6%96%87%E4%BB%B6.md'), '/Users/demo/My Folder/中文文件.md');
assert.equal(normalizePathInput('file:///Users/demo/中文 目录/文档.markdown'), '/Users/demo/中文 目录/文档.markdown');
assert.equal(normalizePathInput('file:///C:/Users/demo/notes.md'), 'C:/Users/demo/notes.md');
assert.equal(normalizePathInput('file:///c:/Users/demo/notes.md'), 'c:/Users/demo/notes.md');
assert.equal(normalizePathInput('file:///D:/Work/%E6%96%87%E6%A1%A3/read.MDX'), 'D:/Work/文档/read.MDX');
assert.equal(normalizePathInput('file://localhost/Users/demo/notes.md'), '/Users/demo/notes.md');

// parseOpenMarkdownUrl: macOS / Linux, Chinese & spaces, Windows drive letters, case-insensitive extensions
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/notes.md'), '/Users/demo/notes.md');
assert.equal(parseOpenMarkdownUrl('file:///home/user/docs.markdown'), '/home/user/docs.markdown');
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/My%20Folder/%E4%B8%AD%E6%96%87%E6%96%87%E4%BB%B6.md'), '/Users/demo/My Folder/中文文件.md');
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/中文 目录/文档.MD'), '/Users/demo/中文 目录/文档.MD');
assert.equal(parseOpenMarkdownUrl('file:///C:/Users/demo/notes.MD'), 'C:/Users/demo/notes.MD');
assert.equal(parseOpenMarkdownUrl('file:///d:/work/project.mdx'), 'd:/work/project.mdx');
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/README.MARKDOWN'), '/Users/demo/README.MARKDOWN');

// parseOpenMarkdownUrl: invalid URLs, non-file protocols, non-Markdown URLs
assert.equal(parseOpenMarkdownUrl('https://example.com/test.md'), null);
assert.equal(parseOpenMarkdownUrl('javascript:alert(1)'), null);
assert.equal(parseOpenMarkdownUrl('not-a-url'), null);
assert.equal(parseOpenMarkdownUrl('file://'), null);
assert.equal(parseOpenMarkdownUrl(''), null);
assert.equal(parseOpenMarkdownUrl(null), null);
assert.equal(parseOpenMarkdownUrl(undefined), null);
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/document.pdf'), null);
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/image.png'), null);
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/text.txt'), null);
assert.equal(parseOpenMarkdownUrl('file:///Users/demo/data.json'), null);

console.log('files path helpers test passed');
