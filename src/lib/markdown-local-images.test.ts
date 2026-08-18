import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  normalizePath,
  isExternalUrl,
  resolveLocalRef,
  assetUrlToPath,
  rewriteAuthorizedLocalImages,
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

  it('精确还原含中文与空格的 angle-bracket 路径', () => {
    const md = '![封面](<./素材/夏 日.png> "标题")';
    const { text, restore } = rewriteLocalImages(md, '/项目/文档', toUrl);
    assert.ok(text.includes('asset://localhost/%E9%A1%B9%E7%9B%AE/%E6%96%87%E6%A1%A3/%E7%B4%A0%E6%9D%90/%E5%A4%8F%20%E6%97%A5.png'));
    assert.equal(restore(text), md);
  });
});

describe('rewriteAuthorizedLocalImages', () => {
  const isWithinBase = (path: string, baseDir: string) =>
    path === baseDir || path.startsWith(`${baseDir.replace(/\/+$/, '')}/`);

  it('逐资源授权相对图片后才生成 URL，并保持可逆保存', async () => {
    const calls: string[] = [];
    const md = '![封面](<./素材/夏 日.png>)';
    const rewrite = await rewriteAuthorizedLocalImages(md, '/项目/文档', {
      authorizeFile: async (path) => {
        calls.push(`authorize:${path}`);
        return { path, name: '夏 日.png', kind: 'image', size: 1, mtime: 1 };
      },
      toAssetUrl: (file) => {
        calls.push(`asset:${file.path}`);
        return toUrl(file.path);
      },
      isWithinBase,
    });

    assert.deepEqual(calls, [
      'authorize:/项目/文档/素材/夏 日.png',
      'asset:/项目/文档/素材/夏 日.png',
    ]);
    assert.ok(rewrite.text.includes('asset://localhost/%E9%A1%B9%E7%9B%AE/%E6%96%87%E6%A1%A3/%E7%B4%A0%E6%9D%90/%E5%A4%8F%20%E6%97%A5.png'));
    assert.equal(rewrite.restore(rewrite.text), md);
  });

  it('拒绝绝对/越 base 引用且不调用授权或生成 URL', async () => {
    const calls: string[] = [];
    const md = '![](/etc/passwd) ![](../../secret.png)';
    const rewrite = await rewriteAuthorizedLocalImages(md, '/docs/project', {
      authorizeFile: async (path) => {
        calls.push(`authorize:${path}`);
        return { path, name: 'x', kind: 'image', size: 1, mtime: 1 };
      },
      toAssetUrl: (file) => {
        calls.push(`asset:${file.path}`);
        return toUrl(file.path);
      },
      isWithinBase,
    });

    assert.deepEqual(calls, []);
    assert.equal(rewrite.text, md);
  });

  it('缺失或拒绝授权时保留原引用且不生成 URL', async () => {
    let assetCalls = 0;
    const md = '![](./missing.png) ![](./denied.png)';
    const rewrite = await rewriteAuthorizedLocalImages(md, '/docs', {
      authorizeFile: async () => { throw new Error('denied'); },
      toAssetUrl: () => {
        assetCalls++;
        return 'asset://localhost/forbidden';
      },
      isWithinBase,
    });

    assert.equal(assetCalls, 0);
    assert.equal(rewrite.text, md);
  });

  it('快速替换 source 后丢弃旧授权结果且不生成旧 URL', async () => {
    const controller = new AbortController();
    let resolveAuthorization!: () => void;
    const authorization = new Promise<void>((resolve) => { resolveAuthorization = resolve; });
    let assetCalls = 0;
    const pending = rewriteAuthorizedLocalImages('![](./old.png)', '/docs', {
      authorizeFile: async (path) => {
        await authorization;
        return { path, name: 'old.png', kind: 'image', size: 1, mtime: 1 };
      },
      toAssetUrl: () => {
        assetCalls++;
        return 'asset://localhost/docs/old.png';
      },
      isWithinBase,
      signal: controller.signal,
    });

    controller.abort();
    resolveAuthorization();
    await assert.rejects(pending, { name: 'AbortError' });
    assert.equal(assetCalls, 0);
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
