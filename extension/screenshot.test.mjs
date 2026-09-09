import assert from 'node:assert/strict';
import {
  computeCaptureTiles,
  computeCanvasSegments,
  validateCaptureLimits,
  sanitizeFilename,
  MAX_TILES,
  MAX_TOTAL_PIXELS,
  MAX_CANVAS_SIDE,
  MAX_CANVAS_PIXELS,
  MIN_CAPTURE_INTERVAL_MS,
} from './screenshot-engine.js';
import {
  isProtectedUrl,
  validateTabForCapture,
} from './screenshot-coordinator.js';
import { ScreenshotStitcher } from './screenshot-stitcher.js';

console.log('--- Running Screenshot Engine & Coordinator Tests ---');

// 1. Single-viewport page (no scrolling needed)
{
  const result = computeCaptureTiles(1200, 800, 1200, 800);
  assert.equal(result.tiles.length, 1);
  const t0 = result.tiles[0];
  assert.equal(t0.scrollX, 0);
  assert.equal(t0.scrollY, 0);
  assert.equal(t0.clipX, 0);
  assert.equal(t0.clipY, 0);
  assert.equal(t0.clipWidth, 1200);
  assert.equal(t0.clipHeight, 800);
  assert.equal(t0.destX, 0);
  assert.equal(t0.destY, 0);
  console.log('✓ Single viewport tile computation passed');
}

// 2. Vertical scrolling with uneven remainder and edge alignment
{
  const viewportW = 1000;
  const viewportH = 800;
  const docW = 1000;
  const docH = 2000;

  const result = computeCaptureTiles(viewportW, viewportH, docW, docH);
  assert.equal(result.tiles.length, 3);

  // Screen 0: y=0..800
  assert.deepEqual(result.tiles[0], {
    tileIndex: 0,
    scrollX: 0,
    scrollY: 0,
    clipX: 0,
    clipY: 0,
    clipWidth: 1000,
    clipHeight: 800,
    destX: 0,
    destY: 0,
  });

  // Screen 1: y=800..1600
  assert.deepEqual(result.tiles[1], {
    tileIndex: 1,
    scrollX: 0,
    scrollY: 800,
    clipX: 0,
    clipY: 0,
    clipWidth: 1000,
    clipHeight: 800,
    destX: 0,
    destY: 800,
  });

  // Screen 2: remaining 400px (y=1600..2000). Max scroll is 2000-800 = 1200.
  assert.deepEqual(result.tiles[2], {
    tileIndex: 2,
    scrollX: 0,
    scrollY: 1200,
    clipX: 0,
    clipY: 400, // 1600 - 1200 = 400 within viewport [0, 800]
    clipWidth: 1000,
    clipHeight: 400,
    destX: 0,
    destY: 1600,
  });

  // Verify total height coverage and no overlap
  const totalCoveredH = result.tiles.reduce((acc, t) => acc + t.clipHeight, 0);
  assert.equal(totalCoveredH, docH);
  console.log('✓ Vertical scrolling and edge alignment passed');
}

// 3. 2D grid scrolling (horizontal and vertical)
{
  const vW = 500;
  const vH = 500;
  const dW = 1200;
  const dH = 1200;

  const result = computeCaptureTiles(vW, vH, dW, dH);
  // Grid size: ceil(1200/500) = 3 columns x 3 rows = 9 tiles
  assert.equal(result.tiles.length, 9);

  // Check that every destination pixel in [0, 1200) x [0, 1200) is covered exactly once
  const grid = Array.from({ length: 1200 }, () => new Uint8Array(1200));
  for (const t of result.tiles) {
    for (let y = t.destY; y < t.destY + t.clipHeight; y++) {
      for (let x = t.destX; x < t.destX + t.clipWidth; x++) {
        assert.equal(grid[y][x], 0, `Pixel (${x}, ${y}) overlapped!`);
        grid[y][x] = 1;
      }
    }
  }
  for (let y = 0; y < 1200; y++) {
    for (let x = 0; x < 1200; x++) {
      assert.equal(grid[y][x], 1, `Pixel (${x}, ${y}) was missed!`);
    }
  }
  console.log('✓ 2D grid scrolling full coverage without gaps or duplicates passed');
}

// 4. Canvas segmenting when exceeding max limits
{
  // Normal page fits in 1 segment
  const segs1 = computeCanvasSegments(1920, 10000, MAX_CANVAS_SIDE, MAX_CANVAS_PIXELS);
  assert.equal(segs1.length, 1);
  assert.equal(segs1[0].width, 1920);
  assert.equal(segs1[0].height, 10000);

  // Very tall page (e.g. 40,000 px height) exceeds 16,384 px limit
  const segs2 = computeCanvasSegments(2000, 40000, MAX_CANVAS_SIDE, MAX_CANVAS_PIXELS);
  assert.ok(segs2.length > 1);
  let accumulatedY = 0;
  segs2.forEach((seg, idx) => {
    assert.equal(seg.index, idx + 1);
    assert.equal(seg.y, accumulatedY);
    assert.ok(seg.height <= MAX_CANVAS_SIDE);
    assert.ok(seg.width * seg.height <= MAX_CANVAS_PIXELS);
    accumulatedY += seg.height;
  });
  assert.equal(accumulatedY, 40000);
  console.log('✓ Canvas chunking and segmenting passed');
}

// 5. Filename sanitization
{
  const fixedDate = new Date(2026, 8, 9, 14, 30, 45); // Sep 9, 2026 14:30:45
  const filenameSingle = sanitizeFilename('Test Page / with <illegal> chars!', null, 1, fixedDate);
  assert.equal(filenameSingle, 'Test_Page_with_illegal_chars-2026-09-09_14-30-45.png');

  const filenameMulti = sanitizeFilename('Report', 2, 3, fixedDate);
  assert.equal(filenameMulti, 'Report-2026-09-09_14-30-45-part2.png');

  const filenameEmpty = sanitizeFilename('', null, 1, fixedDate);
  assert.equal(filenameEmpty, 'screenshot-2026-09-09_14-30-45.png');
  console.log('✓ Filename sanitization passed');
}

// 6. Capture limits validation
{
  assert.equal(validateCaptureLimits(128, 1000, 1000).ok, true);
  assert.equal(validateCaptureLimits(129, 1000, 1000).ok, false);
  assert.equal(validateCaptureLimits(129, 1000, 1000).error, 'CANVAS_LIMIT_EXCEEDED');

  // Total pixels > 128M pixels (128 * 1024 * 1024 = 134,217,728)
  const okPixels = validateCaptureLimits(10, 8192, 16384); // 134,217,728 exactly
  assert.equal(okPixels.ok, true);

  const overflowPixels = validateCaptureLimits(10, 8193, 16384); // 134,234,112 > 128M
  assert.equal(overflowPixels.ok, false);
  assert.equal(overflowPixels.error, 'CANVAS_LIMIT_EXCEEDED');
  console.log('✓ Capture limits (128 tiles / 128M pixels) validation passed');
}

// 7. Coordinator quota rate-limiting constraint
{
  assert.ok(MIN_CAPTURE_INTERVAL_MS >= 550, 'Minimum capture interval must be at least 550ms (max 2 calls/sec quota)');
  console.log('✓ Quota rate limit constant verified');
}

// 8. Protected URLs check
{
  assert.equal(isProtectedUrl('chrome://extensions'), true);
  assert.equal(isProtectedUrl('chrome-extension://abcdefg/popup.html'), true);
  assert.equal(isProtectedUrl('devtools://devtools/bundled/devtools_app.html'), true);
  assert.equal(isProtectedUrl('https://chromewebstore.google.com/detail/something'), true);
  assert.equal(isProtectedUrl('https://chrome.google.com/webstore/category/extensions'), true);
  assert.equal(isProtectedUrl('https://example.com/blog/article'), false);
  assert.equal(isProtectedUrl('http://localhost:3000'), false);
  assert.equal(isProtectedUrl('file:///Users/doc/index.html'), false);
  console.log('✓ Protected URL rules passed');
}

// 9. Tab validation logic
{
  globalThis.chrome = {
    extension: {
      isAllowedFileSchemeAccess: async () => false,
    },
  };

  const protRes = await validateTabForCapture({ id: 1, url: 'chrome://settings' });
  assert.equal(protRes.ok, false);
  assert.equal(protRes.error, 'captureErrorProtected');

  const fileNoAccess = await validateTabForCapture({ id: 2, url: 'file:///test.html' });
  assert.equal(fileNoAccess.ok, false);
  assert.equal(fileNoAccess.error, 'captureErrorFileAccess');

  globalThis.chrome.extension.isAllowedFileSchemeAccess = async () => true;
  const fileWithAccess = await validateTabForCapture({ id: 3, url: 'file:///test.html' });
  assert.equal(fileWithAccess.ok, true);

  const webRes = await validateTabForCapture({ id: 4, url: 'https://example.com' });
  assert.equal(webRes.ok, true);
  console.log('✓ Tab validation rules passed');
}

// 10. Screenshot Stitcher contract test with Mock Canvas
{
  class MockCanvas {
    constructor() {
      this.width = 0;
      this.height = 0;
      this.drawCalls = [];
    }
    getContext() {
      return {
        drawImage: (...args) => {
          this.drawCalls.push(args);
        },
      };
    }
    toBlob(callback, type) {
      callback({ type, size: 1024, mock: true });
    }
  }

  globalThis.document = {
    createElement: (tag) => {
      if (tag === 'canvas') return new MockCanvas();
      return {};
    },
  };

  const createdUrls = [];
  globalThis.URL = {
    createObjectURL: (blob) => {
      const url = `blob:test-${createdUrls.length + 1}`;
      createdUrls.push(url);
      return url;
    },
  };

  const metadata = {
    title: 'Test Stitch',
    totalWidth: 1000,
    totalHeight: 1500,
    viewportWidth: 1000,
    viewportHeight: 1000,
    dpr: 2,
    totalTiles: 2,
  };

  const stitcher = new ScreenshotStitcher(metadata);
  const mockImg = { naturalWidth: 2000, width: 2000, height: 2000 };

  // Tile 0: dest (0, 0), clip (0, 0, 1000, 1000)
  await stitcher.addTile({
    destX: 0,
    destY: 0,
    clipX: 0,
    clipY: 0,
    clipWidth: 1000,
    clipHeight: 1000,
  }, mockImg);

  // Tile 1: dest (0, 1000), clip (0, 500, 1000, 500)
  await stitcher.addTile({
    destX: 0,
    destY: 1000,
    clipX: 0,
    clipY: 500,
    clipWidth: 1000,
    clipHeight: 500,
  }, mockImg);

  const results = await stitcher.finalize();
  assert.equal(results.length, 1);
  assert.equal(results[0].width, 2000);
  assert.equal(results[0].height, 3000);
  assert.ok(results[0].blobUrl.startsWith('blob:'));

  const canvas = stitcher.segments[0].canvas;
  assert.equal(canvas.drawCalls.length, 2);

  // Check tile 0 draw call: (img, sx, sy, sw, sh, dx, dy, dw, dh)
  const call0 = canvas.drawCalls[0];
  assert.equal(call0[1], 0); // sourceX
  assert.equal(call0[2], 0); // sourceY
  assert.equal(call0[3], 2000); // tileDestW
  assert.equal(call0[4], 2000); // drawH
  assert.equal(call0[5], 0); // destX
  assert.equal(call0[6], 0); // destY

  // Check tile 1 draw call:
  const call1 = canvas.drawCalls[1];
  assert.equal(call1[1], 0); // sourceX
  assert.equal(call1[2], 1000); // sourceY: clipY (500) * scale (2) = 1000
  assert.equal(call1[3], 2000); // tileDestW
  assert.equal(call1[4], 1000); // drawH: 500 * 2 = 1000
  assert.equal(call1[5], 0); // destX
  assert.equal(call1[6], 2000); // destY: 1000 * 2 = 2000
  console.log('✓ Screenshot stitcher tiling and DPR scale contract passed');
}

// 11. Concurrency lock and event handling in coordinator
{
  let createdTabs = [];
  globalThis.chrome = {
    ...globalThis.chrome,
    tabs: {
      query: async ({ active }) => {
        if (active) return [{ id: 10, windowId: 1, url: 'https://example.com' }];
        return [];
      },
      create: async (opts) => {
        const tab = { id: 99, ...opts };
        createdTabs.push(tab);
        return tab;
      },
      update: async (tabId, opts) => ({ id: tabId, ...opts }),
      captureVisibleTab: async () => 'data:image/png;base64,mock',
    },
    runtime: {
      getURL: (path) => `chrome-extension://mock/${path}`,
      onConnect: {
        addListener: () => {},
        removeListener: () => {},
      },
    },
    commands: {
      onCommand: { addListener: () => {} },
    },
    contextMenus: {
      create: () => {},
      onClicked: { addListener: () => {} },
    },
  };

  // Verify protected page open creates error tab immediately
  const protTab = { id: 100, url: 'chrome://extensions' };
  createdTabs = [];
  const { handleCaptureTrigger } = await import('./screenshot-coordinator.js');
  await handleCaptureTrigger(protTab);
  assert.equal(createdTabs.length, 1);
  assert.ok(createdTabs[0].url.includes('error=captureErrorProtected'));
  console.log('✓ Coordinator protected page rejection opens error preview tab');
}

console.log('All Screenshot Engine & Coordinator tests passed successfully!');
