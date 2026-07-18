import assert from 'node:assert/strict';
import test from 'node:test';
import {
  extractReasoningStage,
  formatElapsed,
  formatReasoningDuration,
  messagePlainText,
  reasoningToggleLabel,
} from './assistant-message-view';

test('formats reasoning elapsed time', () => {
  assert.equal(formatElapsed(850), '0.9s');
  assert.equal(formatElapsed(65_000), '1m 5s');
  assert.equal(formatReasoningDuration(3200, 'zh'), '3.2 秒');
  assert.equal(formatReasoningDuration(3200, 'en'), '3.2s');
});

test('extracts a stage title from reasoning headings and steps', () => {
  assert.equal(extractReasoningStage('## 分析需求\n先看一下输入'), '分析需求');
  assert.equal(extractReasoningStage('Step 1: Parse input\nStep 2: Build plan'), 'Build plan');
  assert.equal(extractReasoningStage('**规划方案**\n细节说明'), '规划方案');
});

test('reasoning toggle label is dynamic while live and summary when done', () => {
  assert.equal(
    reasoningToggleLabel({ live: true, reasoning: '## 梳理依赖\n继续', durationMs: 1500, locale: 'zh' }),
    '梳理依赖 · 1.5 秒',
  );
  assert.equal(
    reasoningToggleLabel({ live: true, reasoning: '', durationMs: 100, locale: 'zh' }),
    '正在思考 · 0.1 秒',
  );
  assert.equal(
    reasoningToggleLabel({ live: false, expanded: false, durationMs: 4200, locale: 'zh' }),
    '思考了 4.2 秒',
  );
  assert.equal(
    reasoningToggleLabel({ live: false, expanded: true, durationMs: 4200, locale: 'zh' }),
    '隐藏思考过程',
  );
  assert.equal(
    reasoningToggleLabel({ live: false, expanded: false, durationMs: 4200, locale: 'en' }),
    'Thought for 4.2s',
  );
});

test('extracts copyable text from message blocks', () => {
  assert.equal(messagePlainText([{ type: 'reasoning', reasoning: 'hidden' }, { type: 'text', text: 'answer' }]), 'answer');
});
