// 问题7：StorageOverview 纯逻辑（真实根卷存储信息）。
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { formatBytes, isStorageInfo, storagePercent } from './StorageOverview';

describe('isStorageInfo', () => {
  it('接受完整非负数值对象', () => {
    assert.equal(isStorageInfo({ totalBytes: 100, usedBytes: 40, availableBytes: 60 }), true);
  });

  it('拒绝缺失字段 / 负数 / 非数值', () => {
    assert.equal(isStorageInfo(null), false);
    assert.equal(isStorageInfo({ totalBytes: 100, usedBytes: 40 }), false);
    assert.equal(isStorageInfo({ totalBytes: -1, usedBytes: 40, availableBytes: 60 }), false);
    assert.equal(isStorageInfo({ totalBytes: 'x', usedBytes: 40, availableBytes: 60 }), false);
  });
});

describe('storagePercent', () => {
  it('used/total 百分比', () => {
    assert.equal(storagePercent({ totalBytes: 200, usedBytes: 50, availableBytes: 150 }), 25);
  });

  it('total 为 0 时返回 null（不伪造 0%）', () => {
    assert.equal(storagePercent({ totalBytes: 0, usedBytes: 0, availableBytes: 0 }), null);
  });

  it('used 超过 total 时封顶 100', () => {
    assert.equal(storagePercent({ totalBytes: 100, usedBytes: 200, availableBytes: 0 }), 100);
  });
});

describe('formatBytes', () => {
  it('进制与单位', () => {
    assert.equal(formatBytes(0), '0 B');
    assert.equal(formatBytes(1024), '1 KB');
    assert.equal(formatBytes(1024 * 1024 * 1536), '1.5 GB');
  });
});
