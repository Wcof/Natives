import * as fs from 'fs';
import * as path from 'path';
import { z } from 'zod';
import { getDb } from './database';

const MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');

// ── Manifest Schema ──

const ManifestSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  version: z.string().min(1),
  entry: z.string().min(1),
  type: z.enum(['web', 'mcp']).default('web'),
  description: z.string().optional(),
  author: z.string().optional(),
  icon: z.string().optional(),
  minNativesVersion: z.string().optional(),
  permissions: z.array(z.string()).default([]),
  api: z.object({
    bridge: z.string().optional(),
  }).optional(),
  lifecycle: z.object({
    heartbeatInterval: z.number().optional(),
    loadTimeout: z.number().optional(),
  }).optional(),
  i18n: z.object({
    name: z.record(z.string()).optional(),
    description: z.record(z.string()).optional(),
  }).optional(),
});

export type Manifest = z.infer<typeof ManifestSchema>;

export interface ScanResult {
  moduleId: string;
  manifest: Manifest | null;
  error?: string;
}

// ── Version compatibility ──

function parseVersion(v: string): { major: number; minor: number; patch: number } {
  const parts = v.split('.');
  return {
    major: parseInt(parts[0] || '0', 10),
    minor: parseInt(parts[1] || '0', 10),
    patch: parseInt(parts[2] || '0', 10),
  };
}

const NATIVES_VERSION = '0.1.0';

export function checkCompatibility(
  minVersion: string | undefined,
): { compatible: boolean; warning?: string } {
  if (!minVersion) return { compatible: true };

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

export function validateManifest(data: unknown): { ok: true; manifest: Manifest } | { ok: false; error: string } {
  const result = ManifestSchema.safeParse(data);
  if (!result.success) {
    const errors = result.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`).join('; ');
    return { ok: false, error: `Invalid manifest: ${errors}` };
  }
  return { ok: true, manifest: result.data };
}

// ── Directory scanning ──

export function scanModules(): ScanResult[] {
  if (!fs.existsSync(MODULES_DIR)) {
    fs.mkdirSync(MODULES_DIR, { recursive: true });
    return [];
  }

  const entries = fs.readdirSync(MODULES_DIR, { withFileTypes: true });
  const results: ScanResult[] = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const moduleDir = path.join(MODULES_DIR, entry.name);
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
    } catch (err) {
      results.push({ moduleId: entry.name, manifest: null, error: `Failed to parse manifest.json: ${(err as Error).message}` });
    }
  }

  return results;
}

// ── Sync scan results to database ──

export function syncModulesToDb(): void {
  const db = getDb();
  const results = scanModules();

  for (const result of results) {
    if (!result.manifest) continue;

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
  const validIds = results.filter((r) => r.manifest).map((r) => r.manifest!.id);
  if (validIds.length > 0) {
    db.prepare(`DELETE FROM modules WHERE id NOT IN (${validIds.map(() => '?').join(',')})`).run(...validIds);
  }
}
