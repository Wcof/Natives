import * as crypto from 'crypto';

// ── Token Management (P0-3: main-process only, uses Node.js crypto) ──

const TOKEN_TTL_MS = 24 * 60 * 60 * 1000;
const ROTATION_INTERVAL_MS = Math.floor(TOKEN_TTL_MS * 0.7);

let masterSecret: string;

function getOrCreateSecret(): string {
  if (masterSecret) return masterSecret;
  try {
    const { getDb } = require('../main/database');
    const db = getDb();
    const row = db.prepare("SELECT value FROM settings WHERE key = '_token_master_secret'").get() as { value: string } | undefined;
    if (row) { masterSecret = row.value; return masterSecret; }
    const newSecret = crypto.randomBytes(32).toString('hex');
    db.prepare("INSERT INTO settings (key, value, updated_at) VALUES ('_token_master_secret', ?, datetime('now'))").run(newSecret);
    masterSecret = newSecret;
    return masterSecret;
  } catch {
    masterSecret = crypto.randomBytes(32).toString('hex');
    return masterSecret;
  }
}

const tokenMap = new Map<string, { token: string; moduleId: string; createdAt: number }>();

export function generateSessionToken(moduleId: string): string {
  const nonce = crypto.randomBytes(8).toString('hex');
  const timestamp = Date.now().toString();
  const data = `${moduleId}:${timestamp}:${nonce}`;
  const secret = getOrCreateSecret();
  const token = crypto.createHmac('sha256', secret).update(data).digest('hex');
  tokenMap.set(token, { token, moduleId, createdAt: Date.now() });
  return `${token}:${timestamp}`;
}

export function validateSessionToken(token: string, moduleId: string): boolean {
  const [hash] = token.split(':');
  if (!hash) return false;
  const entry = tokenMap.get(hash);
  if (!entry) return false;
  if (Date.now() - entry.createdAt > TOKEN_TTL_MS) {
    tokenMap.delete(hash);
    return false;
  }
  return entry.moduleId === moduleId;
}

export function invalidateToken(token: string): void {
  const [hash] = token.split(':');
  if (hash) tokenMap.delete(hash);
}

export function invalidateModuleTokens(moduleId: string): void {
  for (const [key, value] of tokenMap) {
    if (value.moduleId === moduleId) tokenMap.delete(key);
  }
}

export function invalidateAllTokens(): void { tokenMap.clear(); }

export function rotateStaleTokens(): number {
  const now = Date.now();
  let rotated = 0;
  for (const [key, value] of tokenMap) {
    if (now - value.createdAt > ROTATION_INTERVAL_MS) { tokenMap.delete(key); rotated++; }
  }
  return rotated;
}

export function getTokenMetrics(): { active: number; oldestMs: number; ttlMs: number } {
  let oldest = Infinity;
  for (const value of tokenMap.values()) { if (value.createdAt < oldest) oldest = value.createdAt; }
  return { active: tokenMap.size, oldestMs: oldest === Infinity ? 0 : Date.now() - oldest, ttlMs: TOKEN_TTL_MS };
}
