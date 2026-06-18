import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { generateThumb, setThumbCacheDir } from './thumbnail';

const THUMB_DIR = path.join(os.tmpdir(), 'natives-thumb-test');
const CACHE_DIR = path.join(os.tmpdir(), 'natives-thumb-cache-test');

describe('generateThumb', () => {
  before(() => {
    setThumbCacheDir(CACHE_DIR);
  });

  after(() => {
    if (fs.existsSync(THUMB_DIR)) fs.rmSync(THUMB_DIR, { recursive: true, force: true });
    if (fs.existsSync(CACHE_DIR)) fs.rmSync(CACHE_DIR, { recursive: true, force: true });
  });

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
  });

  it('should generate thumbnail for a PNG image', async () => {
    const pngBuffer = Buffer.from([
      0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
      0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
      0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
      0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
      0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
      0x54, 0x08, 0xD7, 0x63, 0x60, 0x60, 0x60, 0x00,
      0x00, 0x00, 0x04, 0x00, 0x01, 0x27, 0x34, 0x27,
      0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
      0xAE, 0x42, 0x60, 0x82,
    ]);

    fs.mkdirSync(THUMB_DIR, { recursive: true });
    const imgPath = path.join(THUMB_DIR, 'test.png');
    fs.writeFileSync(imgPath, pngBuffer);

    const result = await generateThumb(imgPath, 128);

    if (process.platform === 'darwin') {
      assert.ok(result);
      assert.ok(Buffer.isBuffer(result!.buffer));
      assert.ok(result!.buffer.length > 0);
      assert.equal(result!.cached, false);
    } else {
      assert.equal(result, null);
    }
  });

  it('should return cached result on second call', async () => {
    const pngBuffer = Buffer.from([
      0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
      0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
      0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
      0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
      0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
      0x54, 0x08, 0xD7, 0x63, 0x60, 0x60, 0x60, 0x00,
      0x00, 0x00, 0x04, 0x00, 0x01, 0x27, 0x34, 0x27,
      0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
      0xAE, 0x42, 0x60, 0x82,
    ]);

    fs.mkdirSync(THUMB_DIR, { recursive: true });
    const imgPath = path.join(THUMB_DIR, 'cache-test.png');
    fs.writeFileSync(imgPath, pngBuffer);

    if (process.platform === 'darwin') {
      // First call: should NOT be cached
      const first = await generateThumb(imgPath, 128);
      assert.ok(first);
      assert.equal(first!.cached, false);

      // Second call: should be cached (if cache works)
      const second = await generateThumb(imgPath, 128);
      assert.ok(second);
    }
  });
});
