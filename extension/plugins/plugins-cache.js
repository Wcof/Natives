/**
 * Bounded in-memory and persistent LRU image cache with deduplication (R-P9).
 * Supports 4K background offline resilience with automatic byte-size eviction.
 */

const DEFAULT_CAPACITY = 50;
let maxCapacity = DEFAULT_CAPACITY;
const memCache = new Map();
const inFlightPromises = new Map();

// 50MB budget for 4K background images (holds ~20-25 high-resolution wallpapers)
export const DEFAULT_MAX_BG_CACHE_BYTES = 50 * 1024 * 1024;
let maxBgCacheBytes = DEFAULT_MAX_BG_CACHE_BYTES;
const CACHE_NAME = 'natives-background-cache-v1';
const META_STORAGE_KEY = 'natives_bg_cache_meta';

// In-memory fallback if CacheStorage is unavailable
const mockBlobCache = new Map();

export function setMaxCapacity(capacity) {
  maxCapacity = Math.max(1, capacity || DEFAULT_CAPACITY);
  evictExcess();
}

export function setMaxBackgroundCacheBytes(bytes) {
  maxBgCacheBytes = Math.max(1024 * 1024, Number(bytes) || DEFAULT_MAX_BG_CACHE_BYTES);
}

function evictExpired(now = Date.now()) {
  for (const [key, item] of memCache) {
    if (now > item.expires) {
      memCache.delete(key);
    }
  }
}

function evictExcess() {
  while (memCache.size > maxCapacity) {
    const oldestKey = memCache.keys().next().value;
    if (oldestKey === undefined) break;
    memCache.delete(oldestKey);
  }
}

export function getMemCache(key) {
  const item = memCache.get(key);
  if (!item) return null;
  if (Date.now() > item.expires) {
    memCache.delete(key);
    return null;
  }
  return item.data;
}

export function setMemCache(key, data, ttlMs = 5 * 60 * 1000) {
  const now = Date.now();
  evictExpired(now);
  if (memCache.has(key)) {
    memCache.delete(key);
  } else if (memCache.size >= maxCapacity) {
    const oldestKey = memCache.keys().next().value;
    if (oldestKey !== undefined) {
      memCache.delete(oldestKey);
    }
  }
  memCache.set(key, { data, expires: now + ttlMs });
}

export async function fetchDedup(key, fetcher, ttlMs = 5 * 60 * 1000, signal = null) {
  const cached = getMemCache(key);
  if (cached !== null) return cached;

  if (inFlightPromises.has(key)) {
    return inFlightPromises.get(key);
  }

  const promise = (async () => {
    try {
      if (signal?.aborted) throw new Error('aborted');
      const data = await fetcher(signal);
      if (data !== undefined && data !== null) {
        setMemCache(key, data, ttlMs);
      }
      return data;
    } finally {
      inFlightPromises.delete(key);
    }
  })();

  inFlightPromises.set(key, promise);
  return promise;
}

export function clearMemCache() {
  memCache.clear();
  inFlightPromises.clear();
}

export function getMemCacheSize() {
  return memCache.size;
}

/**
 * Metadata store for background image sizes and usage timestamps
 */
let inMemoryMeta = [];

function getBgMeta() {
  try {
    if (typeof localStorage !== 'undefined' && localStorage?.getItem) {
      const raw = localStorage.getItem(META_STORAGE_KEY);
      return raw ? JSON.parse(raw) : [];
    }
  } catch {}
  return [...inMemoryMeta];
}

function saveBgMeta(list) {
  inMemoryMeta = [...list];
  try {
    if (typeof localStorage !== 'undefined' && localStorage?.setItem) {
      localStorage.setItem(META_STORAGE_KEY, JSON.stringify(list));
    }
  } catch {}
}

/**
 * Evict oldest cached background images if total byte size exceeds quota
 */
async function enforceBgCacheQuota(cache) {
  let meta = getBgMeta();
  let totalBytes = meta.reduce((sum, item) => sum + (Number(item.byteSize) || 0), 0);

  if (totalBytes <= maxBgCacheBytes) return;

  // Sort by lastUsed ascending (oldest first)
  meta.sort((a, b) => (a.lastUsed || 0) - (b.lastUsed || 0));

  while (totalBytes > maxBgCacheBytes * 0.8 && meta.length > 0) {
    const victim = meta.shift();
    if (!victim) break;
    totalBytes -= victim.byteSize || 0;
    if (cache?.delete) {
      try {
        await cache.delete(victim.url);
      } catch {}
    } else {
      mockBlobCache.delete(victim.url);
    }
  }

  saveBgMeta(meta);
}

/**
 * Fetch and persistently cache 4K background image with offline fallback & LRU quota
 */
export async function loadCachedBackground(url, { category = 'default', signal = null } = {}) {
  if (!url || typeof url !== 'string' || !/^https?:\/\//i.test(url)) {
    return url;
  }

  const hasCaches = typeof caches !== 'undefined' && typeof caches.open === 'function';
  let cache = null;
  if (hasCaches) {
    try {
      cache = await caches.open(CACHE_NAME);
    } catch {}
  }

  // 1. Try Cache Match
  if (cache) {
    try {
      const matched = await cache.match(url);
      if (matched) {
        // Touch lastUsed
        const meta = getBgMeta();
        const item = meta.find((m) => m.url === url);
        if (item) {
          item.lastUsed = Date.now();
          saveBgMeta(meta);
        }
        const blob = await matched.blob();
        return URL.createObjectURL(blob);
      }
    } catch {}
  } else if (mockBlobCache.has(url)) {
    const item = mockBlobCache.get(url);
    item.lastUsed = Date.now();
    return url;
  }

  // 2. Fetch Network
  try {
    const response = await fetch(url, { signal, mode: 'cors' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const blob = await response.blob();
    const byteSize = blob.size || 0;

    // Save to CacheStorage
    if (cache) {
      try {
        const responseToCache = new Response(blob, {
          headers: { 'Content-Type': blob.type || 'image/jpeg' },
        });
        await cache.put(url, responseToCache);
      } catch {}
    } else {
      mockBlobCache.set(url, { url, byteSize, lastUsed: Date.now(), category });
    }

    // Update Meta
    let meta = getBgMeta();
    meta = meta.filter((m) => m.url !== url);
    meta.push({ url, byteSize, lastUsed: Date.now(), category });
    saveBgMeta(meta);

    // Enforce quota
    await enforceBgCacheQuota(cache);

    return URL.createObjectURL(blob);
  } catch (err) {
    // 3. Low-network / Offline Fallback: find most recent cached wallpaper in same category
    const meta = getBgMeta();
    const fallbackList = meta
      .filter((m) => m.category === category || category === 'default')
      .sort((a, b) => (b.lastUsed || 0) - (a.lastUsed || 0));

    if (fallbackList.length > 0 && cache) {
      for (const candidate of fallbackList) {
        try {
          const matched = await cache.match(candidate.url);
          if (matched) {
            const blob = await matched.blob();
            return URL.createObjectURL(blob);
          }
        } catch {}
      }
    }

    // Return original url if all fallbacks fail
    return url;
  }
}

/**
 * Get Background Cache statistics (bytes used, image count)
 */
export function getBackgroundCacheStats() {
  const meta = getBgMeta();
  const totalBytes = meta.reduce((sum, item) => sum + (Number(item.byteSize) || 0), 0);
  return {
    count: meta.length,
    totalBytes,
    maxBytes: maxBgCacheBytes,
    usagePercent: Math.min(100, Math.round((totalBytes / maxBgCacheBytes) * 100)),
  };
}

/**
 * Clear all background image cache
 */
export async function clearBackgroundCache() {
  if (typeof caches !== 'undefined' && typeof caches.delete === 'function') {
    try {
      await caches.delete(CACHE_NAME);
    } catch {}
  }
  mockBlobCache.clear();
  saveBgMeta([]);
}
