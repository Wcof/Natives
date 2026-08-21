/**
 * Free Canvas camera helpers (pan / zoom / coordinate transforms).
 */

import { CANVAS_MAX_ZOOM, CANVAS_MIN_ZOOM, CANVAS_ZOOM_STEP, type CanvasCamera } from './types';
import { clamp } from './geometry';

export interface Point {
  x: number;
  y: number;
}

/** World → screen (view coordinates). */
export function worldToScreen(world: Point, camera: CanvasCamera, viewport: Point): Point {
  return {
    x: (world.x - camera.x) * camera.zoom + viewport.x / 2,
    y: (world.y - camera.y) * camera.zoom + viewport.y / 2,
  };
}

/** Screen → world (view coordinates). */
export function screenToWorld(screen: Point, camera: CanvasCamera, viewport: Point): Point {
  return {
    x: (screen.x - viewport.x / 2) / camera.zoom + camera.x,
    y: (screen.y - viewport.y / 2) / camera.zoom + camera.y,
  };
}

export function zoomAt(
  camera: CanvasCamera,
  viewport: Point,
  screen: Point,
  factor: number,
): CanvasCamera {
  const before = screenToWorld(screen, camera, viewport);
  const zoom = clamp(camera.zoom * factor, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
  const after = { x: before.x, y: before.y };
  const next: CanvasCamera = { ...camera, zoom };
  // Keep the world point under the cursor stationary.
  next.x = after.x - (screen.x - viewport.x / 2) / zoom;
  next.y = after.y - (screen.y - viewport.y / 2) / zoom;
  return next;
}

export function zoomStep(camera: CanvasCamera, direction: 1 | -1, viewport: Point, center: Point): CanvasCamera {
  return zoomAt(camera, viewport, center, direction === 1 ? 1 + CANVAS_ZOOM_STEP : 1 - CANVAS_ZOOM_STEP);
}

export function clampCamera(camera: CanvasCamera): CanvasCamera {
  return {
    ...camera,
    zoom: clamp(camera.zoom, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM),
  };
}

export function fitCameraToWorld(worldW: number, worldH: number, viewport: Point): CanvasCamera {
  const zoom = clamp(Math.min(viewport.x / worldW, viewport.y / worldH), CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
  return { x: worldW / 2, y: worldH / 2, zoom };
}
