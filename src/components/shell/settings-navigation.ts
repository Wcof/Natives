export const SETTINGS_SECTIONS = [
  'general',
  'appearance',
  'providers',
  'runtime',
  'engine',
  'plugins',
] as const;

export type SettingsSection = typeof SETTINGS_SECTIONS[number];

export const DEFAULT_SETTINGS_VIEW = 'settings:general' as const;

const SETTINGS_SECTION_SET = new Set<string>(SETTINGS_SECTIONS);

export function isSettingsView(view?: string): boolean {
  return view === 'settings' || Boolean(view?.startsWith('settings:'));
}

export function getSettingsSection(view?: string): SettingsSection {
  if (!view?.startsWith('settings:')) return 'general';

  const section = view.slice('settings:'.length);
  return SETTINGS_SECTION_SET.has(section)
    ? (section as SettingsSection)
    : 'general';
}

export function normalizeSettingsTarget(
  target: string,
): `settings:${SettingsSection}` | null {
  if (target !== '__settings__' && !isSettingsView(target)) return null;
  return `settings:${getSettingsSection(target)}`;
}