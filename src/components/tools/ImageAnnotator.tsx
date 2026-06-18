'use client';

import { useState, useRef, useEffect } from 'react';
import { Edit2, ArrowRight, Type, EyeOff } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';

export default function ImageAnnotator({ imageUrl, onClose }: { imageUrl?: string; onClose: () => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tool, setTool] = useState<'pen' | 'arrow' | 'text' | 'blur'>('pen');
  const [color, setColor] = useState('#ff433d');
  const [drawing, setDrawing] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  if (!imageUrl) {
    return (
      <div style={{ position: 'fixed', inset: 0, zIndex: 1000, background: 'rgba(0,0,0,0.8)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ padding: 40, color: 'var(--text-faint)', fontSize: FONT_SIZE.lg }}>
          {t(locale, 'screenshot.noImage')}
          <button className="btn btn-ghost" style={{ display: 'block', marginTop: SPACING.md }} onClick={onClose}>{t(locale, 'common.close')}</button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ position: 'fixed', inset: 0, zIndex: 1000, background: 'rgba(0,0,0,0.9)', display: 'flex', flexDirection: 'column' }}>
      {/* Toolbar */}
      <div style={{ display: 'flex', gap: SPACING.xs, padding: SPACING.sm, background: 'var(--vibe-toolbar-bg)', borderBottom: '0.0625rem solid var(--vibe-toolbar-border)', alignItems: 'center' }}>
        {(['pen', 'arrow', 'text', 'blur'] as const).map((toolType) => (
          <button
            key={toolType}
            className={`btn ${tool === toolType ? 'btn-primary' : 'btn-ghost'}`}
            style={{ fontSize: FONT_SIZE.sm, padding: `${SPACING.xs}px ${SPACING.sm}px`, display: 'inline-flex', alignItems: 'center', gap: SPACING.xs }}
            onClick={() => setTool(toolType)}
          >
            {toolType === 'pen' && <Edit2 size={12} />}
            {toolType === 'arrow' && <ArrowRight size={12} />}
            {toolType === 'text' && <Type size={12} />}
            {toolType === 'blur' && <EyeOff size={12} />}
            <span>{toolType === 'pen' ? t(locale, 'screenshot.brush') : t(locale, `screenshot.${toolType}`)}</span>
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <input type="color" value={color} onChange={(e) => setColor(e.target.value)} style={{ width: 24, height: 24, padding: 0, border: 'none' }} />
        <button className="btn btn-primary" style={{ fontSize: FONT_SIZE.sm }} onClick={onClose}>{t(locale, 'common.save')}</button>
        <button className="btn btn-ghost" style={{ fontSize: FONT_SIZE.sm }} onClick={onClose}>{t(locale, 'common.cancel')}</button>
      </div>
      {/* Canvas */}
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', overflow: 'auto' }}>
        <canvas ref={canvasRef} style={{ maxWidth: '90%', maxHeight: '90%', border: '1px solid var(--border)' }} />
      </div>
    </div>
  );
}
