import { getDb } from '../main/database';

// ── Profile CRUD ──

async function encrypt(text: string): Promise<string> {
  if (typeof window !== 'undefined' && (window as any).nativesAPI?.env?.encrypt) {
    return (window as any).nativesAPI.env.encrypt(text);
  }
  // Fallback: simple base64 encoding (test mode)
  return Buffer.from(text, 'utf-8').toString('base64');
}

async function decrypt(encoded: string): Promise<string> {
  if (typeof window !== 'undefined' && (window as any).nativesAPI?.env?.decrypt) {
    return (window as any).nativesAPI.env.decrypt(encoded);
  }
  // Fallback: simple base64 decoding (test mode)
  return Buffer.from(encoded, 'base64').toString('utf-8');
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

export async function setVariable(profileName: string, key: string, value: string): Promise<void> {
  const profile = getDb()
    .prepare('SELECT id FROM env_profiles WHERE name = ?')
    .get(profileName) as { id: number } | undefined;

  if (!profile) throw new Error(`Profile '${profileName}' not found`);

  const encrypted = await encrypt(value);
  getDb()
    .prepare('INSERT INTO env_variables (profile_id, key, value_encrypted) VALUES (?, ?, ?) ON CONFLICT(profile_id, key) DO UPDATE SET value_encrypted = excluded.value_encrypted')
    .run(profile.id, key, encrypted);
}

export async function getVariables(profileName: string): Promise<Record<string, string>> {
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
      result[row.key] = await decrypt(row.value_encrypted);
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

export async function injectEnv(profileName: string, env: Record<string, string | undefined>): Promise<void> {
  const vars = await getVariables(profileName);
  for (const [key, value] of Object.entries(vars)) {
    // Don't override existing env vars
    if (env[key] === undefined) {
      env[key] = value;
    }
  }
}
