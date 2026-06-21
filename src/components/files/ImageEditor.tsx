'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { Pen, Square, Minus, ArrowRight, Type, Grid3x3, Undo2, Save, FileOutput } from 'lucide-react';

/**
 * ImageEditor — Canvas-based image editor.
 * Reference: fanbox/public/app.js:979-1154 (buildImageEditor/bindImageEditor)
 *
 * Tools: pen / rect / line / arrow / text / mosaic
 * Features: color picker, thickness slider, undo stack (25), format selection (PNG/JPEG/WEBP),
 *           width adjustment, quality control, save / save-as
 * OOM guard: >60MP pixel images reject editing
 */

export interface ImageEditorProps {
  imagePath: string;
  imageName: string;
  onSave: (dataUrl: string, ext: string, asNew: boolean) => void;
  onClose: () => void;
}

type Tool = 'pen' | 'rect' | 'line' | 'arrow' | 'text' | 'mosaic';

interface Point { x: number; y: number; }

const MAX_UNDO = 25;
const OOM_LIMIT = 60_000_000; // 60MP

export default function ImageEditor({ imagePath, imageName, onSave, onClose }: ImageEditorProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const stateRef = useRef<{
    tool: Tool;
    color: string;
    size: number;
    dragging: boolean;
    sx: number; sy: number;
    lastX: number; lastY: number;
    base: HTMLCanvasElement | null;
    undo: HTMLCanvasElement[];
    dirty: boolean;
  }>({
    tool: 'pen', color: '#ff3b30', size: 5, dragging: false,
    sx: 0, sy: 0, lastX: 0, lastY: 0, base: null,
    undo: [], dirty: false,
  });

  const [loading, setLoading] = useState(true);
  const [oomError, setOomError] = useState(false);
  const [tool, setTool] = useState<Tool>('pen');
  const [color, setColor] = useState('#ff3b30');
  const [size, setSize] = useState(5);
  const [format, setFormat] = useState<'png' | 'jpeg' | 'webp'>('png');
  const [width, setWidth] = useState(0);
  const [quality, setQuality] = useState(85);

  // Snapshot helper
  const snapshot = useCallback((canvas: HTMLCanvasElement): HTMLCanvasElement => {
    const c = document.createElement('canvas');
    c.width = canvas.width;
    c.height = canvas.height;
    c.getContext('2d')!.drawImage(canvas, 0, 0);
    return c;
  }, []);

  // Get canvas-relative position
  const getPos = useCallback((canvas: HTMLCanvasElement, ev: React.PointerEvent): Point => {
    const r = canvas.getBoundingClientRect();
    return {
      x: (ev.clientX - r.left) * (canvas.width / r.width),
      y: (ev.clientY - r.top) * (canvas.height / r.height),
    };
  }, []);

  // Draw shape based on current tool
  const drawShape = useCallback((ctx: CanvasRenderingContext2D, st: typeof stateRef.current, x0: number, y0: number, x1: number, y1: number) => {
    ctx.save();
    ctx.strokeStyle = st.color;
    ctx.fillStyle = st.color;
    ctx.lineWidth = st.size;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    if (st.tool === 'rect') {
      ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    } else if (st.tool === 'line' || st.tool === 'arrow') {
      ctx.beginPath();
      ctx.moveTo(x0, y0);
      ctx.lineTo(x1, y1);
      ctx.stroke();
      if (st.tool === 'arrow') {
        const a = Math.atan2(y1 - y0, x1 - x0);
        const h = Math.max(12, st.size * 3.2);
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x1 - h * Math.cos(a - 0.4), y1 - h * Math.sin(a - 0.4));
        ctx.lineTo(x1 - h * Math.cos(a + 0.4), y1 - h * Math.sin(a + 0.4));
        ctx.closePath();
        ctx.fill();
      }
    }
    ctx.restore();
  }, []);

  // Pixelate (mosaic) effect
  const pixelate = useCallback((ctx: CanvasRenderingContext2D, x0: number, y0: number, x1: number, y1: number) => {
    const x = Math.max(0, Math.min(x0, x1));
    const y = Math.max(0, Math.min(y0, y1));
    const w = Math.min(ctx.canvas.width - x, Math.abs(x1 - x0));
    const h = Math.min(ctx.canvas.height - y, Math.abs(y1 - y0));
    if (w < 2 || h < 2) return;
    const block = Math.max(6, Math.round(Math.min(w, h) / 12));
    const data = ctx.getImageData(x, y, w, h);
    const d = data.data;
    for (let by = 0; by < h; by += block) {
      for (let bx = 0; bx < w; bx += block) {
        let r = 0, g = 0, b = 0, n = 0;
        for (let yy = by; yy < Math.min(by + block, h); yy++) {
          for (let xx = bx; xx < Math.min(bx + block, w); xx++) {
            const i = (yy * w + xx) * 4;
            r += d[i] ?? 0; g += d[i + 1] ?? 0; b += d[i + 2] ?? 0; n++;
          }
        }
        r = r / n; g = g / n; b = b / n;
        for (let yy = by; yy < Math.min(by + block, h); yy++) {
          for (let xx = bx; xx < Math.min(bx + block, w); xx++) {
            const i = (yy * w + xx) * 4;
            d[i] = r; d[i + 1] = g; d[i + 2] = b;
          }
        }
      }
    }
    ctx.putImageData(data, x, y);
  }, []);

  // Load image and initialize canvas
  useEffect(() => {
    setLoading(true);
    setOomError(false);
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      // OOM guard
      if (img.naturalWidth * img.naturalHeight > OOM_LIMIT) {
        setOomError(true);
        setLoading(false);
        return;
      }
      const canvas = canvasRef.current;
      if (!canvas) return;
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      ctx.drawImage(img, 0, 0);
      imgRef.current = img;
      setWidth(img.naturalWidth);
      // Determine default format from extension
      const ext = (imageName.split('.').pop() || 'png').toLowerCase();
      if (ext === 'jpg' || ext === 'jpeg') setFormat('jpeg');
      else if (ext === 'webp') setFormat('webp');
      else setFormat('png');
      setLoading(false);
    };
    img.onerror = () => {
      setOomError(true);
      setLoading(false);
    };
    img.src = imagePath;
  }, [imagePath, imageName]);

  // Sync state refs
  useEffect(() => { stateRef.current.tool = tool; }, [tool]);
  useEffect(() => { stateRef.current.color = color; }, [color]);
  useEffect(() => { stateRef.current.size = size; }, [size]);

  // Pointer event handlers
  const onPointerDown = useCallback(async (ev: React.PointerEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const st = stateRef.current;
    const { x, y } = getPos(canvas, ev);

    if (st.tool === 'text') {
      const txt = window.prompt('输入文字');
      if (!txt) return;
      st.undo.push(snapshot(canvas));
      if (st.undo.length > MAX_UNDO) st.undo.shift();
      ctx.save();
      ctx.fillStyle = st.color;
      ctx.textBaseline = 'top';
      ctx.font = `600 ${Math.max(14, st.size * 6)}px ${getComputedStyle(document.documentElement).getPropertyValue('--font-ui') || 'sans-serif'}`;
      ctx.fillText(txt, x, y);
      ctx.restore();
      st.dirty = true;
      return;
    }

    st.base = snapshot(canvas);
    st.dragging = true;
    st.sx = x; st.sy = y;
    st.lastX = x; st.lastY = y;
    canvas.setPointerCapture(ev.pointerId);
  }, [getPos, snapshot]);

  const onPointerMove = useCallback((ev: React.PointerEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const st = stateRef.current;
    if (!st.dragging) return;
    const { x, y } = getPos(canvas, ev);

    if (st.tool === 'pen') {
      ctx.save();
      ctx.strokeStyle = st.color;
      ctx.lineWidth = st.size;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.beginPath();
      ctx.moveTo(st.lastX, st.lastY);
      ctx.lineTo(x, y);
      ctx.stroke();
      ctx.restore();
      st.lastX = x; st.lastY = y;
      return;
    }

    // Restore to base then draw preview
    if (st.base) ctx.drawImage(st.base, 0, 0);
    if (st.tool === 'mosaic') {
      ctx.save();
      ctx.strokeStyle = st.color;
      ctx.setLineDash([6, 4]);
      ctx.lineWidth = 2;
      ctx.strokeRect(st.sx, st.sy, x - st.sx, y - st.sy);
      ctx.restore();
    } else {
      drawShape(ctx, st, st.sx, st.sy, x, y);
    }
  }, [getPos, drawShape]);

  const onPointerUp = useCallback((ev: React.PointerEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const st = stateRef.current;
    if (!st.dragging) return;
    st.dragging = false;
    const { x, y } = getPos(canvas, ev);

    if (st.tool !== 'pen') {
      if (st.base) ctx.drawImage(st.base, 0, 0);
      if (st.tool === 'mosaic') {
        pixelate(ctx, st.sx, st.sy, x, y);
      } else {
        drawShape(ctx, st, st.sx, st.sy, x, y);
      }
    }
    if (st.base) {
      st.undo.push(st.base);
      if (st.undo.length > MAX_UNDO) st.undo.shift();
    }
    st.dirty = true;
  }, [getPos, drawShape, pixelate]);

  // Undo
  const handleUndo = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const st = stateRef.current;
    const snap = st.undo.pop();
    if (!snap) return;
    ctx.drawImage(snap, 0, 0);
  }, []);

  // Export to dataUrl
  const exportImage = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return { dataUrl: '', ext: 'png' };
    const w = Math.max(16, width || canvas.width);
    let out = canvas;
    if (w !== canvas.width) {
      const h = Math.round(canvas.height * (w / canvas.width));
      out = document.createElement('canvas');
      out.width = w;
      out.height = h;
      out.getContext('2d')!.drawImage(canvas, 0, 0, w, h);
    }
    const q = (quality || 85) / 100;
    const mime = format === 'jpeg' ? 'image/jpeg' : (format === 'webp' ? 'image/webp' : 'image/png');
    const dataUrl = out.toDataURL(mime, q);
    const ext = format === 'jpeg' ? 'jpg' : format;
    return { dataUrl, ext };
  }, [width, quality, format]);

  // Save handlers
  const handleSave = useCallback((asNew: boolean) => {
    const { dataUrl, ext } = exportImage();
    if (!dataUrl) return;
    onSave(dataUrl, ext, asNew);
  }, [exportImage, onSave]);

  // Keyboard shortcuts
  useEffect(() => {
    const onKey = (ev: KeyboardEvent) => {
      if ((ev.metaKey || ev.ctrlKey) && ev.key === 'z') {
        ev.preventDefault();
        handleUndo();
      } else if (ev.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [handleUndo, onClose]);

  const tools: { tool: Tool; icon: typeof Pen; title: string }[] = [
    { tool: 'pen', icon: Pen, title: '自由画笔' },
    { tool: 'rect', icon: Square, title: '矩形框' },
    { tool: 'line', icon: Minus, title: '直线' },
    { tool: 'arrow', icon: ArrowRight, title: '箭头' },
    { tool: 'text', icon: Type, title: '文字' },
    { tool: 'mosaic', icon: Grid3x3, title: '打码' },
  ];

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-sm" style={{ color: 'var(--text-dim)' }}>
        加载图片…
      </div>
    );
  }

  if (oomError) {
    return (
      <div className="flex items-center justify-center h-full text-sm" style={{ color: 'var(--text-dim)' }}>
        图片加载失败或过大（&gt;60MP），暂不支持编辑
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 p-2 border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center gap-1">
          {tools.map(({ tool: t, icon: Icon, title }) => (
            <button
              key={t}
              title={title}
              onClick={() => setTool(t)}
              className="p-1.5 rounded transition-colors"
              style={{
                background: tool === t ? 'var(--vibe-btn-hover-bg)' : 'transparent',
                color: tool === t ? 'var(--vibe-btn-hover-color)' : 'var(--text-dim)',
              }}
            >
              <Icon size={16} />
            </button>
          ))}
        </div>
        <input
          type="color"
          value={color}
          onChange={(e) => setColor(e.target.value)}
          title="颜色"
          className="w-7 h-7 rounded cursor-pointer border-0"
        />
        <div className="flex items-center gap-2">
          <input
            type="range"
            min={1}
            max={60}
            value={size}
            onChange={(e) => setSize(Number(e.target.value))}
            title="粗细"
            className="w-20"
          />
          <span className="text-xs" style={{ color: 'var(--text-faint)' }}>{size}px</span>
        </div>
        <button
          onClick={handleUndo}
          title="撤销 ⌘Z"
          className="p-1.5 rounded transition-colors"
          style={{ color: 'var(--text-dim)' }}
        >
          <Undo2 size={16} />
        </button>
      </div>

      {/* Canvas area */}
      <div className="flex-1 overflow-auto flex items-center justify-center p-4" style={{ background: 'var(--bg-3)' }}>
        <canvas
          ref={canvasRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          className="max-w-full max-h-full shadow-lg"
          style={{ touchAction: 'none', cursor: tool === 'text' ? 'text' : 'crosshair' }}
        />
      </div>

      {/* Export bar */}
      <div className="flex items-center gap-3 p-2 border-t" style={{ borderColor: 'var(--border)' }}>
        <label className="flex items-center gap-1 text-xs" style={{ color: 'var(--text-dim)' }}>
          格式
          <select
            value={format}
            onChange={(e) => setFormat(e.target.value as 'png' | 'jpeg' | 'webp')}
            className="bg-transparent border rounded px-1 py-0.5"
            style={{ borderColor: 'var(--border)', color: 'var(--text)' }}
          >
            <option value="png">PNG</option>
            <option value="jpeg">JPEG</option>
            <option value="webp">WEBP</option>
          </select>
        </label>
        <label className="flex items-center gap-1 text-xs" style={{ color: 'var(--text-dim)' }}>
          宽度
          <input
            type="number"
            min={16}
            value={width}
            onChange={(e) => setWidth(Number(e.target.value))}
            className="w-20 bg-transparent border rounded px-1 py-0.5"
            style={{ borderColor: 'var(--border)', color: 'var(--text)' }}
          />
        </label>
        {format !== 'png' && (
          <label className="flex items-center gap-1 text-xs" style={{ color: 'var(--text-dim)' }}>
            质量
            <input
              type="range"
              min={10}
              max={100}
              value={quality}
              onChange={(e) => setQuality(Number(e.target.value))}
              className="w-20"
            />
            <span className="text-xs" style={{ color: 'var(--text-faint)' }}>{quality}%</span>
          </label>
        )}
        <div className="flex-1" />
        <button
          onClick={() => handleSave(true)}
          className="flex items-center gap-1 px-3 py-1.5 rounded text-xs transition-colors"
          style={{
            background: 'var(--vibe-btn-bg)',
            color: 'var(--text-dim)',
          }}
        >
          <FileOutput size={14} />
          另存为
        </button>
        <button
          onClick={() => handleSave(false)}
          className="flex items-center gap-1 px-3 py-1.5 rounded text-xs transition-colors"
          style={{
            background: 'var(--accent)',
            color: 'var(--accent-ink)',
          }}
        >
          <Save size={14} />
          保存
        </button>
      </div>
    </div>
  );
}
