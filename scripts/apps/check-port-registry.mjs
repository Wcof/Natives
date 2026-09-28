// 端口注册表门禁（ADR-0032 §2.2.3 / 审计 F-09/F-12）：
// 静态扫描仓库中"具备本机监听能力"的代码（Rust / Go / ObjC）里的监听端口字面量，
// 与 docs/standards/technical/05-port-registry.md §2 的登记白名单比对，未登记即 FAIL。
//
// 语义边界：
//   - 只扫能 listen 的语言与形态（TcpListener::bind / net.Listen / http.ListenAndServe /
//     flag "port" 默认值 / ObjC 无监听故不扫）；浏览器端 JS 无监听能力，不入范围；
//   - Rust 的 #[cfg(test)] 内联测试、Go 的 *_test.go、*.test.mjs、testdata/fixture 不扫；
//   - 范围覆盖 crates/ modules/ model-host/ installers/macos/resources/
//     （market-host 与 hosts 已于 2026-09-28 退役删除，审计 F-01/F-02/A2）。
import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

const ROOT = join(import.meta.dirname, '..', '..');
const REGISTRY = join(ROOT, 'docs', 'standards', 'technical', '05-port-registry.md');

// 1) 白名单：从注册表 §2 表格解析端口号（首列）
const registryText = readFileSync(REGISTRY, 'utf8');
const whitelist = new Set();
for (const line of registryText.split('\n')) {
  const m = line.match(/^\|\s*(\d{4,5})\s*\|/);
  if (m) whitelist.add(Number(m[1]));
}
if (!whitelist.has(8765)) {
  throw new Error('port registry is missing the 8765 baseline entry');
}

// 2) 扫描范围与排除
const SCOPES = [
  'crates', 'modules', 'model-host',
  'installers/macos/resources',
];
const EXCLUDE = /(^|\/)(third_party|dist|node_modules|target|docs|archive|testdata|fixtures?)(\/|$)/;
function inScope(file) {
  if (EXCLUDE.test(file)) return false;
  if (!SCOPES.some((s) => file === s || file.startsWith(s + '/'))) return false;
  if (file.endsWith('_test.go') || file.endsWith('.test.mjs') || file.endsWith('.test.js')) return false;
  return /\.(?:rs|go|m|h|c)$/.test(file);
}

const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: ROOT, encoding: 'utf8' }).split('\0').filter(Boolean);
const files = tracked.filter((f) => inScope(f) && existsSync(join(ROOT, f)));

// 3) 监听端口字面量模式
const PATTERNS = [
  /127\.0\.0\.1\s*[,:]\s*["']?(\d{4,5})["']?/g,
  /localhost\s*:\s*["']?(\d{4,5})["']?/g,
  /flag\.(?:String|Int)\(\s*"port"\s*,\s*"?(\d{4,5})"?/g,
  /bind\(\s*\(\s*"[^"]*"\s*,\s*(\d{4,5})\s*\)/g,
];

const violations = [];
for (const file of files) {
  let text = readFileSync(join(ROOT, file), 'utf8');
  if (file.endsWith('.rs')) {
    // 内联测试不是生产监听面
    const cut = text.indexOf('#[cfg(test)]');
    if (cut !== -1) text = text.slice(0, cut);
  }
  for (const pattern of PATTERNS) {
    pattern.lastIndex = 0;
    let m;
    while ((m = pattern.exec(text)) !== null) {
      const port = Number(m[1]);
      if (port < 1024 || port > 65535) continue;
      if (whitelist.has(port)) continue;
      const lineNo = text.slice(0, m.index).split('\n').length;
      violations.push(`${file}:${lineNo} — port ${port} 未登记于 05-port-registry.md`);
    }
  }
}

if (violations.length > 0) {
  console.error('未登记的本地监听端口（ADR-0032 §2.2.3 违规）：');
  for (const v of violations) console.error('  ' + v);
  console.error('\n处置：在 05-port-registry.md 登记并立 ADR，或改用注册表内端口/动态端口。');
  process.exit(1);
}
console.log(`port registry gate passed (${files.length} files scanned, whitelist: ${[...whitelist].sort((a, b) => a - b).join(', ')})`);
