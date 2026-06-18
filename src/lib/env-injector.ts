import { getDb } from '../main/database';
import * as crypto from 'crypto';

// ── safeStorage (Electron only, graceful fallback for tests) ──

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let safeStorage: any = null;
try {
  // Dynamic import: only available in Electron main process
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  safeStorage = require('electron').safeStorage;
} catch {
  // Not running in Electron (test environment) — use fallback encryption
}

// ── Encryption Key Management ──

const ENCRYPTION_KEY_SETTING = 'env_encryption_key';

export function getEncryptionKey(): string {
  const db = getDb();
  const row = db
    .prepare('SELECT value FROM settings WHERE key = ?')
    .get(ENCRYPTION_KEY_SETTING) as { value: string } | undefined;

  if (row) return row.value;

  const key = crypto.randomBytes(32).toString('hex');
  db.prepare('INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime(\'now\'))')
    .run(ENCRYPTION_KEY_SETTING, key);
  return key;
}

// ── Profile CRUD ──

// ★ P2-2: AES-256-GCM with safeStorage priority, exported for bridge-host.ts
export function encrypt(text: string, encryptionKey: string): string {
  if (safeStorage && safeStorage.isEncryptionAvailable()) {
    const encrypted = safeStorage.encryptString(text);
    return encrypted.toString('base64');
  }
  // ★ P2-2: AES-256-GCM instead of XOR
  const crypto = require('crypto');
  const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
  const iv = crypto.randomBytes(16);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
  let encrypted = cipher.update(text, 'utf8', 'base64');
  encrypted += cipher.final('base64');
  const authTag = cipher.getAuthTag().toString('base64');
  return `${iv.toString('base64')}:${authTag}:${encrypted}`;
}

// ★ Exported for bridge-host.ts env.get handler
export function decrypt(encoded: string, encryptionKey: string): string {
  if (safeStorage && safeStorage.isEncryptionAvailable()) {
    const buf = Buffer.from(encoded, 'base64');
    return safeStorage.decryptString(buf);
  }
  // ★ P2-2: Decrypt AES-256-GCM
  const crypto = require('crypto');
  const [ivB64, authTagB64, data] = encoded.split(':');
  if (!ivB64 || !authTagB64 || !data) throw new Error('Invalid encrypted format');
  const key = crypto.scryptSync(encryptionKey, 'natives-salt-v1', 32);
  const iv = Buffer.from(ivB64, 'base64');
  const authTag = Buffer.from(authTagB64, 'base64');
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, iv);
  decipher.setAuthTag(authTag);
  let decrypted = decipher.update(data, 'base64', 'utf8');
  decrypted += decipher.final('utf8');
  return decrypted;
}

export interface Profile {
  id: number;
  name: string;
  is_default: number;
  created_at: string;
}

export function createProfile(name: string): void {
  getDb()
    .prepare('INSERT INTO env_profiles (name, created_at) VALUES (?, datetime(\'now\'))')
    .run(name);
}

export function deleteProfile(name: string): void {
  getDb()
    .prepare('DELETE FROM env_profiles WHERE name = ?')
    .run(name);
}

export function listProfiles(): Profile[] {
  return getDb()
    .prepare('SELECT * FROM env_profiles ORDER BY id')
    .all() as Profile[];
}

// ── Variables ──

export async function setVariable(profileName: string, key: string, value: string, encryptionKey: string): Promise<void> {
  const profile = getDb()
    .prepare('SELECT id FROM env_profiles WHERE name = ?')
    .get(profileName) as { id: number } | undefined;

  if (!profile) throw new Error(`Profile '${profileName}' not found`);

  const encrypted = encrypt(value, encryptionKey);
  getDb()
    .prepare('INSERT INTO env_variables (profile_id, key, value_encrypted) VALUES (?, ?, ?) ON CONFLICT(profile_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted')
    .run(profile.id, key, encrypted);
}

export async function getVariables(profileName: string, encryptionKey: string): Promise<Record<string, string>> {
  const profile = getDb()
    .prepare('SELECT id FROM env_profiles WHERE name = ?')
    .get(profileName) as { id: number } | undefined;

  if (!profile) return {};

  const rows = getDb()
    .prepare('SELECT key, value_encrypted FROM env_variables WHERE profile_id = ?')
    .all(profile.id) as Array<{ key: string; value_encrypted: string }>;

  const result: Record<string, string> = {};
  for (const row of rows) {
    try {
      result[row.key] = decrypt(row.value_encrypted, encryptionKey);
    } catch {
      result[row.key] = '<decryption error>';
    }
  }
  return result;
}

// ── Profile lookup by id ──

// Look up a profile by its stable numeric id (NOT by name).
// Frontend always passes the profile id; querying by name is fragile because
// the id and name fields are independent. See US25/US26/US29.
export function getProfileById(id: string | number): Profile | null {
  const row = getDb()
    .prepare('SELECT * FROM env_profiles WHERE id = ? LIMIT 1')
    .get(String(id)) as Profile | undefined;
  return row || null;
}

// Resolve variables for a profile by id, decrypting each value.
// Falls back to an empty record if the profile is missing (non-fatal).
export async function getVariablesById(profileId: string | number, encryptionKey: string): Promise<Record<string, string>> {
  const profile = getProfileById(profileId);
  if (!profile) return {};
  const rows = getDb()
    .prepare('SELECT key, value_encrypted FROM env_variables WHERE profile_id = ?')
    .all(profile.id) as Array<{ key: string; value_encrypted: string }>;

  const result: Record<string, string> = {};
  for (const row of rows) {
    try {
      result[row.key] = decrypt(row.value_encrypted, encryptionKey);
    } catch {
      result[row.key] = '<decryption error>';
    }
  }
  return result;
}

// ── Default Profile ──

export function setDefaultProfile(name: string): void {
  const db = getDb();
  // Reset all profiles to non-default
  db.prepare('UPDATE env_profiles SET is_default = 0').run();
  // Set selected profile as default
  db.prepare('UPDATE env_profiles SET is_default = 1 WHERE name = ?').run(name);
}

export function getDefaultProfile(): Profile | null {
  const row = getDb()
    .prepare('SELECT * FROM env_profiles WHERE is_default = 1 LIMIT 1')
    .get() as Profile | undefined;
  return row || null;
}

// ── Environment Injection ──

// Inject variables for a profile looked up by NAME (legacy, kept for compatibility).
export async function injectEnv(profileName: string, env: Record<string, string | undefined>, encryptionKey: string): Promise<void> {
  const vars = await getVariables(profileName, encryptionKey);
  for (const [key, value] of Object.entries(vars)) {
    if (env[key] === undefined) {
      env[key] = value;
    }
  }
}

// Inject variables for a profile looked up by ID.
// This is the correct entry point for terminal sessions, where the frontend
// passes the profile id (see US26 Terminal profile selector).
export async function injectEnvById(profileId: string | number, env: Record<string, string | undefined>, encryptionKey: string): Promise<void> {
  const vars = await getVariablesById(profileId, encryptionKey);
  for (const [key, value] of Object.entries(vars)) {
    if (env[key] === undefined) {
      env[key] = value;
    }
  }
}
