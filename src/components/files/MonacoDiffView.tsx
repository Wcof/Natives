'use client';

import { memo } from 'react';
import { DiffEditor } from '@monaco-editor/react';

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
    <div style={{ height: '100%', minHeight: 300, border: '1px solid var(--border,#262920)', borderRadius: 6, overflow: 'hidden' }}>
      {fileName && (
        <div style={{
          padding: '4px 10px', fontSize: 11, color: 'var(--text-faint)',
          background: 'var(--bg-2,#131410)', borderBottom: '1px solid var(--border,#262920)',
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
          fontSize: 12,
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
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            Loading diff editor...
          </div>
        }
      />
    </div>
  );
}

export default memo(MonacoDiffView);
