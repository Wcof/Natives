import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  extractReasoningStage,
  formatElapsed,
  formatRunElapsed,
  formatReasoningDuration,
  messagePlainText,
  reasoningToggleLabel,
} from './assistant-message-view';

test('formats reasoning elapsed time', () => {
  assert.equal(formatElapsed(0), null);
  assert.equal(formatElapsed(850), '0.8s');
  assert.equal(formatElapsed(11_100), '11.1s');
  assert.equal(formatElapsed(59_900), '59.9s');
  assert.equal(formatElapsed(60_000), '1m00.0s');
  assert.equal(formatElapsed(2_051_100), '34m11.1s');
  assert.equal(formatElapsed(3_599_900), '59m59.9s');
  assert.equal(formatElapsed(3_600_000), '1h00m00.0s');
  assert.equal(formatElapsed(5_651_100), '1h34m11.1s');
  assert.equal(formatRunElapsed(5_651_100, true, 'zh'), '已完成 1h34m11.1s');
  assert.equal(formatRunElapsed(11_100, false, 'zh'), '运行中 11.1s');
  assert.equal(formatReasoningDuration(3200, 'zh'), '3.2 秒');
  assert.equal(formatReasoningDuration(3200, 'en'), '3.2s');
  assert.equal(formatReasoningDuration(0, 'zh'), null);
});

test('timeline footer and thinking duration share the bounded 1Hz clock', () => {
  const timeline = readFileSync(
    resolve(process.cwd(), 'src/components/ui/conversation/ConversationTimeline.tsx'),
    'utf8',
  );
  const thinking = readFileSync(
    resolve(process.cwd(), 'src/components/ui/conversation/ThinkingActivity.tsx'),
    'utf8',
  );
  // One parent clock updates all live durations, keeping pure time counters bounded to 1Hz.
  assert.match(timeline, /setInterval\(\(\) => setNow\(Date\.now\(\)\), 1000\)/);
  assert.equal(thinking.includes('setInterval('), false);
  // The thinking strip receives the live timestamp rather than a frozen duration label.
  assert.equal(/thinkingDurationLabel\?:/.test(thinking), false);
  assert.match(thinking, /thinkingStartedAtMs/);
  assert.match(thinking, /nowMs/);
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
  assert.equal(
    reasoningToggleLabel({ live: false, expanded: false, durationMs: 0, locale: 'zh' }),
    '查看思考过程',
  );
});

test('extracts copyable text from message blocks', () => {
  assert.equal(messagePlainText([{ type: 'reasoning', reasoning: 'hidden' }, { type: 'text', text: 'answer' }]), 'answer');
});
