import Database from 'better-sqlite3';
import * as path from 'path';
import * as fs from 'fs';

// Allow test override via environment variable
function getDbDir(): string {
  return process.env.NATIVES_DB_DIR || path.join(process.env.HOME || '~', '.natives');
}
function getDbPath(): string {
  return path.join(getDbDir(), 'natives.db');
}
function getLockPath(): string {
  return path.join(getDbDir(), 'natives.lock');
}

let db: Database.Database | null = null;
let lockFd: number | null = null;

// ── File lock ──

function acquireLock(): void {
  const dbDir = getDbDir();
  if (!fs.existsSync(dbDir)) {
    fs.mkdirSync(dbDir, { recursive: true });
  }
  try {
    lockFd = fs.openSync(getLockPath(), fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_RDWR, 0o644);
  } catch {
    throw new Error('Another instance is already running with this database.');
  }
}

function releaseLock(): void {
  if (lockFd !== null) {
    fs.closeSync(lockFd);
    lockFd = null;
    try { fs.unlinkSync(getLockPath()); } catch { /* ignore */ }
  }
}

// ── Schema ──

const TABLES: Record<string, string> = {
  modules: `CREATE TABLE IF NOT EXISTS modules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    entry TEXT NOT NULL,
    type TEXT NOT NULL,
    description TEXT,
    author TEXT,
    icon TEXT,
    enabled INTEGER DEFAULT 1,
    min_natives_version TEXT,
    state TEXT DEFAULT 'installed',
    created_at TEXT,
    updated_at TEXT
  )`,

  module_permissions: `CREATE TABLE IF NOT EXISTS module_permissions (
    module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    granted INTEGER DEFAULT 0,
    PRIMARY KEY (module_id, permission)
  )`,

  settings: `CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT
  )`,

  module_data: `CREATE TABLE IF NOT EXISTS module_data (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT,
    UNIQUE(module_id, key)
  )`,

  workshop_cache: `CREATE TABLE IF NOT EXISTS workshop_cache (
    id TEXT PRIMARY KEY,
    name TEXT,
    version TEXT,
    description TEXT,
    author TEXT,
    icon TEXT,
    permissions TEXT,
    installed INTEGER DEFAULT 0
  )`,

  env_profiles: `CREATE TABLE IF NOT EXISTS env_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    is_default INTEGER DEFAULT 0,
    created_at TEXT
  )`,

  env_variables: `CREATE TABLE IF NOT EXISTS env_variables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value_encrypted TEXT NOT NULL,
    UNIQUE(profile_id, key)
  )`,

  notifications: `CREATE TABLE IF NOT EXISTS notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT,
    title TEXT,
    body TEXT,
    level TEXT DEFAULT 'info',
    read INTEGER DEFAULT 0,
    created_at TEXT
  )`,

  module_order: `CREATE TABLE IF NOT EXISTS module_order (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE UNIQUE,
    sort_order INTEGER NOT NULL
  )`,

  // TASK-001: Permission audit trail
  permission_audit_log: `CREATE TABLE IF NOT EXISTS permission_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL,
    permission TEXT NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('grant','revoke','deny','approve')),
    granted INTEGER NOT NULL,
    reason TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
  )`,
};

// ── Migration ──

const MIGRATIONS: Array<{ table: string; columns: Array<{ name: string; def: string }> }> = [];

function applyMigrations(): void {
  for (const migration of MIGRATIONS) {
    const existing = db!.pragma(`table_info(${migration.table})`) as Array<{ name: string }>;
    const existingNames = new Set(existing.map((r) => r.name));
    for (const col of migration.columns) {
      if (!existingNames.has(col.name)) {
        db!.exec(`ALTER TABLE ${migration.table} ADD COLUMN ${col.name} ${col.def}`);
      }
    }
  }
}

// ── Initialization ──

export function initDb(): Database.Database {
  if (db) return db;

  acquireLock();

  db = new Database(getDbPath());

  // WAL mode
  db.pragma('journal_mode = WAL');
  db.pragma('foreign_keys = ON');

  // Create all tables
  for (const sql of Object.values(TABLES)) {
    db.exec(sql);
  }

  // Apply incremental migrations
  applyMigrations();

  return db;
}

export function getDb(): Database.Database {
  if (!db) throw new Error('Database not initialized. Call initDb() first.');
  return db;
}

export function closeDb(): void {
  if (db) {
    db.close();
    db = null;
  }
  releaseLock();
}

// ── CRUD helpers ──

export function dbGet(moduleId: string, key: string): string | undefined {
  const row = getDb()
    .prepare('SELECT value FROM module_data WHERE module_id = ? AND key = ?')
    .get(moduleId, key) as { value: string } | undefined;
  return row?.value;
}

export function dbSet(moduleId: string, key: string, value: string): void {
  getDb()
    .prepare('INSERT INTO module_data (module_id, key, value) VALUES (?, ?, ?) ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value')
    .run(moduleId, key, value);
}

export function dbDelete(moduleId: string, key: string): void {
  getDb()
    .prepare('DELETE FROM module_data WHERE module_id = ? AND key = ?')
    .run(moduleId, key);
}

export function dbList(moduleId: string, prefix?: string): string[] {
  if (prefix) {
    const rows = getDb()
      .prepare('SELECT key FROM module_data WHERE module_id = ? AND key LIKE ?')
      .all(moduleId, `${prefix}%`) as Array<{ key: string }>;
    return rows.map((r) => r.key);
  }
  const rows = getDb()
    .prepare('SELECT key FROM module_data WHERE module_id = ?')
    .all(moduleId) as Array<{ key: string }>;
  return rows.map((r) => r.key);
}
