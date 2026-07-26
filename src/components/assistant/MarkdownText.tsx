'use client';

/**
 * Safe GFM Markdown for assistant text / plan blocks.
 *
 * Uses the Markdown preview exported by @uiw/react-md-editor (nohighlight)
 * so we reuse the installed editor stack without pulling the full editor
 * into the assistant first paint (dynamic import + ssr: false).
 */

import React, { Component, type ComponentType, type ErrorInfo, type ReactNode } from 'react';
import dynamic from 'next/dynamic';
import type { MarkdownPreviewProps } from '@uiw/react-markdown-preview';
import styles from './MarkdownText.module.css';
import {
  SAFE_MARKDOWN_ELEMENTS,
  pluginsFilter,
  rewriteMarkdownNode,
  transformMarkdownUrl,
} from './markdown-safety';

export type { MarkdownPreviewProps };
export {
  SAFE_MARKDOWN_ELEMENTS,
  isSafeMarkdownUrl,
  isSafeImageSource,
  transformMarkdownUrl,
  pluginsFilter,
  rewriteMarkdownNode,
} from './markdown-safety';

export interface MarkdownTextProps {
  source?: string | null;
  className?: string;
  /** Optional override for tests (sync preview component). */
  Preview?: ComponentType<MarkdownPreviewProps>;
}

function PlainFallback({ source, className }: { source: string; className?: string }) {
  return (
    <pre className={[styles.fallback, className].filter(Boolean).join(' ')}>{source}</pre>
  );
}

class MarkdownErrorBoundary extends Component<
  { source: string; className?: string; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo): void {
    // Swallow — fallback UI is enough for model output edge cases.
  }

  render(): ReactNode {
    if (this.state.failed) {
      return <PlainFallback source={this.props.source} className={this.props.className} />;
    }
    return this.props.children;
  }
}

/** Sync renderer used by production (via dynamic) and unit tests. */
export function MarkdownTextView({
  source,
  className,
  Preview,
}: {
  source: string;
  className?: string;
  Preview: ComponentType<MarkdownPreviewProps>;
}) {
  const PreviewComponent = Preview;
  return (
    <MarkdownErrorBoundary source={source} className={className}>
      <div className={[styles.root, className].filter(Boolean).join(' ')} data-assistant-markdown>
        <PreviewComponent
          source={source}
          skipHtml
          disableCopy
          urlTransform={transformMarkdownUrl}
          allowedElements={[...SAFE_MARKDOWN_ELEMENTS]}
          unwrapDisallowed
          pluginsFilter={pluginsFilter as MarkdownPreviewProps['pluginsFilter']}
          rehypeRewrite={rewriteMarkdownNode as MarkdownPreviewProps['rehypeRewrite']}
          wrapperElement={{
            className: styles.root,
          }}
          style={{ background: 'transparent', color: 'inherit' }}
        />
      </div>
    </MarkdownErrorBoundary>
  );
}

const DynamicMarkdownPreview = dynamic(
  async () => {
    // nohighlight avoids prism + rehype-raw path from the default preview bundle.
    const mod = await import('@uiw/react-md-editor/nohighlight');
    const Markdown = mod.default.Markdown;
    if (!Markdown) {
      throw new Error('MDEditor.Markdown unavailable');
    }
    return Markdown;
  },
  {
    ssr: false,
    loading: () => null,
  },
);

/**
 * Assistant-safe Markdown. Empty input renders nothing; failures fall back to plain text.
 */
export default function MarkdownText({ source, className, Preview }: MarkdownTextProps) {
  const text = source ?? '';
  if (!text.trim()) return null;

  if (Preview) {
    return <MarkdownTextView source={text} className={className} Preview={Preview} />;
  }

  return (
    <MarkdownErrorBoundary source={text} className={className}>
      <div className={[styles.root, className].filter(Boolean).join(' ')} data-assistant-markdown>
        <DynamicMarkdownPreview
          source={text}
          skipHtml
          disableCopy
          urlTransform={transformMarkdownUrl}
          allowedElements={[...SAFE_MARKDOWN_ELEMENTS]}
          unwrapDisallowed
          pluginsFilter={pluginsFilter as MarkdownPreviewProps['pluginsFilter']}
          rehypeRewrite={rewriteMarkdownNode as MarkdownPreviewProps['rehypeRewrite']}
          wrapperElement={{
            className: styles.root,
          }}
          style={{ background: 'transparent', color: 'inherit' }}
        />
      </div>
    </MarkdownErrorBoundary>
  );
}
