// model_collect_status 响应校验回归测试（validateCollectStatusResponse）：
// 门禁必须严格区分采集中/采集结束，且对错误响应、字段缺失、类型错误
// fail closed（抛错），绝不静默当作 collecting=false 放行（R-P12）。
import test from 'node:test';
import assert from 'node:assert/strict';
import { validateCollectStatusResponse } from './cpu-probe.mjs';

test('collecting=true response is accepted as collecting phase', () => {
  const { collecting } = validateCollectStatusResponse({ ok: true, result: { collecting: true } });
  assert.equal(collecting, true);
});

test('collecting=false response is accepted as quiescent', () => {
  const { collecting } = validateCollectStatusResponse({ ok: true, result: { collecting: false } });
  assert.equal(collecting, false);
});

test('non-ok response fails closed', () => {
  assert.throws(
    () => validateCollectStatusResponse({ ok: false, error: 'boom' }),
    /non-ok response/,
  );
});

test('null/undefined response fails closed', () => {
  assert.throws(() => validateCollectStatusResponse(null), /non-ok response/);
  assert.throws(() => validateCollectStatusResponse(undefined), /non-ok response/);
});

test('missing collecting field fails closed', () => {
  assert.throws(
    () => validateCollectStatusResponse({ ok: true, result: {} }),
    /missing\/invalid collecting/,
  );
});

test('non-boolean collecting field fails closed', () => {
  assert.throws(
    () => validateCollectStatusResponse({ ok: true, result: { collecting: 'false' } }),
    /missing\/invalid collecting/,
  );
  assert.throws(
    () => validateCollectStatusResponse({ ok: true, result: { collecting: 1 } }),
    /missing\/invalid collecting/,
  );
});
