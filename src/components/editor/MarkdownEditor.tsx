'use client';

import { useState, useEffect } from 'react';
import dynamic from 'next/dynamic';
import { t, type Locale } from '@/i18n';
import type { MDEditorProps } from '@uiw/react-md-editor';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

const MDEditor = dynamic(
  () => import('@uiw/react-md-editor'),
  { ssr: false }
);

interface MarkdownEditorProps {
  value: string;
  onChange?: (value: string) => void;
  height?: number;
  readOnly?: boolean;
}

export default function MarkdownEditor({
  value,
  onChange,
  height = 400,
  readOnly = false,
}: MarkdownEditorProps) {
  const [locale, setLocale] = useState<Locale>('en');
  const [mode, setMode] = useState<'edit' | 'preview'>(readOnly ? 'preview' : 'edit');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const handleChange = (val?: string) => {
    onChange?.(val || '');
  };

  // Detect current theme from data-theme attribute
  const isDark = typeof document !== 'undefined'
    ? document.documentElement.getAttribute('data-theme') !== 'light'
    : true;

  return (
    <div data-color-mode={isDark ? 'dark' : 'light'} style={{ borderRadius: BORDER_RADIUS.lg, overflow: 'hidden' }}>
      {!readOnly && (
        <div style={{
          display: 'flex', gap: 4, padding: '6px 8px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--surface)',
        }}>
          <button
            className={`btn-ghost ${mode === 'edit' ? 'active' : ''}`}
            onClick={() => setMode('edit')}
            style={{
              fontSize: FONT_SIZE.sm, padding: '2px 8px', borderRadius: BORDER_RADIUS.sm,
              color: mode === 'edit' ? 'var(--primary)' : 'var(--text-disabled)',
              background: mode === 'edit' ? 'var(--primary-soft)' : 'transparent',
            }}
          >
            Edit
          </button>
          <button
            className={`btn-ghost ${mode === 'preview' ? 'active' : ''}`}
            onClick={() => setMode('preview')}
            style={{
              fontSize: FONT_SIZE.sm, padding: '2px 8px', borderRadius: BORDER_RADIUS.sm,
              color: mode === 'preview' ? 'var(--primary)' : 'var(--text-disabled)',
              background: mode === 'preview' ? 'var(--primary-soft)' : 'transparent',
            }}
          >
            Preview
          </button>
        </div>
      )}
      <MDEditor
        value={value}
        onChange={handleChange}
        height={mode === 'edit' ? height : undefined}
        preview={mode === 'edit' ? 'live' : 'preview'}
        visibleDragbar={false}
      />
    </div>
  );
}
