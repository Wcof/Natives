export const SETTINGS_SECTIONS = [
  'personal',
  'general',
  'appearance',
  'providers',
  'runtime',
  'plugins',
] as const;

export type SettingsSection = typeof SETTINGS_SECTIONS[number];

export const DEFAULT_SETTINGS_VIEW = 'settings:personal' as const;

const SETTINGS_SECTION_SET = new Set<string>(SETTINGS_SECTIONS);

/**
 * Stable control-id allowlist per section (W8). Deep links may only target
 * these ids — an unknown anchor fails safe to the section root instead of
 * executing an arbitrary selector.
 */
const SETTINGS_CONTROL_IDS: Record<SettingsSection, ReadonlySet<string>> = {
  personal: new Set(['overview', 'account', 'usage']),
  general: new Set(['language', 'theme', 'notifications']),
  appearance: new Set(['theme', 'font-size', 'reduced-motion']),
  providers: new Set(['routing', 'keys', 'presets']),
  runtime: new Set(['engine', 'paths', 'logs']),
  plugins: new Set(['installed', 'marketplace']),
};

export function isSettingsView(view?: string): boolean {
  return view === 'settings' || Boolean(view?.startsWith('settings:'));
}

export function getSettingsSection(view?: string): SettingsSection {
  if (!view?.startsWith('settings:')) return 'personal';

  // Strip a #control-id fragment before resolving the section.
  const raw = view.slice('settings:'.length);
  const sectionPart = raw.split('#')[0] ?? '';
  if (sectionPart === 'engineering' || sectionPart === 'engine') {
    return 'runtime';
  }
  if (sectionPart === 'overview') {
    return 'personal';
  }
  return SETTINGS_SECTION_SET.has(sectionPart)
    ? (sectionPart as SettingsSection)
    : 'personal';
}

/**
 * W8: resolve `settings:<section>#<stable-control-id>` (and legacy aliases)
 * into a navigation target. Returns `settings:<section>` — the deep-link
 * fragment is validated separately by {@link normalizeSettingsControlId}.
 */
export function normalizeSettingsTarget(
  target: string,
): `settings:${SettingsSection}` | null {
  if (target !== '__settings__' && !isSettingsView(target)) return null;
  return `settings:${getSettingsSection(target)}`;
}

/**
 * W8: validate the `#<stable-control-id>` fragment of a settings deep link.
 * Unknown anchors return null (fail safe — never run an arbitrary selector).
 * `zh`/`en` language keywords map to the `language` control id.
 */
export function normalizeSettingsControlId(
  section: SettingsSection,
  rawFragment?: string,
): string | null {
  if (!rawFragment) return null;
  const fragment = rawFragment.startsWith('#') ? rawFragment.slice(1) : rawFragment;
  const trimmed = fragment.trim().toLowerCase();
  if (trimmed === '') return null;
  if (trimmed === 'zh' || trimmed === 'en' || trimmed === 'zh-cn' || trimmed === 'en-us') {
    return SETTINGS_CONTROL_IDS[section].has('language') ? 'language' : null;
  }
  return SETTINGS_CONTROL_IDS[section].has(trimmed) ? trimmed : null;
}

/** Resolve a settings view into `{ section, controlId }` (both validated). */
export function resolveSettingsView(
  view?: string,
): { section: SettingsSection; controlId: string | null } | null {
  if (!isSettingsView(view)) return null;
  const section = getSettingsSection(view);
  const fragment = view?.includes('#') ? view.split('#')[1] : undefined;
  return { section, controlId: normalizeSettingsControlId(section, fragment) };
}
