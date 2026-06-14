'use client';

import { useState, useRef } from 'react';

export default function ImageAnnotator({ imageUrl, onClose }: { imageUrl?: string; onClose: () => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tool, setTool] = useState<'pen' | 'arrow' | 'text' | 'blur'>('pen');
  const [color, setColor] = useState('#ff433d');
  const [drawing, setDrawing] = useState(false);

  if (!imageUrl) {
    return (
      <div style={{ position: 'fixed', inset: 0, zIndex: 1000, background: 'rgba(0,0,0,0.8)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ padding: 40, color: 'var(--text-faint)', fontSize: 13 }}>
          No image selected
          <button className="btn btn-ghost" style={{ display: 'block', marginTop: 12 }} onClick={onClose}>Close</button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ position: 'fixed', inset: 0, zIndex: 1000, background: 'rgba(0,0,0,0.9)', display: 'flex', flexDirection: 'column' }}>
      {/* Toolbar */}
      <div style={{ display: 'flex', gap: 4, padding: 8, background: 'var(--panel)', borderBottom: '1px solid var(--border)', alignItems: 'center' }}>
        {(['pen', 'arrow', 'text', 'blur'] as const).map((t) => (
          <button key={t} className={`btn ${tool === t ? 'btn-primary' : 'btn-ghost'}`} style={{ fontSize: 11, padding: '4px 10px' }} onClick={() => setTool(t)}>
            {t === 'pen' ? '✏️' : t === 'arrow' ? '→' : t === 'text' ? 'T' : '🌫️'} {t}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <input type="color" value={color} onChange={(e) => setColor(e.target.value)} style={{ width: 24, height: 24, padding: 0, border: 'none' }} />
        <button className="btn btn-primary" style={{ fontSize: 11 }} onClick={onClose}>Save</button>
        <button className="btn btn-ghost" style={{ fontSize: 11 }} onClick={onClose}>Cancel</button>
      </div>
      {/* Canvas */}
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', overflow: 'auto' }}>
        <canvas ref={canvasRef} style={{ maxWidth: '90%', maxHeight: '90%', border: '1px solid var(--border)' }} />
      </div>
    </div>
  );
}
