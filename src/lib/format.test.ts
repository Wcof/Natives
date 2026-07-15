import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { fmtCount } from './format';
import { zh } from '@/i18n/zh';

describe('fmtCount', () => {
  it('uses locale-aware compact units', () => {
    assert.equal(fmtCount(20_000, 'zh'), '2万');
    assert.equal(fmtCount(20_000, 'en'), '20K');
  });
});

describe('Chinese usage labels', () => {
  it('does not mix English token terms into the Chinese dashboard', () => {
    assert.equal(zh.usage.totalTokens, '总词元');
    assert.equal(zh.usage.inputTokens, '输入词元');
    assert.equal(zh.usage.outputTokens, '输出词元');
    assert.equal(zh.usage.tokenLabel, '词元');
    assert.equal(zh.usage.tokens, '词元');
  });
});
