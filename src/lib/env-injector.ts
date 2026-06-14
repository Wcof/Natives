import { getDb } from '../main/database';
import * as crypto from 'crypto';

// ── Profile CRUD ──

const ALGORITHM = 'aes-256-gcm';
const ENCRYPTION_KEY = crypto.createHash('sha256').update('natives-encryption-key-v1').digest('hex').slice(0, 32);

function encrypt(text: string): string {
  const iv = crypto.randomBytes(16);
  const cipher = crypto.createCipheriv(ALGORITHM, ENCRYPTION_KEY, iv);
  let encrypted = cipher.update(text, 'utf-8', 'hex');
  encrypted += cipher.final('hex');
  return iv.toString('hex') + ':' + encrypted + ':' + cipher.getAuthTag().toString('hex');
}

function decrypt(encoded: string): string {
  const parts = encoded.split(':');
  const iv = Buffer.from(parts[0]!, 'hex');
  const encrypted = parts[1]!;
  const authTag = Buffer.from(parts[2]!, 'hex');
  const decipher = crypto.createDecipheriv(ALGORITHM, ENCRYPTION_KEY, iv);
  decipher.setAuthTag(authTag);
  let decrypted = decipher.update(encrypted, 'hex', 'utf-8');
  decrypted += decipher.final('utf-8');
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

export function setVariable(profileName: string, key: string, value: string): void {
  const profile = getDb()
    .prepare('SELECT id FROM env_profiles WHERE name = ?')
    .get(profileName) as { id: number } | undefined;

  if (!profile) throw new Error(`Profile '${profileName}' not found`);

  const encrypted = encrypt(value);
  getDb()
    .prepare('INSERT INTO env_variables (profile_id, key, value_encrypted) VALUES (?, ?, ?) ON CONFLICT(profile_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted')
    .run(profile.id, key, encrypted);
}

export function getVariables(profileName: string): Record<string, string> {
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
      result[row.key] = decrypt(row.value_encrypted);
    } catch {
      result[row.key] = '<decryption error>';
    }
  }
  return result;
}

// ── Default Profile ──

export function getDefaultProfile(): Profile | null {
  const row = getDb()
    .prepare('SELECT * FROM env_profiles WHERE is_default = 1 LIMIT 1')
    .get() as Profile | undefined;
  return row || null;
}

// ── Environment Injection ──

export function injectEnv(profileName: string, env: Record<string, string | undefined>): void {
  const vars = getVariables(profileName);
  for (const [key, value] of Object.entries(vars)) {
    // Don't override existing env vars
    if (env[key] === undefined) {
      env[key] = value;
    }
  }
}
