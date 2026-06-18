'use client';

import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface HtmlPreviewProps {
  htmlContent: string;
  fileName?: string;
  sandbox?: string;
}

export default function HtmlPreview({
  htmlContent,
  fileName,
  sandbox = 'allow-scripts allow-forms',
}: HtmlPreviewProps) {
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', height: '100%',
      borderRadius: BORDER_RADIUS.lg, overflow: 'hidden',
      border: '1px solid var(--border)',
    }}>
      {fileName && (
        <div style={{
          padding: '6px 10px', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)',
          background: 'var(--bg-2)', borderBottom: '1px solid var(--border)',
          fontFamily: 'var(--font-mono)',
        }}>
          {fileName}
        </div>
      )}
      <iframe
        srcDoc={htmlContent}
        sandbox={sandbox}
        title={fileName || 'HTML Preview'}
        style={{
          width: '100%',
          flex: 1,
          border: 'none',
          background: 'white',
        }}
      />
    </div>
  );
}
