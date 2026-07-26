import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  normalizePath,
  isExternalUrl,
  resolveLocalRef,
  assetUrlToPath,
  rewriteLocalImages,
} from './markdown-local-images';
import { htmlSignature, semanticEqual } from './markdown-semantic';

const toUrl = (abs: string) => `asset://localhost${encodeURI(abs)}`;

describe('normalizePath', () => {
  it('折叠 ./ 与 ../', () => {
    assert.equal(normalizePath('/a/b/./c/../d.png'), '/a/b/d.png');
    assert.equal(normalizePath('/a/../../b'), '/b');
  });
});

describe('resolveLocalRef', () => {
  it('相对路径基于 baseDir 解析', () => {
    assert.equal(resolveLocalRef('./图/封面.png', '/docs'), '/docs/图/封面.png');
    assert.equal(resolveLocalRef('../img/a.png', '/docs/sub'), '/docs/img/a.png');
  });

  it('本地绝对路径与 file:// 保留解析', () => {
    assert.equal(resolveLocalRef('/abs/x.png', '/docs'), '/abs/x.png');
    assert.equal(resolveLocalRef('file:///abs/y.png', '/docs'), '/abs/y.png');
  });

  it('外链/数据/资产 URL 不改写', () => {
    for (const url of ['https://x.com/a.png', 'data:image/png;base64,xx', 'blob:abc', 'asset://localhost/a', '#anchor']) {
      assert.equal(resolveLocalRef(url, '/docs'), null, url);
    }
    assert.equal(isExternalUrl('http://a/b'), true);
  });
});

describe('assetUrlToPath', () => {
  it('还原 convertFileSrc 两种形态', () => {
    assert.equal(assetUrlToPath('asset://localhost/a/%E5%9B%BE.png'), '/a/图.png');
    assert.equal(assetUrlToPath('http://asset.localhost/a/b.png'), '/a/b.png');
    assert.equal(assetUrlToPath('https://x.com/a.png'), null);
  });
});

describe('rewriteLocalImages', () => {
  it('改写 md 图片语法并可精确还原原文', () => {
    const md = '# t\n\n![封面](./图/封面.png "标题")\n\n![](https://x.com/a.png)';
    const { text, restore } = rewriteLocalImages(md, '/docs', toUrl);
    assert.ok(text.includes('asset://localhost/docs/%E5%9B%BE/%E5%B0%81%E9%9D%A2.png'));
    assert.ok(text.includes('https://x.com/a.png')); // 外链不动
    assert.equal(restore(text), md); // 字节级还原
  });

  it('改写内联 <img> 并还原', () => {
    const md = '<img src="../a b.png" width="100">';
    const { text, restore } = rewriteLocalImages(md, '/d/sub', toUrl);
    assert.ok(text.includes('asset://localhost/d/a%20b.png'));
    assert.equal(restore(text), md);
  });

  it('编辑期新增的资产 URL 落盘还原为真实路径', () => {
    const { restore } = rewriteLocalImages('x', '/d', toUrl);
    const edited = '![新图](asset://localhost/tmp/%E6%96%B0.png)';
    assert.equal(restore(edited), '![新图](/tmp/新.png)');
  });
});

describe('markdown-semantic', () => {
  it('htmlSignature 折叠空白并提取结构', () => {
    const a = htmlSignature('<p>hello  <b>world</b></p>');
    const b = htmlSignature('<p>hello <b>world</b> </p>');
    assert.equal(a, b);
    assert.notEqual(a, htmlSignature('<p>hello world</p>'));
  });

  it('img src/alt 与 a href 参与签名', () => {
    assert.notEqual(
      htmlSignature('<img src="/a.png" alt="x">'),
      htmlSignature('<img src="/b.png" alt="x">'),
    );
    assert.notEqual(
      htmlSignature('<a href="/1">t</a>'),
      htmlSignature('<a href="/2">t</a>'),
    );
  });

  it('semanticEqual：等价写法通过，丢内容不通过', async () => {
    assert.equal(await semanticEqual('# 标题\n\n正文', '# 标题\n\n正文\n'), true);
    assert.equal(await semanticEqual('# 标题\n\n正文', '# 标题'), false);
  });
});
