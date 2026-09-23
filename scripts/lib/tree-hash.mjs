// Canonical Tree Hash — Node 等价实现（计划 §21）。
// 与 Rust `crates/native-file-host/src/app_tree_hash.rs` 算法逐字节一致：
//   递归普通文件（禁止 symlink）→ relative path `/` 分隔 → 排序 →
//   每文件 `path\0size\0sha256\n` → 最终 SHA-256。
// Packager（installer-package.mjs）与 CI 检查共用本实现，禁止第二份算法。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, lstatSync } from 'node:fs';
import { join, relative } from 'node:path';

export function treeSha256(root) {
  const entries = [];
  walk(root, '');
  const canonical = entries
    .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0))
    .map((entry) => `${entry.path}\0${entry.size}\0${entry.sha256}\n`)
    .join('');
  return createHash('sha256').update(canonical, 'utf8').digest('hex');

  function walk(dir, prefix) {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      const rel = prefix ? `${prefix}/${name}` : name;
      const stat = lstatSync(full);
      assert.ok(!stat.isSymbolicLink(), `symlink not allowed in tree: ${full}`);
      if (stat.isDirectory()) {
        walk(full, rel);
      } else if (stat.isFile()) {
        const sha256 = createHash('sha256').update(readFileSync(full)).digest('hex');
        entries.push({ path: rel, size: stat.size, sha256 });
      }
    }
  }
}

// 自检：与测试 fixture 的 canonical 行格式一致。
export function canonicalLines(root) {
  const lines = [];
  const hash = treeSha256(root);
  assert.match(hash, /^[a-f0-9]{64}$/);
  lines.push(hash);
  return lines;
}

export { relative };
