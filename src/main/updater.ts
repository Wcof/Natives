import { getDb } from './database';
import { checkCompatibility } from './module-manager';

// ── Version Comparison ──

export function compareVersions(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] || 0;
    const nb = pb[i] || 0;
    if (na > nb) return 1;
    if (na < nb) return -1;
  }
  return 0;
}

export interface ModuleUpdateInfo {
  moduleId: string;
  name: string;
  currentVersion: string;
  newVersion: string;
  status: 'up-to-date' | 'update-available' | 'incompatible';
  warning?: string;
}

// ── Scan for updates by re-reading manifest ──

import * as fs from 'fs';
import * as path from 'path';

const MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');

export function checkForUpdates(): ModuleUpdateInfo[] {
  const db = getDb();
  const modules = db.prepare('SELECT id, name, version FROM modules').all() as Array<{
    id: string;
    name: string;
    version: string;
  }>;

  const results: ModuleUpdateInfo[] = [];

  for (const mod of modules) {
    const manifestPath = path.join(MODULES_DIR, mod.id, 'manifest.json');
    if (!fs.existsSync(manifestPath)) continue;

    try {
      const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
      const newVersion = manifest.version;
      if (!newVersion) continue;

      const cmp = compareVersions(newVersion, mod.version);
      if (cmp > 0) {
        // Version increased — check compatibility
        const compat = checkCompatibility(manifest.minNativesVersion);
        results.push({
          moduleId: mod.id,
          name: mod.name,
          currentVersion: mod.version,
          newVersion,
          status: compat.compatible ? 'update-available' : 'incompatible',
          warning: compat.warning,
        });
      } else {
        results.push({
          moduleId: mod.id,
          name: mod.name,
          currentVersion: mod.version,
          newVersion: mod.version,
          status: 'up-to-date',
        });
      }
    } catch {
      // Can't read manifest, skip
    }
  }

  return results;
}

// ── Perform Update ──

export function applyUpdate(moduleId: string): { success: true } | { success: false; error: string } {
  try {
    const db = getDb();
    const manifestPath = path.join(MODULES_DIR, moduleId, 'manifest.json');
    if (!fs.existsSync(manifestPath)) {
      return { success: false, error: 'Module no longer exists' };
    }

    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));

    // Backup module_data
    const moduleData = db
      .prepare('SELECT key, value FROM module_data WHERE module_id = ?')
      .all(moduleId) as Array<{ key: string; value: string }>;

    // Update version in database
    db.prepare('UPDATE modules SET version = ?, updated_at = datetime(\'now\') WHERE id = ?')
      .run(manifest.version, moduleId);

    // Re-insert module_data (IDs might have changed, but data stays)
    // Module data is keyed by (module_id, key) so it survives version changes

    return { success: true };
  } catch (err) {
    return { success: false, error: (err as Error).message };
  }
}

// ── Bridge API: natives version ──

export const NATIVES_VERSION = '0.1.0';
