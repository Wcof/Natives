// Fixed supply-chain sources. Network errors may switch source; validation errors never do.
export const RELEASE_ROOT = 'https://github.com/Wcof/Natives/releases/';
export const MIRROR_ROOT = 'https://ghproxy.net/';
export const CATALOG_SOURCES = Object.freeze([
  `${RELEASE_ROOT}download/app-catalog-v2/`,
  `${MIRROR_ROOT}${RELEASE_ROOT}download/app-catalog-v2/`,
]);

export function transferError(code, message) { return Object.assign(new Error(message), { code }); }

export function artifactSources(url) {
  const parsed = new URL(url);
  if (parsed.username || parsed.password || parsed.hash || parsed.search
      || !url.startsWith(`${RELEASE_ROOT}download/`)) {
    throw transferError('APP_SOURCE_INVALID', 'package URL is outside the release allowlist');
  }
  return [url, `${MIRROR_ROOT}${url}`];
}

function checkedRedirect(response) {
  if (!response.url) return;
  const url = new URL(response.url);
  if (['chrome-extension:', 'file:'].includes(url.protocol)) return;
  if (url.protocol !== 'https:' || ![
    'github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com',
    'releases.githubusercontent.com', 'ghproxy.net',
  ].includes(url.hostname)) {
    throw transferError('APP_SOURCE_INVALID', 'unexpected download redirect');
  }
}

function cancellable(promise, signal) {
  if (signal.aborted) {
    void promise.catch(() => {});
    return Promise.reject(signal.reason);
  }
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason);
    signal.addEventListener('abort', abort, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
  });
}

export async function fetchBytes(url, { limit, signal, fetchImpl = globalThis.fetch,
  timeoutMs = 5_000, totalTimeoutMs = 60_000, onProgress } = {}) {
  const controller = new AbortController();
  const forwardAbort = () => controller.abort(transferError('APP_CANCELLED', 'download cancelled'));
  if (signal?.aborted) forwardAbort();
  signal?.addEventListener('abort', forwardAbort, { once: true });
  const timeout = () => controller.abort(transferError('APP_NETWORK', 'download timed out'));
  let idleTimer = setTimeout(timeout, timeoutMs);
  const totalTimer = setTimeout(timeout, totalTimeoutMs);
  let reader;
  try {
    if (controller.signal.aborted) throw controller.signal.reason;
    const response = await cancellable(Promise.resolve().then(() => fetchImpl(url, {
      signal: controller.signal, cache: 'no-store', credentials: 'omit', redirect: 'follow',
    })), controller.signal);
    checkedRedirect(response);
    if (!response.ok) throw transferError('APP_NETWORK', `download HTTP ${response.status}`);
    const declared = Number(response.headers?.get('content-length'));
    if (declared > limit) throw transferError('APP_SIZE_LIMIT', 'download exceeds byte budget');
    reader = response.body?.getReader();
    if (!reader) throw transferError('APP_NETWORK', 'download stream unavailable');
    const chunks = [];
    let size = 0;
    for (;;) {
      clearTimeout(idleTimer);
      idleTimer = setTimeout(timeout, timeoutMs);
      const { done, value } = await cancellable(reader.read(), controller.signal);
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw transferError('APP_SIZE_LIMIT', 'download exceeds byte budget');
      chunks.push(value);
      onProgress?.(size);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return bytes;
  } catch (error) {
    controller.abort();
    if (error?.code) throw error;
    throw transferError('APP_NETWORK', 'download connection failed');
  } finally {
    clearTimeout(idleTimer);
    clearTimeout(totalTimer);
    signal?.removeEventListener('abort', forwardAbort);
    if (reader) void reader.cancel().catch(() => {});
  }
}

export async function fetchFromSources(sources, options) {
  let failure;
  for (const url of sources) {
    try { return await fetchBytes(url, options); }
    catch (error) {
      if (error.code !== 'APP_NETWORK') throw error;
      failure = error;
    }
  }
  throw failure || transferError('APP_NETWORK', 'no download source');
}
