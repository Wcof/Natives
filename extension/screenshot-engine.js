export const MAX_TILES = 128;
export const MAX_TOTAL_PIXELS = 128 * 1024 * 1024; // 128M pixels
export const MAX_CANVAS_SIDE = 16384;
export const MAX_CANVAS_PIXELS = 64 * 1024 * 1024; // 64M pixels
export const MIN_CAPTURE_INTERVAL_MS = 550;

/**
 * Computes non-overlapping capture tiles to cover the entire scrollable document.
 * Each tile specifies:
 * - scrollX, scrollY: window scroll position
 * - clipX, clipY, clipWidth, clipHeight: sub-rectangle to crop from viewport
 * - destX, destY: position on the combined document canvas
 */
export function computeCaptureTiles(viewportWidth, viewportHeight, docWidth, docHeight) {
  const vW = Math.max(1, Math.floor(viewportWidth));
  const vH = Math.max(1, Math.floor(viewportHeight));
  const dW = Math.max(vW, Math.floor(docWidth));
  const dH = Math.max(vH, Math.floor(docHeight));

  const maxScrollX = Math.max(0, dW - vW);
  const maxScrollY = Math.max(0, dH - vH);

  const tiles = [];
  let tileIndex = 0;

  for (let destY = 0; destY < dH; destY += vH) {
    const clipHeight = Math.min(vH, dH - destY);
    const scrollY = Math.min(destY, maxScrollY);
    const clipY = destY - scrollY;

    for (let destX = 0; destX < dW; destX += vW) {
      const clipWidth = Math.min(vW, dW - destX);
      const scrollX = Math.min(destX, maxScrollX);
      const clipX = destX - scrollX;

      tiles.push({
        tileIndex,
        scrollX,
        scrollY,
        clipX,
        clipY,
        clipWidth,
        clipHeight,
        destX,
        destY,
      });
      tileIndex++;
    }
  }

  return {
    viewportWidth: vW,
    viewportHeight: vH,
    docWidth: dW,
    docHeight: dH,
    tiles,
  };
}

/**
 * Computes vertical canvas segments when stitched image exceeds single canvas limits.
 */
export function computeCanvasSegments(pixelWidth, pixelHeight, maxSide = MAX_CANVAS_SIDE, maxPixels = MAX_CANVAS_PIXELS) {
  const pW = Math.max(1, Math.round(pixelWidth));
  const pH = Math.max(1, Math.round(pixelHeight));

  const maxSegmentHeight = Math.min(maxSide, Math.max(1, Math.floor(maxPixels / pW)));
  const segments = [];
  let y = 0;
  let index = 1;

  while (y < pH) {
    const h = Math.min(maxSegmentHeight, pH - y);
    segments.push({
      index,
      x: 0,
      y,
      width: pW,
      height: h,
    });
    y += h;
    index++;
  }

  return segments;
}

/**
 * Validates whether the document scale and tile count fall within safe memory bounds.
 */
export function validateCaptureLimits(tileCount, pixelWidth, pixelHeight) {
  if (tileCount > MAX_TILES) {
    return { ok: false, error: 'CANVAS_LIMIT_EXCEEDED' };
  }
  const totalPixels = Math.round(pixelWidth) * Math.round(pixelHeight);
  if (totalPixels > MAX_TOTAL_PIXELS) {
    return { ok: false, error: 'CANVAS_LIMIT_EXCEEDED' };
  }
  return { ok: true };
}

/**
 * Formats a clean filename for saving PNG screenshots.
 */
export function sanitizeFilename(title, segmentIndex = null, totalSegments = 1, date = new Date()) {
  const safeTitle = (title || 'screenshot')
    .replace(/[^\p{L}\p{N}_-]+/gu, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 60) || 'screenshot';

  const pad = (n) => String(n).padStart(2, '0');
  const timestamp = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}_${pad(date.getHours())}-${pad(date.getMinutes())}-${pad(date.getSeconds())}`;

  if (totalSegments > 1 && segmentIndex !== null) {
    return `${safeTitle}-${timestamp}-part${segmentIndex}.png`;
  }
  return `${safeTitle}-${timestamp}.png`;
}
