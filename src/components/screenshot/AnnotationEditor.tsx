'use client';

import { useRef, useState, useCallback, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { useFocusTrap } from '@/lib/useFocusTrap';

interface AnnotationEditorProps {
  locale: Locale;
  imageUrl: string;
  onSave: (dataUrl: string) => void;
  onClose: () => void;
}

type Tool = 'brush' | 'arrow' | 'text' | 'blur' | 'mask';

interface DrawAction {
  tool: Tool;
  points: { x: number; y: number }[];
  color: string;
  size: number;
  text?: string;
}

const COLORS = ['#ff4444', '#ff8800', '#ffdd00', '#44cc44', '#4488ff', '#ffffff', '#000000'];

export default function AnnotationEditor({ locale, imageUrl, onSave, onClose }: AnnotationEditorProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const trap = useFocusTrap();
  const [tool, setTool] = useState<Tool>('brush');
  const [color, setColor] = useState('#ff4444');
  const [size, setSize] = useState(3);
  const [isDrawing, setIsDrawing] = useState(false);
  const [actions, setActions] = useState<DrawAction[]>([]);
  const [currentAction, setCurrentAction] = useState<DrawAction | null>(null);
  const [textInput, setTextInput] = useState('');
  const [textPos, setTextPos] = useState<{ x: number; y: number } | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);

  // Load image onto canvas
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const img = new Image();
    img.onload = () => {
      imageRef.current = img;
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext('2d');
      if (ctx) {
        ctx.drawImage(img, 0, 0);
      }
    };
    img.src = imageUrl;
  }, [imageUrl]);

  // Redraw all actions
  const redraw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Clear and redraw base image
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (imageRef.current) {
      ctx.drawImage(imageRef.current, 0, 0);
    }

    // Draw all completed actions
    for (const action of actions) {
      drawAction(ctx, action);
    }
    // Draw current in-progress action
    if (currentAction) {
      drawAction(ctx, currentAction);
    }
  }, [actions, currentAction]);

  useEffect(() => {
    redraw();
  }, [redraw]);

  const getPos = (e: React.MouseEvent<HTMLCanvasElement>): { x: number; y: number } => {
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;
    return {
      x: (e.clientX - rect.left) * scaleX,
      y: (e.clientY - rect.top) * scaleY,
    };
  };

  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const pos = getPos(e);
    if (tool === 'text') {
      setTextPos(pos);
      setTextInput('');
      return;
    }
    setIsDrawing(true);
    setCurrentAction({ tool, points: [pos], color, size });
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!isDrawing || !currentAction) return;
    const pos = getPos(e);
    setCurrentAction({ ...currentAction, points: [...currentAction.points, pos] });
  };

  const handleMouseUp = () => {
    if (!isDrawing || !currentAction) return;
    setIsDrawing(false);
    setActions((prev) => [...prev, currentAction]);
    setCurrentAction(null);
  };

  const handleTextSubmit = () => {
    if (!textPos || !textInput.trim()) {
      setTextPos(null);
      return;
    }
    const action: DrawAction = {
      tool: 'text',
      points: [textPos],
      color,
      size: Math.max(size, 14),
      text: textInput.trim(),
    };
    setActions((prev) => [...prev, action]);
    setTextPos(null);
    setTextInput('');
  };

  const handleUndo = () => {
    setActions((prev) => prev.slice(0, -1));
  };

  const handleClear = () => {
    setActions([]);
  };

  const handleSave = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dataUrl = canvas.toDataURL('image/png');
    onSave(dataUrl);
  };

  return (
    <div
      ref={trap.dialogRef}
      role="dialog"
      aria-modal="true"
      aria-label={t(locale, 'screenshot.title')}
      onKeyDown={(e) => {
        trap.handleKeyDown(e);
        if (e.key === 'Escape') onClose();
      }}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 10000,
        background: 'rgba(0,0,0,0.85)',
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      {/* Toolbar */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '8px 16px',
          background: 'var(--surface)',
          borderBottom: '1px solid var(--border)',
        }}
      >
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-dim)', marginRight: 8 }}>
          {t(locale, 'screenshot.annotationTools')}
        </span>

        {(['brush', 'arrow', 'text', 'blur', 'mask'] as Tool[]).map((toolName) => (
          <button
            key={toolName}
            className={`btn btn-sm ${tool === toolName ? 'btn-primary' : ''}`}
            onClick={() => setTool(toolName)}
          >
            {toolName === 'brush' && '✏️'}
            {toolName === 'arrow' && '➡️'}
            {toolName === 'text' && '🔤'}
            {toolName === 'blur' && '🌫️'}
            {toolName === 'mask' && '⬛'}
            {' '}{t(locale, `screenshot.${toolName}`)}
          </button>
        ))}

        <div style={{ flex: 1 }} />

        {/* Color picker */}
        {tool !== 'mask' && tool !== 'blur' && (
          <div style={{ display: 'flex', gap: 4 }}>
            {COLORS.map((c) => (
              <button
                key={c}
                onClick={() => setColor(c)}
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: '50%',
                  background: c,
                  border: color === c ? '2px solid var(--accent)' : '1px solid var(--border)',
                  cursor: 'pointer',
                }}
              />
            ))}
          </div>
        )}

        {/* Size slider */}
        <input
          type="range"
          min={1}
          max={20}
          value={size}
          onChange={(e) => setSize(Number(e.target.value))}
          style={{ width: 80 }}
          title={t(locale, 'screenshot.brush')}
        />

        <button className="btn btn-sm" onClick={handleUndo} disabled={actions.length === 0}>
          ↩ {t(locale, 'screenshot.undo')}
        </button>
        <button className="btn btn-sm" onClick={handleClear} disabled={actions.length === 0}>
          🗑 {t(locale, 'screenshot.clearAll')}
        </button>
        <button className="btn btn-sm btn-primary" onClick={handleSave}>
          💾 {t(locale, 'common.save')}
        </button>
        <button className="btn btn-sm" onClick={onClose}>
          ✕ {t(locale, 'screenshot.close')}
        </button>
      </div>

      {/* Canvas */}
      <div style={{ flex: 1, overflow: 'auto', display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 16 }}>
        <canvas
          ref={canvasRef}
          style={{ maxWidth: '100%', maxHeight: '100%', cursor: tool === 'text' ? 'text' : 'crosshair' }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        />
      </div>

      {/* Text input popup */}
      {textPos && (
        <div
          style={{
            position: 'fixed',
            top: '50%',
            left: '50%',
            transform: 'translate(-50%, -50%)',
            background: 'var(--surface)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: 16,
            zIndex: 10001,
          }}
        >
          <input
            autoFocus
            value={textInput}
            onChange={(e) => setTextInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleTextSubmit();
              if (e.key === 'Escape') setTextPos(null);
            }}
            placeholder="Enter text..."
            style={{
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: 4,
              padding: '6px 10px',
              color: 'var(--text)',
              fontSize: 14,
              minWidth: 200,
            }}
          />
          <div style={{ display: 'flex', gap: 6, marginTop: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-sm" onClick={() => setTextPos(null)}>
              {t(locale, 'common.cancel')}
            </button>
            <button className="btn btn-sm btn-primary" onClick={handleTextSubmit}>
              {t(locale, 'common.ok')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Drawing Helpers ──

function drawAction(ctx: CanvasRenderingContext2D, action: DrawAction) {
  ctx.save();
  ctx.strokeStyle = action.color;
  ctx.fillStyle = action.color;
  ctx.lineWidth = action.size;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';

  switch (action.tool) {
    case 'brush':
      if (action.points.length < 2) {
        const p0 = action.points[0]!;
        ctx.fillRect(p0.x - 1, p0.y - 1, 2, 2);
        return;
      }
      ctx.beginPath();
      const first = action.points[0]!;
      ctx.moveTo(first.x, first.y);
      for (let i = 1; i < action.points.length; i++) {
        const pt = action.points[i]!;
        ctx.lineTo(pt.x, pt.y);
      }
      ctx.stroke();
      break;

    case 'arrow':
      if (action.points.length < 2) break;
      {
        const from = action.points[0]!;
        const to = action.points[action.points.length - 1]!;
        const angle = Math.atan2(to.y - from.y, to.x - from.x);
        const headLen = Math.max(12, action.size * 4);

        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
        ctx.stroke();

        // Arrowhead
        ctx.beginPath();
        ctx.moveTo(to.x, to.y);
        ctx.lineTo(to.x - headLen * Math.cos(angle - Math.PI / 6), to.y - headLen * Math.sin(angle - Math.PI / 6));
        ctx.lineTo(to.x - headLen * Math.cos(angle + Math.PI / 6), to.y - headLen * Math.sin(angle + Math.PI / 6));
        ctx.closePath();
        ctx.fill();
      }
      break;

    case 'text':
      if (action.text) {
        const pos = action.points[0]!;
        ctx.font = `${action.size}px sans-serif`;
        ctx.fillText(action.text, pos.x, pos.y);
      }
      break;

    case 'blur':
      if (action.points.length < 2) break;
      {
        const xs = action.points.map(p => p.x);
        const ys = action.points.map(p => p.y);
        const minX = Math.min(...xs);
        const minY = Math.min(...ys);
        const maxX = Math.max(...xs);
        const maxY = Math.max(...ys);
        const w = maxX - minX;
        const h = maxY - minY;
        if (w < 5 || h < 5) break;

        const imageData = ctx.getImageData(minX, minY, w, h);
        ctx.filter = `blur(${Math.max(4, action.size)}px)`;
        ctx.putImageData(imageData, minX, minY);
        ctx.drawImage(ctx.canvas, minX, minY, w, h, minX, minY, w, h);
        ctx.filter = 'none';
      }
      break;

    case 'mask':
      if (action.points.length < 2) break;
      {
        const xs = action.points.map(p => p.x);
        const ys = action.points.map(p => p.y);
        const minX = Math.min(...xs);
        const minY = Math.min(...ys);
        const maxX = Math.max(...xs);
        const maxY = Math.max(...ys);
        ctx.fillStyle = '#000000';
        ctx.fillRect(minX, minY, maxX - minX, maxY - minY);
      }
      break;
  }

  ctx.restore();
}
