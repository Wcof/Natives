import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { detectScreenshotFile, formatDetectedName } from './screenshot';

describe('Screenshot', () => {
  it('should detect screenshot from common patterns', () => {
    assert.ok(detectScreenshotFile('Screenshot 2024-01-01 at 10.30.00.png'));
    assert.ok(detectScreenshotFile('Screen Shot 2024-01-01 at 10.30.00.png'));
    assert.ok(detectScreenshotFile('图片 2024-01-01 10.30.00.png'));
    assert.ok(detectScreenshotFile('截图 2024-01-01 10.30.00.png'));
  });

  it('should reject non-screenshot files', () => {
    assert.equal(detectScreenshotFile('document.pdf'), false);
    assert.equal(detectScreenshotFile('photo.jpg'), false);
  });

  it('should format readable names from detected files', () => {
    const name = formatDetectedName('Screenshot 2024-06-15 at 14.30.00.png');
    assert.ok(name.includes('2024-06-15'));
    assert.ok(name.includes('screenshot'));
  });
});
