'use client';

import { memo } from 'react';
import { DiffEditor } from '@monaco-editor/react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

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
        theme="vs-dark"
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
