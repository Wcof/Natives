/**
 * use-thumbnail hook — 统一缩略图生成逻辑
 *
 * 调用 window.nativesAPI.thumbnail.generate(filePath, width)，
 * 返回 base64 JPEG data URL。失败时返回 null，加载中返回 undefined。
 *
 * 使用示例:
 *   const { dataUrl, loading, error } = useThumbnail('/path/to/image.png', 160);
 *   if (loading) return <Skeleton />;
 *   if (error || !dataUrl) return <FbImage />;
 *   return <img src={dataUrl} alt={alt} />;
 */

import { useState, useEffect } from 'react';
import { thumbnailApi } from '@/lib/files-api';

const MAX_CACHE = 128;
const cache = new Map<string, string>();
const pending = new Map<string, Promise<string>>();

async function loadThumbnail(filePath: string, width: number): Promise<string> {
  const key = `${filePath}:${width}`;
  const cached = cache.get(key);
  if (cached) return cached;
  const active = pending.get(key);
  if (active) return active;
  const request = (async () => {
    const result = await thumbnailApi().generate(filePath, width);
    const base64 = typeof result === 'string' ? result : (result as { buffer?: string } | null)?.buffer;
    if (!base64) throw new Error('thumbnail returned empty');
    const dataUrl = `data:image/jpeg;base64,${base64}`;
    cache.set(key, dataUrl);
    if (cache.size > MAX_CACHE) cache.delete(cache.keys().next().value as string);
    return dataUrl;
  })();
  pending.set(key, request);
  try { return await request; } finally { pending.delete(key); }
}

interface UseThumbnailResult {
  /** 生成的 data:image/jpeg;base64,... URL，成功时非空 */
  dataUrl: string | null;
  /** 是否正在生成缩略图 */
  loading: boolean;
  /** 生成失败时的错误信息 */
  error: string | null;
}

export function useThumbnail(filePath: string, width: number, enabled = true): UseThumbnailResult {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(enabled);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDataUrl(null);

    (async () => {
      try {
        const result = await loadThumbnail(filePath, width);
        if (!cancelled) {
          setDataUrl(result);
        }
      } catch (err) {
        if (!cancelled) {
          setError((err as Error).message);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    })();

    return () => { cancelled = true; };
  }, [enabled, filePath, width]);

  return { dataUrl, loading, error };
}
