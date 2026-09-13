// Managed App Package 扫描器（单 Runtime 整改 P4-2）。
//
// 规则来源：docs/standards/technical/06-sub-apps.md R-SUBAPP-NATIVE-01 ——
// 任何普通官方 Managed App Package 不得包含 Mach-O executable
//（含 app-exec、fund-host、stock-host、*.app、*.dylib 等独立可执行文件）。
// 发现即 exit 1；去 executable 化不取消供应链校验（P4-3）：
// artifact_sha256 / payload_sha256 / Ed25519 catalog 签名校验继续由
// check-catalog-signature.mjs 与 Core 安装事务负责，本扫描器只做载荷形状检查。
//
// 用法：
//   node scripts/apps/check-managed-app-package.mjs --self-test
//   node scripts/apps/check-managed-app-package.mjs <path>...   # .nap 文件或资源包目录
//
// 注意：scripts/apps/package-demo.mjs 与 Natives-App-Fund/scripts/package.sh
// 产出的 legacy executable .nap（迁移期 fallback，AGENTS.md ADR-0027 例外）
// 会按本规则判定为违规；它们是过渡期开发候选，不是 P4 目标资源包。
import { gunzipSync } from 'node:zlib';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';

// 每个文件只读头部判定 magic，避免载入大载荷。
const MAGIC_PROBE_BYTES = 64;

// Mach-O：MH_MAGIC/MH_CIGAM (32/64) + FAT；附带 ELF/PE 以覆盖跨平台残留。
const EXECUTABLE_MAGICS = [
  'feedface', // MH_MAGIC
  'cefaedfe', // MH_CIGAM
  'feedfacf', // MH_MAGIC_64
  'cffaedfe', // MH_CIGAM_64
  'cafebabe', // FAT_MAGIC
  'bebafeca', // FAT_CIGAM
  '7f454c46', // ELF
];

// R-SUBAPP-NATIVE-01 文件名黑名单：*.app、*.dylib、*-host、app-exec。
function nameViolation(name) {
  const base = basename(name);
  if (base === 'app-exec') return 'app-exec executable';
  if (base.endsWith('.dylib')) return '*.dylib shared library';
  if (base.endsWith('.app') || base.endsWith('.app/')) return '*.app bundle';
  if (base.endsWith('-host') || base.includes('-host-')) return '*-host executable payload';
  return null;
}

function magicViolation(bytes) {
  if (bytes.length >= 2 && bytes[0] === 0x4d && bytes[1] === 0x5a) {
    return 'PE/MZ executable';
  }
  const hex = Buffer.from(bytes.subarray(0, 4)).toString('hex');
  if (EXECUTABLE_MAGICS.includes(hex)) {
    return `native executable (magic ${hex})`;
  }
  return null;
}

function modeViolation(mode) {
  // 任意 execute 位（含 0700/0755 等资源包不允许的权限形状）。
  if (typeof mode === 'number' && (mode & 0o111) !== 0) {
    return `executable file mode ${mode.toString(8)}`;
  }
  return null;
}

export function findViolations(entries) {
  const violations = [];
  for (const entry of entries) {
    const reasons = [];
    const byName = nameViolation(entry.name);
    if (byName) reasons.push(byName);
    const byMagic = magicViolation(entry.bytes);
    if (byMagic) reasons.push(byMagic);
    const byMode = modeViolation(entry.mode);
    if (byMode) reasons.push(byMode);
    if (reasons.length) {
      violations.push({ name: entry.name, reasons });
    }
  }
  return violations;
}

export function entriesFromDirectory(dir) {
  const entries = [];
  const walk = (current, prefix) => {
    for (const name of readdirSync(current).sort()) {
      const full = join(current, name);
      const rel = prefix ? `${prefix}/${name}` : name;
      const stat = statSync(full);
      if (stat.isDirectory()) {
        if (name.endsWith('.app')) {
          entries.push({ name: `${rel}/`, mode: 0, bytes: Buffer.alloc(0) });
          continue;
        }
        walk(full, rel);
        continue;
      }
      // 头部探测即可判定 magic；execute 位取自真实文件 mode。
      const fd = readFileSync(full);
      entries.push({ name: rel, mode: stat.mode, bytes: fd.subarray(0, Math.min(fd.length, MAGIC_PROBE_BYTES)) });
    }
  };
  walk(resolve(dir), '');
  return entries;
}

export function entriesFromNapFile(path) {
  const raw = readFileSync(path);
  let payload;
  try {
    payload = gunzipSync(raw);
  } catch {
    payload = raw; // 未压缩载荷：按原字节检查。
  }
  return [{ name: basename(path), mode: 0, bytes: payload.subarray(0, Math.min(payload.length, MAGIC_PROBE_BYTES)) }];
}

export function scanPaths(paths) {
  const entries = [];
  for (const path of paths) {
    const stat = statSync(path);
    entries.push(...(stat.isDirectory() ? entriesFromDirectory(path) : entriesFromNapFile(path)));
  }
  return findViolations(entries);
}

function selfTest() {
  const pass = Buffer.from('{"manifestVersion":1}\n');
  const macho64 = Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]);
  const elf = Buffer.from([0x7f, 0x45, 0x4c, 0x46]);
  const assert = (condition, message) => {
    if (!condition) {
      console.error(`self-test failed: ${message}`);
      process.exitCode = 1;
    }
  };
  assert(findViolations([{ name: 'manifest.json', mode: 0o644, bytes: pass }]).length === 0, 'clean resource entry must pass');
  assert(findViolations([{ name: 'payload', mode: 0o644, bytes: macho64 }]).length === 1, 'Mach-O magic must fail');
  assert(findViolations([{ name: 'payload', mode: 0o644, bytes: elf }]).length === 1, 'ELF magic must fail');
  assert(findViolations([{ name: 'fund-host', mode: 0o644, bytes: pass }]).length === 1, '*-host name must fail');
  assert(findViolations([{ name: 'app-exec', mode: 0o644, bytes: pass }]).length === 1, 'app-exec name must fail');
  assert(findViolations([{ name: 'lib/ui.js', mode: 0o755, bytes: pass }]).length === 1, 'executable mode must fail');
  assert(findViolations([{ name: 'assets/libcore.dylib', mode: 0o644, bytes: pass }]).length === 1, '*.dylib name must fail');
  assert(findViolations([{ name: 'Fund.app/', mode: 0, bytes: Buffer.alloc(0) }]).length === 1, '*.app bundle must fail');
  console.log('managed app package scanner self-test passed');
}

const isMain = process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href;
if (isMain) {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) {
    selfTest();
  } else if (args.length) {
    const violations = scanPaths(args);
    if (violations.length) {
      for (const violation of violations) {
        console.error(`${violation.name}: ${violation.reasons.join(', ')}`);
      }
      console.error(`R-SUBAPP-NATIVE-01 violated: ${violations.length} executable artifact(s) found`);
      process.exit(1);
    }
    console.log('managed app package scan passed');
  } else {
    console.error('usage: check-managed-app-package.mjs (--self-test | <path>...)');
    process.exit(2);
  }
}
