import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { validateClipboardData } from './clipboard';

describe('Clipboard', () => {
  it('should accept valid text data', () => {
    assert.equal(validateClipboardData('hello world'), true);
  });

  it('should reject empty data', () => {
    assert.equal(validateClipboardData(''), false);
    assert.equal(validateClipboardData(null), false);
  });

  it('should accept valid image data (base64)', () => {
    assert.equal(validateClipboardData('data:image/png;base64,iVBORw0KGgo='), true);
    assert.equal(validateClipboardData('data:image/jpeg;base64,/9j/4AAQ=='), true);
  });
});
