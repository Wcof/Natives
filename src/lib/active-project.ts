export interface ActiveProjectApi {
  db?: {
    get?: (key: string) => Promise<unknown>;
    set?: (key: string, value: unknown) => Promise<unknown>;
    delete?: (key: string) => Promise<unknown>;
  };
}

const ACTIVE_PROJECT_KEY = 'active_project_path';
const LEGACY_KEY = 'natives:active_project_path';

function browserLegacyValue(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(LEGACY_KEY);
  } catch {
    return null;
  }
}

/**
 * Validate that a path exists and is a directory.
 * Uses the filesystem API when available inside Tauri.
 * Falls back to trusting the stored value in browser/SSR context.
 */
async function pathExists(path: string): Promise<boolean> {
  if (typeof window === 'undefined') return true;
  try {
    const api = (window as any).nativesAPI;
    if (api?.fs?.listDir) {
      await api.fs.listDir(path);
      return true;
    }
  } catch {
    return false; // listDir threw -> path does not exist
  }
  return true; // no fs API available, trust the value
}

/** SQLite is authoritative; the second argument exists only for one-time migration/testing. */
export async function readActiveProject(
  api: ActiveProjectApi | undefined,
  legacyValue: string | null = browserLegacyValue(),
): Promise<string | null> {
  try {
    const stored = await api?.db?.get?.(ACTIVE_PROJECT_KEY);
    if (typeof stored === 'string' && stored.trim()) {
      // Clean up stale entries (directory no longer exists)
      if (!(await pathExists(stored.trim()))) {
        await writeActiveProject(api, null);
        return null;
      }
      return stored.trim();
    }
  } catch {
    // Fall through to migration; a temporary DB error must not crash the shell.
  }

  const legacy = typeof legacyValue === 'string' && legacyValue.trim() ? legacyValue : null;
  if (legacy) {
    // One-time migration: write to the authoritative DB store and drop the
    // legacy localStorage value. localStorage must never be re-warmed as a
    // parallel authority (Assistant Workspace Integration Design §6).
    await writeActiveProject(api, legacy);
    try { window.localStorage.removeItem(LEGACY_KEY); } catch { /* ignore */ }
    return legacy;
  }
  return null;
}

export async function writeActiveProject(
  api: ActiveProjectApi | undefined,
  projectPath: string | null,
): Promise<void> {
  // SQLite is the sole authoritative store. localStorage is migration-only.
  if (projectPath?.trim()) {
    await api?.db?.set?.(ACTIVE_PROJECT_KEY, projectPath.trim());
  } else {
    await api?.db?.delete?.(ACTIVE_PROJECT_KEY);
  }
}
