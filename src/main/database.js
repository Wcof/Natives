"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.initDb = initDb;
exports.getDb = getDb;
exports.closeDb = closeDb;
exports.dbGet = dbGet;
exports.dbSet = dbSet;
exports.dbDelete = dbDelete;
exports.dbList = dbList;
const better_sqlite3_1 = __importDefault(require("better-sqlite3"));
const path = __importStar(require("path"));
const fs = __importStar(require("fs"));
// Allow test override via environment variable
function getDbDir() {
    return process.env.NATIVES_DB_DIR || path.join(process.env.HOME || '~', '.natives');
}
function getDbPath() {
    return path.join(getDbDir(), 'natives.db');
}
function getLockPath() {
    return path.join(getDbDir(), 'natives.lock');
}
let db = null;
let lockFd = null;
// ── File lock ──
function acquireLock() {
    const dbDir = getDbDir();
    if (!fs.existsSync(dbDir)) {
        fs.mkdirSync(dbDir, { recursive: true });
    }
    try {
        lockFd = fs.openSync(getLockPath(), fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_RDWR, 0o644);
    }
    catch {
        throw new Error('Another instance is already running with this database.');
    }
}
function releaseLock() {
    if (lockFd !== null) {
        fs.closeSync(lockFd);
        lockFd = null;
        try {
            fs.unlinkSync(getLockPath());
        }
        catch { /* ignore */ }
    }
}
// ── Schema ──
const TABLES = {
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
const MIGRATIONS = [];
function applyMigrations() {
    for (const migration of MIGRATIONS) {
        const existing = db.pragma(`table_info(${migration.table})`);
        const existingNames = new Set(existing.map((r) => r.name));
        for (const col of migration.columns) {
            if (!existingNames.has(col.name)) {
                db.exec(`ALTER TABLE ${migration.table} ADD COLUMN ${col.name} ${col.def}`);
            }
        }
    }
}
// ── Initialization ──
function initDb() {
    if (db)
        return db;
    acquireLock();
    db = new better_sqlite3_1.default(getDbPath());
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
function getDb() {
    if (!db)
        throw new Error('Database not initialized. Call initDb() first.');
    return db;
}
function closeDb() {
    if (db) {
        db.close();
        db = null;
    }
    releaseLock();
}
// ── CRUD helpers ──
function dbGet(moduleId, key) {
    const row = getDb()
        .prepare('SELECT value FROM module_data WHERE module_id = ? AND key = ?')
        .get(moduleId, key);
    return row?.value;
}
function dbSet(moduleId, key, value) {
    getDb()
        .prepare('INSERT INTO module_data (module_id, key, value) VALUES (?, ?, ?) ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value')
        .run(moduleId, key, value);
}
function dbDelete(moduleId, key) {
    getDb()
        .prepare('DELETE FROM module_data WHERE module_id = ? AND key = ?')
        .run(moduleId, key);
}
function dbList(moduleId, prefix) {
    if (prefix) {
        const rows = getDb()
            .prepare('SELECT key FROM module_data WHERE module_id = ? AND key LIKE ?')
            .all(moduleId, `${prefix}%`);
        return rows.map((r) => r.key);
    }
    const rows = getDb()
        .prepare('SELECT key FROM module_data WHERE module_id = ?')
        .all(moduleId);
    return rows.map((r) => r.key);
}
//# sourceMappingURL=database.js.map