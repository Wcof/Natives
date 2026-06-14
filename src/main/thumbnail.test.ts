import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { generateThumb } from './thumbnail';

const THUMB_DIR = path.join(os.tmpdir(), 'natives-thumb-test');

describe('generateThumb', () => {
  it('should return null for non-existent file', async () => {
    const result = await generateThumb('/nonexistent/file.jpg', 128);
    assert.equal(result, null);
  });

  it('should return null for non-image non-video non-pdf file', async () => {
    const filePath = path.join(THUMB_DIR, 'test.txt');
    fs.mkdirSync(THUMB_DIR, { recursive: true });
    fs.writeFileSync(filePath, 'hello', 'utf-8');
    const result = await generateThumb(filePath, 128);
    assert.equal(result, null);
    fs.rmSync(THUMB_DIR, { recursive: true, force: true });
  });

  it('should generate thumbnail for a PNG image using sips', async () => {
    // Create a minimal 1x1 pixel PNG
    const pngBuffer = Buffer.from([
      0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
      0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
      0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 pixel
      0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
      0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
      0x54, 0x08, 0xD7, 0x63, 0x60, 0x60, 0x60, 0x00,
      0x00, 0x00, 0x04, 0x00, 0x01, 0x27, 0x34, 0x27,
      0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
      0xAE, 0x42, 0x60, 0x82, // IEND chunk
    ]);

    fs.mkdirSync(THUMB_DIR, { recursive: true });
    const imgPath = path.join(THUMB_DIR, 'test.png');
    fs.writeFileSync(imgPath, pngBuffer);

    const result = await generateThumb(imgPath, 128);

    if (process.platform === 'darwin') {
      assert.ok(result);
      assert.ok(Buffer.isBuffer(result!.buffer));
      assert.ok(result!.buffer.length > 0);
      assert.ok(result!.contentType.startsWith('image/'));
      assert.equal(result!.cached, false);
    } else {
      assert.equal(result, null);
    }

    fs.rmSync(THUMB_DIR, { recursive: true, force: true });
  });
});
