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
Object.defineProperty(exports, "__esModule", { value: true });
exports.checkCompatibility = checkCompatibility;
exports.validateManifest = validateManifest;
exports.scanModules = scanModules;
exports.syncModulesToDb = syncModulesToDb;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const zod_1 = require("zod");
const database_1 = require("./database");
function getModulesDir() {
    if (process.env.NATIVES_DB_DIR) {
        return path.join(process.env.NATIVES_DB_DIR, 'modules');
    }
    return path.join(process.env.HOME || '~', '.natives', 'modules');
}
// ── Manifest Schema ──
const ManifestSchema = zod_1.z.object({
    id: zod_1.z.string().min(1),
    name: zod_1.z.string().min(1),
    version: zod_1.z.string().min(1),
    entry: zod_1.z.string().min(1),
    type: zod_1.z.enum(['web', 'mcp']).default('web'),
    description: zod_1.z.string().optional(),
    author: zod_1.z.string().optional(),
    icon: zod_1.z.string().optional(),
    minNativesVersion: zod_1.z.string().optional(),
    permissions: zod_1.z.array(zod_1.z.string()).default([]),
    api: zod_1.z.object({
        bridge: zod_1.z.string().optional(),
    }).optional(),
    lifecycle: zod_1.z.object({
        heartbeatInterval: zod_1.z.number().optional(),
        loadTimeout: zod_1.z.number().optional(),
    }).optional(),
    i18n: zod_1.z.object({
        name: zod_1.z.record(zod_1.z.string()).optional(),
        description: zod_1.z.record(zod_1.z.string()).optional(),
    }).optional(),
});
// ── Version compatibility ──
function parseVersion(v) {
    const parts = v.split('.');
    return {
        major: parseInt(parts[0] || '0', 10),
        minor: parseInt(parts[1] || '0', 10),
        patch: parseInt(parts[2] || '0', 10),
    };
}
const NATIVES_VERSION = '0.1.0';
function checkCompatibility(minVersion) {
    if (!minVersion)
        return { compatible: true };
    const required = parseVersion(minVersion);
    const current = parseVersion(NATIVES_VERSION);
    if (required.major > current.major) {
        return {
            compatible: false,
            warning: `This module requires Natives v${minVersion}, but current version is ${NATIVES_VERSION}. Major version mismatch — cannot load.`,
        };
    }
    if (required.minor > current.minor) {
        return {
            compatible: true,
            warning: `This module requires Natives v${minVersion}, but current version is ${NATIVES_VERSION}. Some features may not work.`,
        };
    }
    return { compatible: true };
}
// ── Manifest validation ──
function validateManifest(data) {
    const result = ManifestSchema.safeParse(data);
    if (!result.success) {
        const errors = result.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`).join('; ');
        return { ok: false, error: `Invalid manifest: ${errors}` };
    }
    return { ok: true, manifest: result.data };
}
// ── Directory scanning ──
function scanModules() {
    const modulesDir = getModulesDir();
    if (!fs.existsSync(modulesDir)) {
        fs.mkdirSync(modulesDir, { recursive: true });
        return [];
    }
    const entries = fs.readdirSync(modulesDir, { withFileTypes: true });
    const results = [];
    for (const entry of entries) {
        if (!entry.isDirectory())
            continue;
        const moduleDir = path.join(modulesDir, entry.name);
        const manifestPath = path.join(moduleDir, 'manifest.json');
        if (!fs.existsSync(manifestPath)) {
            results.push({ moduleId: entry.name, manifest: null, error: 'Missing manifest.json' });
            continue;
        }
        try {
            const raw = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
            const validation = validateManifest(raw);
            if (!validation.ok) {
                results.push({ moduleId: entry.name, manifest: null, error: validation.error });
                continue;
            }
            results.push({ moduleId: entry.name, manifest: validation.manifest });
        }
        catch (err) {
            results.push({ moduleId: entry.name, manifest: null, error: `Failed to parse manifest.json: ${err.message}` });
        }
    }
    return results;
}
// ── Sync scan results to database ──
function syncModulesToDb() {
    const db = (0, database_1.getDb)();
    const results = scanModules();
    for (const result of results) {
        if (!result.manifest)
            continue;
        const { id, name, version, entry, type, description, author, icon, minNativesVersion, permissions } = result.manifest;
        // UPSERT into modules table
        db.prepare(`
      INSERT INTO modules (id, name, version, entry, type, description, author, icon, min_natives_version, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
      ON CONFLICT(id) DO UPDATE SET
        name = excluded.name,
        version = excluded.version,
        entry = excluded.entry,
        description = excluded.description,
        author = excluded.author,
        icon = excluded.icon,
        min_natives_version = excluded.min_natives_version,
        updated_at = datetime('now')
    `).run(id, name, version, entry, type, description || null, author || null, icon || null, minNativesVersion || null);
        // Sync permissions (transaction to avoid partial state)
        const syncPerms = db.transaction(() => {
            db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(id);
            for (const perm of permissions) {
                db.prepare('INSERT INTO module_permissions (module_id, permission, granted) VALUES (?, ?, 0)').run(id, perm);
            }
        });
        syncPerms();
        // Ensure module_order entry exists
        db.prepare('INSERT OR IGNORE INTO module_order (module_id, sort_order) VALUES (?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM module_order))').run(id);
    }
    // Remove modules from DB that no longer exist on disk
    const validIds = results.filter((r) => r.manifest).map((r) => r.manifest.id);
    if (validIds.length > 0) {
        db.prepare(`DELETE FROM modules WHERE id NOT IN (${validIds.map(() => '?').join(',')})`).run(...validIds);
    }
}
//# sourceMappingURL=module-manager.js.map