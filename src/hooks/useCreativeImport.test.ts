// 问题13：导入文件按类型路由 + HTML 预填（纯函数，node:test）。
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { htmlImportInitial, routeImportFile } from './useCreativeImport';

describe('routeImportFile（问题13：ZIP/HTML/其他路由）', () => {
  it('zip 走 Workshop 模块安装', () => {
    assert.equal(routeImportFile('/tmp/app.zip'), 'zip');
    assert.equal(routeImportFile('C:\\Users\\me\\app.ZIP'), 'zip');
  });

  it('html/htm 走 local_project 静态运行', () => {
    assert.equal(routeImportFile('/Users/me/site/index.html'), 'html');
    assert.equal(routeImportFile('/Users/me/site/index.htm'), 'html');
    assert.equal(routeImportFile('C:\\site\\index.HTML'), 'html');
  });

  it('其他类型不可导入', () => {
    assert.equal(routeImportFile('/tmp/app.js'), 'other');
    assert.equal(routeImportFile('/tmp/app.png'), 'other');
    assert.equal(routeImportFile('/tmp/noext'), 'other');
  });
});

describe('htmlImportInitial（HTML 导入预填 root/entryFile/title）', () => {
  it('提取父目录 + 相对文件名 + 标题（posix）', () => {
    const initial = htmlImportInitial('/Users/me/site/index.html');
    assert.equal(initial.root, '/Users/me/site');
    assert.equal(initial.entryFile, 'index.html');
    assert.equal(initial.title, 'index.html');
  });

  it('Windows 反斜杠路径归一化', () => {
    const initial = htmlImportInitial('C:\\Users\\me\\site\\app.htm');
    assert.equal(initial.root, 'C:/Users/me/site');
    assert.equal(initial.entryFile, 'app.htm');
    assert.equal(initial.title, 'app.htm');
  });
});
