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
console.log('All plugins-cache tests passed!\n');
