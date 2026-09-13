// All extension distributors and budget checks consume these exact bytes.
import { existsSync, readdirSync, readFileSync, lstatSync, mkdirSync, rmSync, writeFileSync, createWriteStream } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { transformSync } from 'esbuild';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';

export const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const OMIT = new Set(['node_modules', '.git', 'fixtures', 'test', 'tests', '__pycache__']);
const FILES = /^(manifest\.json|(?!(?:ui-harness|test-dom-mock|files-preview)\.js$)(?:[^/]+|plugins\/.+|apps\/.+|ai-performance\/.+)\.(?:html|css|js)|apps\/catalog-v3\.(?:json|sig)|icons\/(?:agents\/)?(?!folder-source\.svg$)[^/]+|_locales\/[^/]+\/messages\.json)$/;

// 生产 Extension 身份稳定化（本地加载已解压模式）：
// manifest.json 注入固定 RSA 公钥（key 字段），Chrome 由此派生稳定 Extension ID，
// 与加载路径无关；ID = SHA-256(SPKI DER) 前 16 字节映射 a–p。
// 派生脚本一次性生成，密钥材料随仓库受版本控制，任何 Release 不得更换。
// 模式隔离（方案 §5 P1/契约 §4.2）：local 候选使用独立 key/ID，绝不与
// production 共用 Extension ID——两种模式可并存且互不覆盖。
const EXTENSION_ID_KEY = readFileSync(join(ROOT, 'scripts/extension-id-key.b64'), 'utf8').trim();
const EXTENSION_ID_KEY_LOCAL = readFileSync(join(ROOT, 'scripts/extension-id-key.local.b64'), 'utf8').trim();

function extensionIdForKey(keyB64) {
  const hash = createHash('sha256').update(keyB64, 'base64').digest();
  return [...hash.subarray(0, 16)]
    .map((b) => (b >> 4).toString(16) + (b & 15).toString(16))
    .map((h) => h.split('').map((d) => String.fromCharCode(97 + parseInt(d, 16))).join(''))
    .join('');
}

export const STABLE_EXTENSION_ID = extensionIdForKey(EXTENSION_ID_KEY);
export const LOCAL_EXTENSION_ID = extensionIdForKey(EXTENSION_ID_KEY_LOCAL);

// local 模式的 Native Messaging Host 名称（与 build-pkg.sh 的注册文件名一致）。
// esbuild minify 之后字符串字面量通常输出为双引号，因此同时兼容替换单/双引号及无引号标识符。
const HOST_NAME_SUBSTITUTIONS_LOCAL = [
  ['"com.natives.file_manager"', '"com.natives.local.file_manager"'],
  ["'com.natives.file_manager'", "'com.natives.local.file_manager'"],
  ['"com.natives.model_host"', '"com.natives.local.model_host"'],
  ["'com.natives.model_host'", "'com.natives.local.model_host'"],
];

function extensionKeyFor(mode) {
  return mode === 'local' ? EXTENSION_ID_KEY_LOCAL : EXTENSION_ID_KEY;
}

function manifestBytes(root, mode = 'production') {
  const manifest = JSON.parse(readFileSync(join(root, 'extension/manifest.json'), 'utf8'));
  manifest.key = extensionKeyFor(mode);
  return Buffer.from(JSON.stringify(manifest));
}
const EXECUTABLE_MAGIC = new Set(['7f454c46', 'cffaedfe', 'cefaedfe', 'feedfacf', 'feedface', 'cafebabe', 'bebafeca']);

export function distributableFiles(root = ROOT) {
  const base = join(root, 'extension'), files = [];
  if (!existsSync(base)) return files;
  function visit(dir) {
    for (const name of readdirSync(dir)) {
      if (OMIT.has(name)) continue;
      const path = join(dir, name), info = lstatSync(path);
      if (info.isSymbolicLink()) throw new Error('Linked extension asset: ' + relative(base, path));
      if (info.isDirectory()) { visit(path); continue; }
      const file = relative(base, path).split('\\').join('/');
      const bytes = readFileSync(path);
      if (/\.(nap|exe|dll|so|dylib|node|wasm|zip)$/i.test(file)
          || EXECUTABLE_MAGIC.has(bytes.subarray(0, 4).toString('hex')) || bytes.subarray(0, 2).toString() === 'MZ') {
        throw new Error('Native executable or package in extension: ' + file);
      }
      if (FILES.test(file) && !file.split('/').some((part) => part.startsWith('.'))) files.push(file);
    }
  }
  visit(base);
  return files.sort();
}

export function extensionFileBytes(file, root = ROOT) {
  const bytes = readFileSync(join(root, 'extension', file));
  const loader = file.endsWith('.js') ? 'js' : file.endsWith('.css') ? 'css' : null;
  if (loader) return Buffer.from(transformSync(bytes.toString('utf8'), {
    loader, minify: true, target: 'chrome120', legalComments: 'none', charset: 'utf8',
  }).code);
  if (file === 'manifest.json' || file.startsWith('_locales/')) return Buffer.from(JSON.stringify(JSON.parse(bytes)));
  if (file.endsWith('.svg')) {
    // SVG 无构建期压缩通道；去除注释与标签间空白（不改动矢量语义）。
    const minified = bytes.toString('utf8')
      .replace(/<!--[\s\S]*?-->/g, '')
      .replace(/>\s+</g, '><')
      .replace(/\s{2,}/g, ' ')
      .trim();
    return Buffer.from(minified);
  }
  // The signed catalog must retain its exact byte representation.
  return bytes;
}

export function buildExtension(destination = join(ROOT, 'dist/extension'), root = ROOT, mode = 'production') {
  const local = mode === 'local';
  const files = distributableFiles(root);
  const output = files.map((file) => {
    let bytes = file === 'manifest.json' ? manifestBytes(root, mode) : extensionFileBytes(file, root);
    // local 模式隔离：JS 内的 Core/Model Host 名替换为 local 命名空间，
    // 与 build-pkg.sh 的 NM 注册文件名严格一致（plan §5 P1）。
    if (local && file.endsWith('.js')) {
      let text = bytes.toString('utf8');
      for (const [from, to] of HOST_NAME_SUBSTITUTIONS_LOCAL) text = text.split(from).join(to);
      bytes = Buffer.from(text, 'utf8');
    }
    return [file, bytes];
  });
  rmSync(destination, { recursive: true, force: true });
  for (const [file, bytes] of output) {
    const path = join(destination, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, bytes);
  }
  return files;
}

// ---- Release 产物（用户交付链：natives-extension-{version}.zip → 校验 → 解压 → 受管 current/）----
// Runtime 首启负责 ZIP SHA-256 校验、解压与 current 版本切换；本脚本只产出正式 Artifact。
function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

// 纯 Node 存储型 ZIP 写入器（method=store）：不依赖外部 zip/ditto。
// 产出标准 PKZip，unzip/ditto/Chrome 均可读取；条目名以 '/' 分隔，不含顶层前缀。
const CRC32_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = (c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = CRC32_TABLE[(c ^ buf[i]) & 0xFF] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function dosDateTime(mtime) {
  const time = ((mtime.getHours() & 0x1F) << 11) | ((mtime.getMinutes() & 0x3F) << 5) | ((mtime.getSeconds() / 2) & 0x1F);
  const date = (((mtime.getFullYear() - 1980) & 0x7F) << 9) | (((mtime.getMonth() + 1) & 0x0F) << 5) | (mtime.getDate() & 0x1F);
  return { time, date };
}

function createStoredZip(entries, zipPath) {
  // entries: [{ name, bytes, mtime }]
  const chunks = [];
  const central = [];
  let offset = 0;
  for (const { name, bytes, mtime } of entries) {
    const nameBuf = Buffer.from(name, 'utf8');
    const crc = crc32(bytes);
    const { time, date } = dosDateTime(mtime);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);          // version needed
    local.writeUInt16LE(0x0800, 6);      // UTF-8 names
    local.writeUInt16LE(0, 8);           // method: store
    local.writeUInt16LE(time, 10);
    local.writeUInt16LE(date, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(bytes.length, 18);
    local.writeUInt32LE(bytes.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    chunks.push(local, nameBuf, bytes);

    const cd = Buffer.alloc(46);
    cd.writeUInt32LE(0x02014b50, 0);
    cd.writeUInt16LE(20, 4);             // version made by
    cd.writeUInt16LE(20, 6);             // version needed
    cd.writeUInt16LE(0x0800, 8);         // UTF-8 names
    cd.writeUInt16LE(0, 10);             // method: store
    cd.writeUInt16LE(time, 12);
    cd.writeUInt16LE(date, 14);
    cd.writeUInt32LE(crc, 16);
    cd.writeUInt32LE(bytes.length, 20);
    cd.writeUInt32LE(bytes.length, 24);
    cd.writeUInt16LE(nameBuf.length, 28);
    // extra/comment/disk/attrs 留 0
    cd.writeUInt32LE(offset, 42);
    central.push(cd, nameBuf);
    offset += 30 + nameBuf.length + bytes.length;
  }
  const centralBuf = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(offset, 16);
  writeFileSync(zipPath, Buffer.concat([...chunks, centralBuf, eocd]));
}

export function packageExtensionRelease({ version, outputDir = join(ROOT, 'dist/extension-release'), protocolVersion = 1 } = {}) {
  if (!version) throw new Error('packageExtensionRelease: version required');
  const staged = join(outputDir, 'staged-extension');
  // 先清空输出目录再构建 staged，避免 rmSync 把 staged 连同产物一起删掉。
  rmSync(outputDir, { recursive: true, force: true });
  mkdirSync(outputDir, { recursive: true });
  const files = buildExtension(staged);
  // manifest 必须带稳定 key：Chrome 由 key 派生固定 ID，升级不改 ID。
  const manifest = JSON.parse(readFileSync(join(staged, 'manifest.json'), 'utf8'));
  if (manifest.key !== EXTENSION_ID_KEY) throw new Error('staged manifest missing stable extension key');
  if (manifest.version !== version) throw new Error(`manifest version ${manifest.version} != release version ${version}`);

  const zipPath = join(outputDir, `natives-extension-${version}.zip`);
  createStoredZip(files.map((file) => ({
    name: file,
    bytes: readFileSync(join(staged, file)),
    mtime: lstatSync(join(staged, file)).mtime,
  })), zipPath);

  const zipSha = sha256File(zipPath);
  writeFileSync(join(outputDir, 'SHA256SUMS'), `${zipSha}  natives-extension-${version}.zip\n`);
  const metadata = {
    schema: 'natives-extension-metadata/1',
    extensionId: STABLE_EXTENSION_ID,
    version,
    protocolVersion,
    zip: `natives-extension-${version}.zip`,
    sha256: zipSha,
    fileCount: files.length,
    manifestVersion: manifest.manifest_version,
  };
  writeFileSync(join(outputDir, 'extension-metadata.json'), JSON.stringify(metadata, null, 2) + '\n');
  return { ...metadata, outputDir };
}

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const manifest = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8'));
  const result = packageExtensionRelease({ version: manifest.version });
  console.log(JSON.stringify(result));
}
