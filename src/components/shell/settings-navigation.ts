export const SETTINGS_SECTIONS = [
  'overview',
  'general',
  'appearance',
  'providers',
  'runtime',
  'engineering',
  'engine',
  'plugins',
] as const;

export type SettingsSection = typeof SETTINGS_SECTIONS[number];

export const DEFAULT_SETTINGS_VIEW = 'settings:overview' as const;

const SETTINGS_SECTION_SET = new Set<string>(SETTINGS_SECTIONS);

export function isSettingsView(view?: string): boolean {
  return view === 'settings' || Boolean(view?.startsWith('settings:'));
}

export function getSettingsSection(view?: string): SettingsSection {
  if (!view?.startsWith('settings:')) return 'overview';

  const section = view.slice('settings:'.length);
  return SETTINGS_SECTION_SET.has(section)
    ? (section as SettingsSection)
    : 'overview';
}

export function normalizeSettingsTarget(
  target: string,
): `settings:${SettingsSection}` | null {
  if (target !== '__settings__' && !isSettingsView(target)) return null;
  return `settings:${getSettingsSection(target)}`;
}
