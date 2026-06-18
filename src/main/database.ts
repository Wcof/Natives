import Database from 'better-sqlite3';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

// Allow test override via environment variable
function getDbDir(): string {
  return process.env.NATIVES_DB_DIR || path.join(os.homedir(), '.natives');
}
function getDbPath(): string {
  return path.join(getDbDir(), 'natives.db');
}
function getLockPath(): string {
  return path.join(getDbDir(), 'natives.lock');
}

let db: Database.Database | null = null;
let lockFd: number | null = null;

// ── File lock（带 PID 写入和陈旧锁检测） ──

function acquireLock(): void {
  const dbDir = getDbDir();
  if (!fs.existsSync(dbDir)) {
    fs.mkdirSync(dbDir, { recursive: true });
  }
  const lockPath = getLockPath();

  // 检查是否有陈旧锁文件（进程已退出但锁文件残留）
  if (fs.existsSync(lockPath)) {
    try {
      const content = fs.readFileSync(lockPath, 'utf-8').trim();
      const oldPid = parseInt(content, 10);
      if (oldPid === process.pid) {
        // 已被当前进程持有，允许复用
        return;
      }
      if (oldPid && !isPidAlive(oldPid)) {
        // 陈旧锁，安全删除
        try { fs.unlinkSync(lockPath); } catch { /* ignore */ }
      }
    } catch {
      // 无法读取锁文件，尝试删除
      try { fs.unlinkSync(lockPath); } catch { /* ignore */ }
    }
  }

  try {
    lockFd = fs.openSync(lockPath, fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_RDWR, 0o644);
    // 写入当前 PID，便于陈旧锁检测
    fs.writeSync(lockFd, String(process.pid));
    fs.fsyncSync(lockFd);
  } catch {
    throw new Error('Another instance is already running with this database.');
  }
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0); // 信号 0 = 检测进程是否存在
    return true;
  } catch {
    return false;
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

// ── Indexes ──

const INDEXES: string[] = [
  'CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read, created_at)',
  'CREATE INDEX IF NOT EXISTS idx_audit_log_module ON permission_audit_log(module_id, created_at)',
];

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

  // 创建所有表（事务内，保证原子性）
  const createAll = db.transaction(() => {
    for (const sql of Object.values(TABLES)) {
      db!.exec(sql);
    }
    for (const sql of INDEXES) {
      db!.exec(sql);
    }
  });
  createAll();

  // Apply incremental migrations
  applyMigrations();

  return db;
}

export function getDb(): Database.Database {
  if (!db) throw new Error('Database not initialized. Call initDb() first.');
  return db;
}

export function closeDb(): void {
  _stmts = null; // 清除预编译语句缓存
  if (db) {
    db.close();
    db = null;
  }
  releaseLock();
}

// ── CRUD helpers（缓存预编译语句 — better-sqlite3 最佳实践）──
// 每次调用 db.prepare() 会重新解析 SQL。高频调用（db:get/set IPC、state persistence、
// usage tracking）时缓存 Statement 对象可消除重复解析开销。

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let _stmts: Record<string, any> | null = null;

function getStmts() {
  if (_stmts) return _stmts;
  const d = getDb();
  _stmts = {
    get: d.prepare('SELECT value FROM module_data WHERE module_id = ? AND key = ?'),
    set: d.prepare('INSERT INTO module_data (module_id, key, value) VALUES (?, ?, ?) ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value'),
    delete: d.prepare('DELETE FROM module_data WHERE module_id = ? AND key = ?'),
    listAll: d.prepare('SELECT key FROM module_data WHERE module_id = ?'),
    listPrefix: d.prepare('SELECT key FROM module_data WHERE module_id = ? AND key LIKE ?'),
  };
  return _stmts;
}

export function dbGet(moduleId: string, key: string): string | undefined {
  const row = getStmts().get.get(moduleId, key) as { value: string } | undefined;
  return row?.value;
}

export function dbSet(moduleId: string, key: string, value: string): void {
  getStmts().set.run(moduleId, key, value);
}

export function dbDelete(moduleId: string, key: string): void {
  getStmts().delete.run(moduleId, key);
}

export function dbList(moduleId: string, prefix?: string): string[] {
  if (prefix) {
    const rows = getStmts().listPrefix.all(moduleId, `${prefix}%`) as Array<{ key: string }>;
    return rows.map((r) => r.key);
  }
  const rows = getStmts().listAll.all(moduleId) as Array<{ key: string }>;
  return rows.map((r) => r.key);
}
