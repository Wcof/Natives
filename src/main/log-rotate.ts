import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

// ── Rotating Log Writer（对标 CodePilot — 50MB 活跃 + 5 归档 = ~300MB 上限）──
// CodePilot 因用户日志达到 12.5GB 而加此防线。

const MAX_ACTIVE_SIZE = 50 * 1024 * 1024; // 50MB

// ── Log Sanitization（对标 CodePilot/log-sanitize.ts — 防止 API key 泄漏到日志）──
// 用户通过"打开日志文件夹"查看日志并附在 bug 报告中，日志文件必须脱敏。

const HOME_DIR = os.homedir();
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
const HOME_REGEX = HOME_DIR ? new RegExp(escapeRegExp(HOME_DIR), 'g') : null;

function maskTail(s: string): string {
  if (s.length <= 4) return '***';
  return '***' + s.slice(-4);
}

const KEY_PATTERNS: { prefix: string; regex: RegExp }[] = [
  { prefix: 'sk-ant-', regex: /\bsk-ant-[A-Za-z0-9_-]{16,}/g },
  { prefix: 'sk-', regex: /\bsk-[A-Za-z0-9_-]{16,}/g },
  { prefix: 'ghp_', regex: /\bghp_[A-Za-z0-9]{20,}/g },
  { prefix: 'gho_', regex: /\bgho_[A-Za-z0-9]{20,}/g },
  { prefix: 'ghs_', regex: /\bghs_[A-Za-z0-9]{20,}/g },
  { prefix: 'ghu_', regex: /\bghu_[A-Za-z0-9]{20,}/g },
  { prefix: 'ghr_', regex: /\bghr_[A-Za-z0-9]{20,}/g },
  { prefix: 'hf_', regex: /\bhf_[A-Za-z0-9]{20,}/g },
  { prefix: 'xai-', regex: /\bxai-[A-Za-z0-9_-]{20,}/g },
  { prefix: 'anthropic-', regex: /\banthropic-[A-Za-z0-9_-]{16,}/g },
  { prefix: 'key-', regex: /\bkey-[A-Za-z0-9_-]{16,}/g },
];

const BEARER_REGEX = /Bearer\s+([A-Za-z0-9._\-+=]{16,})/g;
const QUERY_SECRET_REGEX = /([?&])(api_key|api-key|apikey|access_token|access-token|token|secret|password|pwd|key|auth_token|auth-token|x-api-key)=([^&\s"'<>]+)/gi;
const AUTH_HEADER_REGEX = /(["']?Authorization["']?\s*[:=]\s*["']?)(?:Bearer\s+)?([^\s"'<>,]+)/gi;

const SENSITIVE_FIELDS = [
  'api_key', 'apikey', 'token', 'auth_token', 'access_token', 'refresh_token',
  'secret', 'secret_key', 'client_secret', 'password', 'pwd', 'authorization',
  'x_api_key', 'aws_access_key_id', 'aws_secret_access_key', 'bearer',
];

function buildFieldAlternation(): string {
  return SENSITIVE_FIELDS.map((f) => f.replace(/_/g, '[_-]?')).join('|');
}
const FIELD_ALT = buildFieldAlternation();
const JSON_FIELD_REGEX = new RegExp(`("(?:${FIELD_ALT})"\\s*:\\s*")([^"\\\\]+)(")`, 'gi');
const ENV_FIELD_REGEX = new RegExp(`\\b(${FIELD_ALT})\\s*=\\s*([^\\s,;"'&}\\]]+)`, 'gi');

const BIG_BLOB_REGEX = /\b([A-Za-z0-9_-]{40,})\b/g;
function looksLikeToken(s: string): boolean {
  if (/^[0-9]+$/.test(s)) return false;
  if (/^[0-9a-f]+$/i.test(s) && s.length <= 64) return false;
  return /[A-Z]/.test(s) && /[a-z]/.test(s);
}

function sanitizeLogLine(line: string): string {
  let out = line;
  for (const { prefix, regex } of KEY_PATTERNS) {
    out = out.replace(regex, (m) => prefix + maskTail(m.slice(prefix.length)));
  }
  out = out.replace(BEARER_REGEX, (_m, tok: string) => `Bearer ${maskTail(tok)}`);
  out = out.replace(QUERY_SECRET_REGEX, (_m, sep: string, key: string) => `${sep}${key}=***`);
  out = out.replace(AUTH_HEADER_REGEX, (_m, pfx: string, val: string) => `${pfx}${maskTail(val)}`);
  out = out.replace(JSON_FIELD_REGEX, (_m, pfx: string, _val: string, sfx: string) => `${pfx}***${sfx}`);
  out = out.replace(ENV_FIELD_REGEX, (_m, key: string) => `${key}=***`);
  out = out.replace(BIG_BLOB_REGEX, (m) => {
    if (m.includes('***') || !looksLikeToken(m)) return m;
    return maskTail(m);
  });
  if (HOME_REGEX) out = out.replace(HOME_REGEX, '~');
  return out;
}
const MAX_ARCHIVES = 5;
const LOG_DIR = path.join(os.homedir(), '.natives', 'logs');

let logStream: fs.WriteStream | null = null;
let currentSize = 0;
let initialized = false;

function getLogPath(index?: number): string {
  if (index === undefined || index === 0) {
    return path.join(LOG_DIR, 'natives.log');
  }
  return path.join(LOG_DIR, `natives.${index}.log`);
}

function rotate(): void {
  // 关闭当前流
  if (logStream) {
    logStream.end();
    logStream = null;
  }

  // 删除最老的归档
  const oldestPath = getLogPath(MAX_ARCHIVES);
  try { if (fs.existsSync(oldestPath)) fs.unlinkSync(oldestPath); } catch { /* ignore */ }

  // 归档：natives.4.log ← natives.3.log ← ... ← natives.log
  for (let i = MAX_ARCHIVES - 1; i >= 1; i--) {
    const src = getLogPath(i);
    const dst = getLogPath(i + 1);
    try { if (fs.existsSync(src)) fs.renameSync(src, dst); } catch { /* ignore */ }
  }

  // 当前 → natives.1.log
  try { if (fs.existsSync(getLogPath())) fs.renameSync(getLogPath(), getLogPath(1)); } catch { /* ignore */ }

  // 创建新活跃文件
  openStream();
}

function openStream(): void {
  try {
    if (!fs.existsSync(LOG_DIR)) fs.mkdirSync(LOG_DIR, { recursive: true });
    logStream = fs.createWriteStream(getLogPath(), { flags: 'a' });
    currentSize = 0;
  } catch {
    logStream = null;
  }
}

function writeLine(level: string, args: unknown[]): void {
  if (!initialized) {
    openStream();
    initialized = true;
  }

  if (!logStream) return;

  const ts = new Date().toISOString();
  const msg = args.map((a) => (typeof a === 'string' ? a : JSON.stringify(a))).join(' ');
  const line = sanitizeLogLine(`[${ts}] [${level}] ${msg}`) + '\n';

  // 轮转检查
  if (currentSize + line.length > MAX_ACTIVE_SIZE) {
    rotate();
  }

  if (logStream) {
    logStream.write(line);
    currentSize += line.length;
  }
}

// ── 公共 API ──

export function info(...args: unknown[]): void {
  writeLine('INFO', args);
}

export function warn(...args: unknown[]): void {
  writeLine('WARN', args);
}

export function error(...args: unknown[]): void {
  writeLine('ERROR', args);
}

/**
 * 拦截 console.log/warn/error，同时输出到文件和原始流。
 * 在 app.whenReady() 后调用一次即可。
 */
export function interceptConsole(): void {
  const origLog = console.log.bind(console);
  const origWarn = console.warn.bind(console);
  const origError = console.error.bind(console);

  console.log = (...args: unknown[]) => {
    origLog(...args);
    writeLine('INFO', args);
  };
  console.warn = (...args: unknown[]) => {
    origWarn(...args);
    writeLine('WARN', args);
  };
  console.error = (...args: unknown[]) => {
    origError(...args);
    writeLine('ERROR', args);
  };
}

export function close(): void {
  if (logStream) {
    logStream.end();
    logStream = null;
  }
}
