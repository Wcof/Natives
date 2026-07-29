'use client';

import React, { useMemo } from 'react';
import type { ComponentType } from 'react';
import type { MarkdownPreviewProps } from '@uiw/react-markdown-preview';
import SafeMarkdown from '@/components/ui/SafeMarkdown';
import { transformMarkdownUrl } from '@/lib/markdown-safety';
import { rewriteLocalImages } from '@/lib/markdown-local-images';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import { useFileContent } from '@/lib/useFileContent';
import { t, type Locale } from '@/i18n';

export function fileMarkdownUrlTransform(
  url: string,
  key?: string,
  node?: { tagName?: string } | null,
): string {
  if (
    key === 'src'
    && node?.tagName?.toLowerCase() === 'img'
    && /^(?:asset:\/\/localhost|https?:\/\/asset\.localhost)\//i.test(url)
  ) {
    return url;
  }
  return transformMarkdownUrl(url, key, node);
}

export function FileMarkdownView({
  source,
  Preview,
}: {
  source: string;
  Preview?: ComponentType<MarkdownPreviewProps>;
}) {
  return <SafeMarkdown {...fileMarkdownRenderProps(source)} Preview={Preview} />;
}

export function fileMarkdownRenderProps(source: string) {
  return { source, urlTransform: fileMarkdownUrlTransform };
}

export default function FileMarkdownPreview({ path, locale }: { path: string; locale: Locale }) {
  const { content, loading, error } = useFileContent(path);
  const source = useMemo(() => {
    if (content === null) return null;
    const convert = hasNativeFiles() ? fsApi().convertFileSrc : undefined;
    if (!convert) return content;
    const baseDir = path.substring(0, path.lastIndexOf('/')) || '/';
    return rewriteLocalImages(content, baseDir, (absolutePath) => convert(absolutePath) ?? absolutePath).text;
  }, [content, path]);

  if (loading) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-disabled)', fontSize: 12 }}>{t(locale, 'common.loading')}</div>;
  }
  if (error || source === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-disabled)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }
  return <div style={{ padding: 16 }}><FileMarkdownView source={source} /></div>;
}
