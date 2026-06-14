import * as fs from 'fs';
import * as path from 'path';
import AdmZip from 'adm-zip';
import { getDb } from './database';
import { validateManifest, type Manifest } from './module-manager';

const MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');

function ensureModulesDir(): void {
  if (!fs.existsSync(MODULES_DIR)) {
    fs.mkdirSync(MODULES_DIR, { recursive: true });
  }
}

export function installModule(source: string): { success: true; moduleId: string } | { success: false; error: string } {
  ensureModulesDir();

  let moduleDir: string | null = null;
  let manifest: Manifest;

  try {
    // Determine if source is a directory or ZIP
    const stat = fs.statSync(source);

    if (stat.isDirectory()) {
      manifest = readManifestFromDir(source);
      const dest = path.join(MODULES_DIR, manifest.id);
      if (fs.existsSync(dest)) {
        fs.rmSync(dest, { recursive: true });
      }
      copyDirSync(source, dest);
      moduleDir = dest;
    } else if (stat.isFile() && source.endsWith('.zip')) {
      const extracted = extractZip(source);
      manifest = readManifestFromDir(extracted);
      const dest = path.join(MODULES_DIR, manifest.id);
      if (fs.existsSync(dest)) {
        fs.rmSync(dest, { recursive: true });
      }
      fs.renameSync(extracted, dest);
      moduleDir = dest;
    } else {
      return { success: false, error: 'Source must be a directory or .zip file' };
    }

    // Register in database
    const db = getDb();
    db.prepare(`
      INSERT INTO modules (id, name, version, entry, type, description, author, icon, min_natives_version, state, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'installed', datetime('now'), datetime('now'))
      ON CONFLICT(id) DO UPDATE SET
        name = excluded.name, version = excluded.version, entry = excluded.entry,
        description = excluded.description, author = excluded.author, icon = excluded.icon,
        min_natives_version = excluded.min_natives_version, state = 'installed',
        updated_at = datetime('now')
    `).run(
      manifest.id, manifest.name, manifest.version, manifest.entry, manifest.type,
      manifest.description || null, manifest.author || null, manifest.icon || null,
      manifest.minNativesVersion || null,
    );

    // Register permissions
    db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(manifest.id);
    for (const perm of manifest.permissions || []) {
      db.prepare('INSERT INTO module_permissions (module_id, permission, granted) VALUES (?, ?, 0)').run(manifest.id, perm);
    }

    // Ensure order entry
    db.prepare('INSERT OR IGNORE INTO module_order (module_id, sort_order) VALUES (?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM module_order))').run(manifest.id);

    return { success: true, moduleId: manifest.id };
  } catch (err) {
    // Clean up on failure
    if (moduleDir && fs.existsSync(moduleDir)) {
      fs.rmSync(moduleDir, { recursive: true });
    }
    return { success: false, error: (err as Error).message };
  }
}

export function uninstallModule(moduleId: string): { success: true } | { success: false; error: string } {
  try {
    const moduleDir = path.join(MODULES_DIR, moduleId);
    if (fs.existsSync(moduleDir)) {
      fs.rmSync(moduleDir, { recursive: true });
    }

    const db = getDb();
    db.prepare('DELETE FROM module_permissions WHERE module_id = ?').run(moduleId);
    db.prepare('DELETE FROM module_order WHERE module_id = ?').run(moduleId);
    db.prepare('DELETE FROM module_data WHERE module_id = ?').run(moduleId);
    db.prepare('DELETE FROM modules WHERE id = ?').run(moduleId);

    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
}

export function enableModule(moduleId: string): void {
  getDb().prepare('UPDATE modules SET enabled = 1 WHERE id = ?').run(moduleId);
}

export function disableModule(moduleId: string): void {
  getDb().prepare('UPDATE modules SET enabled = 0 WHERE id = ?').run(moduleId);
}

export function getInstalledModules(): Array<{
  id: string; name: string; version: string; enabled: number; state: string;
}> {
  return getDb().prepare('SELECT id, name, version, enabled, state FROM modules ORDER BY id').all() as Array<{
    id: string; name: string; version: string; enabled: number; state: string;
  }>;
}

// ── Module update ──

export function updateModule(moduleId: string): { success: true } | { success: false; error: string } {
  try {
    const manifestPath = path.join(MODULES_DIR, moduleId, 'manifest.json');
    if (!fs.existsSync(manifestPath)) {
      return { success: false, error: 'Module no longer exists' };
    }

    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
    const validation = validateManifest(manifest);
    if (!validation.ok) {
      return { success: false, error: validation.error };
    }

    const db = getDb();

    // Backup module_data before version change
    const backup = db
      .prepare('SELECT key, value FROM module_data WHERE module_id = ?')
      .all(moduleId) as Array<{ key: string; value: string }>;

    // Update version in database
    db.prepare('UPDATE modules SET version = ?, updated_at = datetime(\'now\') WHERE id = ?')
      .run(validation.manifest.version, moduleId);

    // Re-insert module_data (data survives version changes)
    // Keys are already scoped by (module_id, key) UNIQUE constraint

    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
}

// ── Helpers ──

function readManifestFromDir(dir: string): Manifest {
  const manifestPath = path.join(dir, 'manifest.json');
  if (!fs.existsSync(manifestPath)) {
    throw new Error('Missing manifest.json in module directory');
  }
  const raw = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
  const validation = validateManifest(raw);
  if (!validation.ok) {
    throw new Error(validation.error);
  }
  return validation.manifest;
}

function copyDirSync(src: string, dest: string): void {
  fs.mkdirSync(dest, { recursive: true });
  const entries = fs.readdirSync(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDirSync(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}

function extractZip(zipPath: string): string {
  const zip = new AdmZip(zipPath);
  const extractDir = path.join(MODULES_DIR, `__extract_${Date.now()}`);
  zip.extractAllTo(extractDir, true);

  // If zip has a single root directory, use that
  const entries = fs.readdirSync(extractDir);
  if (entries.length === 1) {
    const singleEntry = path.join(extractDir, entries[0]!);
    if (fs.statSync(singleEntry).isDirectory()) {
      return singleEntry;
    }
  }

  return extractDir;
}
