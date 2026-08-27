import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { checkWebUrl, deriveOriginFromUrl } from './apps-web-url';

describe('apps-web-url (APPV2-T01 前端规范化回归)', () => {
  it('无 scheme 输入补 https 并标记 changed', () => {
    const r = checkWebUrl('chatgpt.com');
    assert.equal(r.ok, true);
    assert.equal(r.normalized, 'https://chatgpt.com');
    assert.equal(r.changed, true);

    const b = checkWebUrl('alidocs.dingtalk.com/i/spaces/demo');
    assert.equal(b.normalized, 'https://alidocs.dingtalk.com/i/spaces/demo');
  });

  it('已带 scheme 的合法输入原样通过', () => {
    const r = checkWebUrl('https://github.com');
    assert.equal(r.ok, true);
    assert.equal(r.normalized, 'https://github.com');
    assert.equal(r.changed, false);
  });

  it('公网 http 拒绝（要求 https），loopback http 允许', () => {
    assert.equal(checkWebUrl('http://example.com').reason, 'public-http');
    assert.equal(checkWebUrl('http://127.0.0.1:8080').ok, true);
    assert.equal(checkWebUrl('http://localhost:5173').ok, true);
  });

  it('非法 scheme / 空输入 / 无点 host 拒绝', () => {
    assert.equal(checkWebUrl('ftp://x.com').reason, 'scheme');
    assert.equal(checkWebUrl('').reason, 'empty');
    assert.equal(checkWebUrl('   ').reason, 'empty');
    // 无 scheme 的 "notaurl" 补 https 后 host 无点 → 拒绝（与 Rust 侧一致）。
    assert.equal(checkWebUrl('notaurl').reason, 'invalid-host');
    assert.equal(checkWebUrl('baidu.').reason, 'invalid-host');
    assert.equal(checkWebUrl('https://').reason, 'invalid-host');
  });

  it('deriveOriginFromUrl 忽略 scheme/port/path/userinfo', () => {
    assert.equal(deriveOriginFromUrl('https://user:pass@GitHub.com:8443/x?q=1'), 'github.com');
    assert.equal(deriveOriginFromUrl('http://127.0.0.1:8080/'), '127.0.0.1');
    assert.equal(deriveOriginFromUrl('https://'), null);
  });
});
