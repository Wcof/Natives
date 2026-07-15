import assert from 'node:assert/strict';
import test from 'node:test';
import { formatElapsed, messagePlainText } from './assistant-message-view';

test('formats reasoning elapsed time', () => {
  assert.equal(formatElapsed(850), '0.9s');
  assert.equal(formatElapsed(65_000), '1m 5s');
});

test('extracts copyable text from message blocks', () => {
  assert.equal(messagePlainText([{ type: 'reasoning', reasoning: 'hidden' }, { type: 'text', text: 'answer' }]), 'answer');
});
