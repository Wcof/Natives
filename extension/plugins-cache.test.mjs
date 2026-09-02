import assert from 'node:assert/strict';
import {
  getMemCache,
  setMemCache,
  clearMemCache,
  setMaxCapacity,
  getMemCacheSize,
} from './plugins/plugins-cache.js';

console.log('--- Plugins Cache Unit Tests ---');

// 1. Basic get & set
clearMemCache();
setMemCache('test1', { value: 42 }, 10_000);
assert.deepEqual(getMemCache('test1'), { value: 42 });
assert.equal(getMemCacheSize(), 1);
console.log('✓ Basic set & get passed');

// 2. TTL Expiration
clearMemCache();
setMemCache('expireSoon', 'hello', 10); // 10ms
assert.equal(getMemCache('expireSoon'), 'hello');
await new Promise((resolve) => setTimeout(resolve, 25));
assert.equal(getMemCache('expireSoon'), null);
assert.equal(getMemCacheSize(), 0);
console.log('✓ TTL expiration passed');

// 3. Updating existing key does not duplicate or evict mistakenly
clearMemCache();
setMaxCapacity(3);
setMemCache('k1', 'v1', 10_000);
setMemCache('k2', 'v2', 10_000);
setMemCache('k1', 'v1-updated', 10_000);
assert.equal(getMemCacheSize(), 2);
assert.equal(getMemCache('k1'), 'v1-updated');
assert.equal(getMemCache('k2'), 'v2');
console.log('✓ Update existing key passed');

// 4. Capacity eviction (FIFO/LRU)
clearMemCache();
setMaxCapacity(3);
setMemCache('a', '1', 10_000);
setMemCache('b', '2', 10_000);
setMemCache('c', '3', 10_000);
assert.equal(getMemCacheSize(), 3);
// Adding 4th item when capacity is 3 should evict 'a' (oldest)
setMemCache('d', '4', 10_000);
assert.equal(getMemCacheSize(), 3);
assert.equal(getMemCache('a'), null, 'Oldest item "a" must be evicted');
assert.equal(getMemCache('b'), '2');
assert.equal(getMemCache('c'), '3');
assert.equal(getMemCache('d'), '4');
console.log('✓ Capacity eviction passed');

// 5. Expired entries purged on set
clearMemCache();
setMaxCapacity(3);
setMemCache('exp1', 'e1', 10);
setMemCache('keep1', 'k1', 10_000);
await new Promise((resolve) => setTimeout(resolve, 20));
// Now exp1 is expired. Adding new item should clean exp1 first
setMemCache('new1', 'n1', 10_000);
assert.equal(getMemCache('exp1'), null);
assert.equal(getMemCache('keep1'), 'k1');
assert.equal(getMemCache('new1'), 'n1');
assert.equal(getMemCacheSize(), 2);
console.log('✓ Expired entries purged on set passed');

// 6. Clear cache
clearMemCache();
assert.equal(getMemCacheSize(), 0);
assert.equal(getMemCache('keep1'), null);
console.log('✓ Clear cache passed');

// Reset to default capacity
setMaxCapacity(50);

// 7. Background Image LRU Quota & Offline Resilience
console.log('--- Background Image Cache & Quota Tests ---');
const {
  loadCachedBackground,
  getBackgroundCacheStats,
  clearBackgroundCache,
  setMaxBackgroundCacheBytes,
} = await import('./plugins/plugins-cache.js');

await clearBackgroundCache();
const initialStats = getBackgroundCacheStats();
assert.equal(initialStats.count, 0);
assert.equal(initialStats.totalBytes, 0);

// Mock fetch for image blob in test environment
const originalFetch = globalThis.fetch;
globalThis.fetch = async (url) => ({
  ok: true,
  status: 200,
  blob: async () => ({
    size: 1.5 * 1024 * 1024, // 1.5 MB mock image
    type: 'image/jpeg',
  }),
});
if (!globalThis.URL.createObjectURL) {
  globalThis.URL.createObjectURL = (blob) => `blob:http://localhost/${Date.now()}`;
}

// Set small quota to test LRU auto-eviction (2MB quota: holds 1 image, 2nd image causes eviction)
setMaxBackgroundCacheBytes(2 * 1024 * 1024);

const mockUrl1 = 'https://images.unsplash.com/test-bg-1.jpg';
const cached1 = await loadCachedBackground(mockUrl1, { category: 'unsplash' });
assert.ok(cached1);

let stats = getBackgroundCacheStats();
assert.equal(stats.count, 1);

// Add 2nd image -> triggers quota check and evicts 1st image
const mockUrl2 = 'https://images.unsplash.com/test-bg-2.jpg';
const cached2 = await loadCachedBackground(mockUrl2, { category: 'unsplash' });
assert.ok(cached2);

stats = getBackgroundCacheStats();
assert.equal(stats.count, 1); // 1st was evicted because total exceeded 2MB

await clearBackgroundCache();
assert.equal(getBackgroundCacheStats().count, 0);

// Restore fetch
globalThis.fetch = originalFetch;
console.log('✓ Background image LRU quota & resilience passed');

console.log('All plugins-cache tests passed!\n');
