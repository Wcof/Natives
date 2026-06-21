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

import { useState, useEffect, startTransition } from 'react';

interface UseThumbnailResult {
  /** 生成的 data:image/jpeg;base64,... URL，成功时非空 */
  dataUrl: string | null;
  /** 是否正在生成缩略图 */
  loading: boolean;
  /** 生成失败时的错误信息 */
  error: string | null;
}

export function useThumbnail(filePath: string, width: number): UseThumbnailResult {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    startTransition(() => { setLoading(true); });
    startTransition(() => { setError(null); });
    startTransition(() => { setDataUrl(null); });

    (async () => {
      try {
        const api = (window as any).nativesAPI;
        if (!api?.thumbnail?.generate) {
          throw new Error('thumbnail.generate API not available');
        }
        const result = await api.thumbnail.generate(filePath, width);
        if (!cancelled) {
          // API 返回字符串（base64 JPEG）或 { buffer: string }
          const base64 = typeof result === 'string' ? result : (result as any)?.buffer;
          if (base64) {
            startTransition(() => { setDataUrl(`data:image/jpeg;base64,${base64}`); });
          } else {
            throw new Error('thumbnail returned empty');
          }
        }
      } catch (err) {
        if (!cancelled) {
          setError((err as Error).message);
        }
      } finally {
        if (!cancelled) {
          startTransition(() => { setLoading(false); });
        }
      }
    })();

    return () => { cancelled = true; };
  }, [filePath, width]);

  return { dataUrl, loading, error };
}