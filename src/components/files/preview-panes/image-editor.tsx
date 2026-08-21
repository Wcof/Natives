'use client';

/**
 * FilePreview 图片编辑 pane（ImageEditor 写路径 + 只读图片预览）。
 * 从 FilePreview.tsx 抽出（FIL-004：format views / lifecycle 分责）。
 */

import { Suspense, lazy, useEffect, useMemo, useState } from 'react';
import { Pencil } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import { authorizeImageEditAsset } from '@/lib/preview/image-edit';
import { createDefaultContext } from '@/lib/preview/composition';
import { PreviewProviderError } from '@/lib/preview/errors';
import { classifyError } from '@/lib/error-classifier';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';

// Lazy-loaded heavy component
const ImageEditor = lazy(() => import('../ImageEditor'));

interface ImageEditUrlState {
  url: string | null;
  error: unknown;
  loading: boolean;
}

function useImageEditUrl(path: string): ImageEditUrlState {
  const context = useMemo(() => createDefaultContext(), []);
  const [state, setState] = useState<ImageEditUrlState>({ url: null, error: null, loading: true });

  useEffect(() => {
    const controller = new AbortController();
    setState({ url: null, error: null, loading: true });
    (async () => {
      try {
        const url = await authorizeImageEditAsset(
          path,
          context,
          fsApi().convertImagePreview,
          controller.signal,
        );
        if (!controller.signal.aborted) setState({ url, error: null, loading: false });
      } catch (error) {
        if (controller.signal.aborted) return;
        if (error instanceof PreviewProviderError && error.code === 'cancelled') return;
        setState({ url: null, error, loading: false });
      }
    })();
    return () => controller.abort();
  }, [context, path]);

  return state;
}

/** 图片编辑写路径（editMode）：预览 → 进入 ImageEditor → 保存/另存 */
export function ImageEditPane({ entry, locale, onImageClick }: {
  entry: FileEntry;
  locale: Locale;
  onImageClick: (src: string) => void;
}) {
  const { url: imageUrl, error, loading } = useImageEditUrl(entry.path);
  const [imageEditing, setImageEditing] = useState(false);

  if (loading) {
    return (
      <div style={{ color: 'var(--text-disabled)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (error) {
    const classified = classifyError(error, { locale });
    return (
      <div style={{ color: 'var(--danger)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {classified.userMessage}
      </div>
    );
  }

  if (!imageUrl) return null;

  if (imageEditing) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 8px', borderBottom: '1px solid var(--border)' }}>
          <button
            onClick={() => setImageEditing(false)}
            className="text-xs px-2 py-1 rounded"
            style={{ background: 'var(--surface)', color: 'var(--text-secondary)' }}
          >
            {t(locale, 'filePreview.backToPreview')}
          </button>
          <span className="text-xs" style={{ color: 'var(--text-disabled)' }}>{entry.name}</span>
        </div>
        <Suspense fallback={<MathCurveLoader />}>
          <ImageEditor
            imagePath={imageUrl}
            imageName={entry.name}
            onSave={(dataUrl, ext, asNew) => {
              if (dataUrl && hasNativeFiles()) {
                const base64 = dataUrl.split(',')[1] || '';
                const p = entry.path || '';
                const dir = p.substring(0, p.lastIndexOf('/')) || '/';
                const entryName = entry.name || 'image';
                const name = asNew
                  ? entryName.replace(/\.[^.]+$/, '') + '-edited.' + ext
                  : entryName;
                fsApi().saveBlob(dir, name, base64).catch(() => {});
              }
              setImageEditing(false);
            }}
            onClose={() => setImageEditing(false)}
          />
        </Suspense>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flex: 1, position: 'relative' }}>
      <img
        src={imageUrl}
        alt={entry.name}
        onClick={() => onImageClick(imageUrl)}
        style={{
          maxWidth: '100%', maxHeight: '100%', objectFit: 'contain',
          background: 'repeating-conic-gradient(color-mix(in srgb, var(--neutral-500) 20%, transparent) 0% 25%, transparent 0% 50%) 50% / 20px 20px',
          cursor: 'zoom-in',
        }}
      />
      <button
        onClick={(e) => { e.stopPropagation(); setImageEditing(true); }}
        title={t(locale, 'filePreview.editImage')}
        style={{
          position: 'absolute', top: 8, right: 8,
          display: 'flex', alignItems: 'center', gap: 4,
          padding: '4px 8px', borderRadius: 6, fontSize: 12,
          background: 'var(--surface)', color: 'var(--text-secondary)',
          border: '1px solid var(--border)', cursor: 'pointer',
        }}
      >
        <Pencil size={13} />
        {t(locale, 'filePreview.editImage')}
      </button>
    </div>
  );
}
