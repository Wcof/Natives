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
exports.installModule = installModule;
exports.uninstallModule = uninstallModule;
exports.enableModule = enableModule;
exports.disableModule = disableModule;
exports.getInstalledModules = getInstalledModules;
exports.readManifestFromSource = readManifestFromSource;
exports.updateModule = updateModule;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const adm_zip_1 = __importDefault(require("adm-zip"));
const database_1 = require("./database");
const module_manager_1 = require("./module-manager");
function getModulesDir() {
    if (process.env.NATIVES_DB_DIR) {
        return path.join(process.env.NATIVES_DB_DIR, 'modules');
    }
    return path.join(process.env.HOME || '~', '.natives', 'modules');
}
function ensureModulesDir() {
    const modulesDir = getModulesDir();
    if (!fs.existsSync(modulesDir)) {
        fs.mkdirSync(modulesDir, { recursive: true });
    }
}
function installModule(source) {
    ensureModulesDir();
    let moduleDir = null;
    let manifest;
    try {
        // Determine if source is a directory or ZIP
        const stat = fs.statSync(source);
        if (stat.isDirectory()) {
            manifest = readManifestFromDir(source);
            const dest = path.join(getModulesDir(), manifest.id);
            if (fs.existsSync(dest)) {
                fs.rmSync(dest, { recursive: true });
            }
            copyDirSync(source, dest);
            moduleDir = dest;
        }
        else if (stat.isFile() && source.endsWith('.zip')) {
            const extracted = extractZip(source);
            manifest = readManifestFromDir(extracted);
            const dest = path.join(getModulesDir(), manifest.id);
            if (fs.existsSync(dest)) {
                fs.rmSync(dest, { recursive: true });
            }
            fs.renameSync(extracted, dest);
            moduleDir = dest;
        }
        else {
            return { success: false, error: 'Source must be a directory or .zip file' };
        }
        // Register in database
        const db = (0, database_1.getDb)();
        db.prepare(`
      INSERT INTO modules (id, name, version, entry, type, description, author, icon, min_natives_version, state, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'installed', datetime('now'), datetime('now'))
      ON CONFLICT(id) DO UPDATE SET
        name = excluded.name, version = excluded.version, entry = excluded.entry,
        description = excluded.description, author = excluded.author, icon = excluded.icon,
        min_natives_version = excluded.min_natives_version, state = 'installed',
        updated_at = datetime('now')
    `).run(manifest.id, manifest.name, manifest.version, manifest.entry, manifest.type, manifest.description || null, manifest.author || null, manifest.icon || null, manifest.minNativesVersion || null);
        // Register permissions
        db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(manifest.id);
        for (const perm of manifest.permissions || []) {
            db.prepare('INSERT INTO module_permissions (module_id, permission, granted) VALUES (?, ?, 0)').run(manifest.id, perm);
        }
        // Ensure order entry
        db.prepare('INSERT OR IGNORE INTO module_order (module_id, sort_order) VALUES (?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM module_order))').run(manifest.id);
        return { success: true, moduleId: manifest.id };
    }
    catch (err) {
        // Clean up on failure
        if (moduleDir && fs.existsSync(moduleDir)) {
            fs.rmSync(moduleDir, { recursive: true });
        }
        return { success: false, error: err.message };
    }
}
function uninstallModule(moduleId) {
    try {
        const moduleDir = path.join(getModulesDir(), moduleId);
        if (fs.existsSync(moduleDir)) {
            fs.rmSync(moduleDir, { recursive: true });
        }
        const db = (0, database_1.getDb)();
        db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(moduleId);
        db.prepare('DELETE FROM module_order WHERE module_id = ?').run(moduleId);
        db.prepare('DELETE FROM module_data WHERE module_id = ?').run(moduleId);
        db.prepare('DELETE FROM modules WHERE id = ?').run(moduleId);
        return { success: true };
    }
    catch (err) {
        return { success: false, error: err.message };
    }
}
function enableModule(moduleId) {
    (0, database_1.getDb)().prepare('UPDATE modules SET enabled = 1 WHERE id = ?').run(moduleId);
}
function disableModule(moduleId) {
    (0, database_1.getDb)().prepare('UPDATE modules SET enabled = 0 WHERE id = ?').run(moduleId);
}
function getInstalledModules() {
    return (0, database_1.getDb)().prepare('SELECT id, name, version, enabled, state FROM modules ORDER BY id').all();
}
// ── Read manifest from source (for permission dialog) ──
function readManifestFromSource(source) {
    try {
        const stat = fs.statSync(source);
        let manifestDir;
        if (stat.isDirectory()) {
            manifestDir = source;
        }
        else if (stat.isFile() && source.endsWith('.zip')) {
            const extracted = extractZip(source);
            manifestDir = extracted;
        }
        else {
            return { error: 'Source must be a directory or .zip file' };
        }
        const manifest = readManifestFromDir(manifestDir);
        return { manifest };
    }
    catch (err) {
        return { error: err.message };
    }
}
// ── Module update ──
function updateModule(moduleId, updateSource) {
    try {
        const moduleDir = path.join(getModulesDir(), moduleId);
        const manifestPath = path.join(moduleDir, 'manifest.json');
        // If an update source is provided, replace files first
        if (updateSource) {
            const sourceResult = readManifestFromSource(updateSource);
            if ('error' in sourceResult) {
                return { success: false, error: sourceResult.error };
            }
            // Validate the update source matches the module being updated
            if (sourceResult.manifest.id !== moduleId) {
                return { success: false, error: `Update source module ID "${sourceResult.manifest.id}" does not match "${moduleId}"` };
            }
            const stat = fs.statSync(updateSource);
            let updateDir;
            if (stat.isDirectory()) {
                updateDir = updateSource;
            }
            else if (stat.isFile() && updateSource.endsWith('.zip')) {
                updateDir = extractZip(updateSource);
            }
            else {
                return { success: false, error: 'Update source must be a directory or .zip file' };
            }
            // Clean old module dir and copy new files
            if (fs.existsSync(moduleDir)) {
                fs.rmSync(moduleDir, { recursive: true });
            }
            copyDirSync(updateDir, moduleDir);
        }
        // Re-read manifest from the (now updated) module directory
        if (!fs.existsSync(manifestPath)) {
            return { success: false, error: 'Module no longer exists after update' };
        }
        const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
        const validation = (0, module_manager_1.validateManifest)(manifest);
        if (!validation.ok) {
            return { success: false, error: validation.error };
        }
        const db = (0, database_1.getDb)();
        // Backup module_data before version change
        const backup = db
            .prepare('SELECT key, value FROM module_data WHERE module_id = ?')
            .all(moduleId);
        // Update all fields in database
        db.prepare(`UPDATE modules SET
      name = ?, version = ?, entry = ?, type = ?,
      description = ?, author = ?, icon = ?, min_natives_version = ?,
      updated_at = datetime('now') WHERE id = ?`)
            .run(validation.manifest.name, validation.manifest.version, validation.manifest.entry, validation.manifest.type, validation.manifest.description || null, validation.manifest.author || null, validation.manifest.icon || null, validation.manifest.minNativesVersion || null, moduleId);
        // Re-insert module_data (data survives version changes)
        // Keys are already scoped by (module_id, key) UNIQUE constraint
        // Sync permissions from new manifest
        db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(moduleId);
        for (const perm of validation.manifest.permissions || []) {
            db.prepare('INSERT INTO module_permissions (module_id, permission, granted) VALUES (?, ?, 0)').run(moduleId, perm);
        }
        return { success: true };
    }
    catch (err) {
        return { success: false, error: err.message };
    }
}
// ── Helpers ──
function readManifestFromDir(dir) {
    const manifestPath = path.join(dir, 'manifest.json');
    if (!fs.existsSync(manifestPath)) {
        throw new Error('Missing manifest.json in module directory');
    }
    const raw = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
    const validation = (0, module_manager_1.validateManifest)(raw);
    if (!validation.ok) {
        throw new Error(validation.error);
    }
    return validation.manifest;
}
function copyDirSync(src, dest) {
    fs.mkdirSync(dest, { recursive: true });
    const entries = fs.readdirSync(src, { withFileTypes: true });
    for (const entry of entries) {
        const srcPath = path.join(src, entry.name);
        const destPath = path.join(dest, entry.name);
        if (entry.isDirectory()) {
            copyDirSync(srcPath, destPath);
        }
        else {
            fs.copyFileSync(srcPath, destPath);
        }
    }
}
function extractZip(zipPath) {
    const zip = new adm_zip_1.default(zipPath);
    const extractDir = path.join(getModulesDir(), `__extract_${Date.now()}`);
    ensureModulesDir();
    // TASK-006: Sanitize each zip entry against path traversal (Zip Slip).
    // Reject entries with '..' or absolute paths.
    const zipEntries = zip.getEntries();
    for (const entry of zipEntries) {
        const entryPath = entry.entryName || '';
        if (entryPath.includes('..') || path.isAbsolute(entryPath)) {
            throw new Error(`Zip Slip detected: rejected unsafe entry "${entryPath}"`);
        }
    }
    zip.extractAllTo(extractDir, true);
    // If zip has a single root directory, use that
    const entries = fs.readdirSync(extractDir);
    if (entries.length === 1) {
        const singleEntry = path.join(extractDir, entries[0]);
        if (fs.statSync(singleEntry).isDirectory()) {
            return singleEntry;
        }
    }
    return extractDir;
}
//# sourceMappingURL=installer.js.map