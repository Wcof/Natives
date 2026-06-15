'use client';

import { useState, useEffect } from 'react';
import dynamic from 'next/dynamic';
import { t, type Locale } from '@/i18n';
import type { MDEditorProps } from '@uiw/react-md-editor';

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

  // Detect current theme from CSS variable
  const isDark = typeof document !== 'undefined'
    ? getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() === '#0b0c0a'
    : true;

  return (
    <div data-color-mode={isDark ? 'dark' : 'light'} style={{ borderRadius: 8, overflow: 'hidden' }}>
      {!readOnly && (
        <div style={{
          display: 'flex', gap: 4, padding: '6px 8px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-2)',
        }}>
          <button
            className={`btn-ghost ${mode === 'edit' ? 'active' : ''}`}
            onClick={() => setMode('edit')}
            style={{
              fontSize: 11, padding: '2px 8px', borderRadius: 4,
              color: mode === 'edit' ? 'var(--accent)' : 'var(--text-faint)',
              background: mode === 'edit' ? 'var(--accent-soft)' : 'transparent',
            }}
          >
            Edit
          </button>
          <button
            className={`btn-ghost ${mode === 'preview' ? 'active' : ''}`}
            onClick={() => setMode('preview')}
            style={{
              fontSize: 11, padding: '2px 8px', borderRadius: 4,
              color: mode === 'preview' ? 'var(--accent)' : 'var(--text-faint)',
              background: mode === 'preview' ? 'var(--accent-soft)' : 'transparent',
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
