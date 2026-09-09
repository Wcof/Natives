import {
  computeCanvasSegments,
  MAX_CANVAS_SIDE,
  MAX_CANVAS_PIXELS,
} from './screenshot-engine.js';

export class ScreenshotStitcher {
  constructor(metadata, options = {}) {
    this.metadata = metadata;
    this.maxSide = options.maxSide || MAX_CANVAS_SIDE;
    this.maxPixels = options.maxPixels || MAX_CANVAS_PIXELS;
    this.scale = metadata.dpr || 1;
    this.scaleDetermined = false;

    this.fullPixelWidth = Math.max(1, Math.round(metadata.totalWidth * this.scale));
    this.fullPixelHeight = Math.max(1, Math.round(metadata.totalHeight * this.scale));

    this.segments = [];
    this.isInitialized = false;
  }

  _initSegments(imgWidth) {
    if (this.isInitialized) return;
    if (imgWidth && this.metadata.viewportWidth) {
      this.scale = imgWidth / this.metadata.viewportWidth;
      this.fullPixelWidth = Math.max(1, Math.round(this.metadata.totalWidth * this.scale));
      this.fullPixelHeight = Math.max(1, Math.round(this.metadata.totalHeight * this.scale));
    }

    const segDefs = computeCanvasSegments(
      this.fullPixelWidth,
      this.fullPixelHeight,
      this.maxSide,
      this.maxPixels
    );

    this.segments = segDefs.map((def) => {
      const canvas = document.createElement('canvas');
      canvas.width = def.width;
      canvas.height = def.height;
      const ctx = canvas.getContext('2d');
      return {
        ...def,
        canvas,
        ctx,
      };
    });

    this.isInitialized = true;
  }

  async addTile(tile, img) {
    if (!this.isInitialized) {
      this._initSegments(img.naturalWidth || img.width);
    }

    const scale = this.scale;
    const tileDestX = Math.round(tile.destX * scale);
    const tileDestY = Math.round(tile.destY * scale);
    const tileDestW = Math.round(tile.clipWidth * scale);
    const tileDestH = Math.round(tile.clipHeight * scale);

    for (const segment of this.segments) {
      const intY0 = Math.max(tileDestY, segment.y);
      const intY1 = Math.min(tileDestY + tileDestH, segment.y + segment.height);

      if (intY1 > intY0) {
        const drawH = intY1 - intY0;
        const sourceOffsetY = intY0 - tileDestY;

        const sourceX = Math.round(tile.clipX * scale);
        const sourceY = Math.round(tile.clipY * scale) + sourceOffsetY;

        const destXInCanvas = tileDestX - segment.x;
        const destYInCanvas = intY0 - segment.y;

        segment.ctx.drawImage(
          img,
          sourceX,
          sourceY,
          tileDestW,
          drawH,
          destXInCanvas,
          destYInCanvas,
          tileDestW,
          drawH
        );
      }
    }
  }

  async finalize() {
    if (!this.isInitialized) {
      this._initSegments();
    }

    const results = [];
    for (const segment of this.segments) {
      const blob = await new Promise((resolve) => {
        segment.canvas.toBlob((b) => resolve(b), 'image/png');
      });
      const blobUrl = URL.createObjectURL(blob);
      results.push({
        index: segment.index,
        total: this.segments.length,
        blob,
        blobUrl,
        width: segment.width,
        height: segment.height,
      });
    }

    return results;
  }
}
