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

/** SQLite is authoritative; the second argument exists only for one-time migration/testing. */
export async function readActiveProject(
  api: ActiveProjectApi | undefined,
  legacyValue: string | null = browserLegacyValue(),
): Promise<string | null> {
  try {
    const stored = await api?.db?.get?.(ACTIVE_PROJECT_KEY);
    if (typeof stored === 'string' && stored.trim()) {
      // Project existence comes from project.list. The file manager intentionally
      // scopes general file access to trusted roots, so probing here would reject
      // a user-selected external volume and erase a valid active project.
      return stored.trim();
    }
  } catch {
    // Fall through to migration; a temporary DB error must not crash the shell.
  }

  const legacy = typeof legacyValue === 'string' && legacyValue.trim() ? legacyValue : null;
  if (legacy) {
    // One-time migration: write to the authoritative DB store and drop the
    // legacy localStorage value. localStorage must never be re-warmed as a
    // parallel authority (Assistant Workspace Integration Design, Section 6).
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
