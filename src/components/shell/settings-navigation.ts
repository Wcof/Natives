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

export function isSettingsView(view?: string): boolean {
  return view === 'settings' || Boolean(view?.startsWith('settings:'));
}

export function getSettingsSection(view?: string): SettingsSection {
  if (!view?.startsWith('settings:')) return 'personal';

  const section = view.slice('settings:'.length);
  if (section === 'engineering' || section === 'engine') {
    return 'runtime';
  }
  if (section === 'overview') {
    return 'personal';
  }
  return SETTINGS_SECTION_SET.has(section)
    ? (section as SettingsSection)
    : 'personal';
}

export function normalizeSettingsTarget(
  target: string,
): `settings:${SettingsSection}` | null {
  if (target !== '__settings__' && !isSettingsView(target)) return null;
  return `settings:${getSettingsSection(target)}`;
}
