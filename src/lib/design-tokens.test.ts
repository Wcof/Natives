import { test } from 'node:test';
import assert from 'node:assert';
import { MOTION_EASING, TRANSITION, SPINNER_EASING, LAYOUT } from './design-tokens';

// ── Foundation Motion / Layout Token Contract Tests ──
// R-U13/R-U14: 统一缓动曲线 + 三档时长；spinner 是唯一 linear 例外。
// R-U2.10: 共享结构令牌（阅读宽度、标题栏、紧凑控件、回合间距）。

test('MOTION_EASING uses the single unified curve', () => {
  assert.equal(MOTION_EASING, 'cubic-bezier(0.16, 1, 0.3, 1)');
});

test('TRANSITION tiers all reference MOTION_EASING', () => {
  for (const tier of [TRANSITION.fast, TRANSITION.normal, TRANSITION.slow]) {
    assert.ok(tier.endsWith(MOTION_EASING), `${tier} should end with ${MOTION_EASING}`);
  }
});

test('TRANSITION tiers are 120/200/300ms', () => {
  assert.equal(TRANSITION.fast, `120ms ${MOTION_EASING}`);
  assert.equal(TRANSITION.normal, `200ms ${MOTION_EASING}`);
  assert.equal(TRANSITION.slow, `300ms ${MOTION_EASING}`);
});

test('SPINNER_EASING is the only linear exception (R-U13)', () => {
  assert.equal(SPINNER_EASING, 'linear');
});

test('shared layout structure tokens (R-U2.10)', () => {
  assert.equal(LAYOUT.readingWidth, 760);
  assert.equal(LAYOUT.titlebarHeight, 48);
  assert.equal(LAYOUT.controlCompact, 32);
  assert.equal(LAYOUT.turnGap, 40);
});
