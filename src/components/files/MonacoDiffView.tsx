'use client';

import dynamic from 'next/dynamic';
import { memo, useState, useEffect } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

// Dynamic import — Monaco DiffEditor is ~2MB, only load when viewing diffs
const DiffEditor = dynamic(() => import('@monaco-editor/react').then((m) => m.DiffEditor), {
  ssr: false,
  loading: () => <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-dim)', fontSize: FONT_SIZE.md }}>Loading diff view…</div>,
});

interface MonacoDiffViewProps {
  original: string;
  modified: string;
  language?: string;
  fileName?: string;
}

/**
 * Monaco-powered Git diff viewer (TASK-009).
 * Replaces the simple text-based GitDiffView with a full Monaco DiffEditor
 * providing syntax-highlighted side-by-side diffs, inline change navigation,
 * and word-level diff highlighting.
 */
function MonacoDiffView({ original, modified, language = 'plaintext', fileName }: MonacoDiffViewProps) {
  // Detect theme — light vs dark Monaco theme
  const getMonacoTheme = (): string => {
    if (typeof document !== 'undefined') {
      const htmlTheme = document.documentElement.getAttribute('data-theme');
      if (htmlTheme === 'frosted-jasmine') return 'vs-light';
    }
    return 'vs-dark';
  };

  const [monacoTheme, setMonacoTheme] = useState<string>(getMonacoTheme);

  useEffect(() => {
    const handler = () => setMonacoTheme(getMonacoTheme());
    window.addEventListener('theme-changed', handler);
    const observer = new MutationObserver(handler);
    if (document.documentElement) {
      observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    }
    return () => {
      window.removeEventListener('theme-changed', handler);
      observer.disconnect();
    };
  }, []);
  return (
    <div style={{ height: '100%', minHeight: 300, border: '1px solid var(--vibe-btn-border)', borderRadius: BORDER_RADIUS.md, overflow: 'hidden' }}>
      {fileName && (
        <div style={{
          padding: '4px 10px', fontSize: FONT_SIZE.sm, color: 'var(--text-faint)',
          background: 'var(--vibe-toolbar-bg)', borderBottom: '1px solid var(--vibe-btn-border)',
          fontFamily: 'var(--font-mono)',
        }}>
          {fileName}
        </div>
      )}
      <DiffEditor
        original={original}
        modified={modified}
        language={language}
        theme={monacoTheme}
        options={{
          renderSideBySide: true,
          fontSize: FONT_SIZE.md,
          fontFamily: 'Menlo, Monaco, "Courier New", monospace',
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          readOnly: true,
          wordWrap: 'on',
          renderMarginRevertIcon: false,
          automaticLayout: true,
        }}
        height="100%"
        loading={
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: FONT_SIZE.md }}>
            Loading diff editor...
          </div>
        }
      />
    </div>
  );
}

export default memo(MonacoDiffView);
