import type { PreviewContext } from './contracts';
import { cancelledError, fatalError, PreviewProviderError } from './errors';

export type ImagePreviewConverter = (
  path: string,
) => Promise<{ ok: boolean; jpegPath: string }>;

const CONVERSION_EXTENSIONS = new Set(['heic', 'heif', 'tif', 'tiff']);

function throwIfCancelled(signal?: AbortSignal): void {
  if (signal?.aborted) throw cancelledError();
}

/** Source and converted files must each be authorized before creating an asset URL. */
export async function authorizeImageEditAsset(
  path: string,
  context: PreviewContext,
  convertImagePreview?: ImagePreviewConverter,
  signal?: AbortSignal,
): Promise<string> {
  throwIfCancelled(signal);
  const source = await context.authorizeFile(path);
  throwIfCancelled(signal);

  const extension = source.path.split('.').pop()?.toLowerCase() ?? '';
  let assetFile = source;
  if (CONVERSION_EXTENSIONS.has(extension)) {
    if (!convertImagePreview) {
      throw fatalError('host_error', 'image conversion is unavailable');
    }

    let converted: Awaited<ReturnType<ImagePreviewConverter>>;
    try {
      converted = await convertImagePreview(source.path);
    } catch (error) {
      if (error instanceof PreviewProviderError) throw error;
      throw fatalError('host_error', 'image conversion failed');
    }
    throwIfCancelled(signal);
    if (!converted.ok || !converted.jpegPath) {
      throw fatalError('host_error', 'image conversion failed');
    }
    assetFile = await context.authorizeFile(converted.jpegPath);
    throwIfCancelled(signal);
  }

  try {
    const url = context.toAssetUrl(assetFile);
    if (!url) throw fatalError('host_error', 'asset URL creation failed');
    return url;
  } catch (error) {
    if (error instanceof PreviewProviderError) throw error;
    throw fatalError('host_error', 'asset URL creation failed');
  }
}
