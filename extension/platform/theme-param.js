// ADR-0048 D5: the DSH plugin entry passes the host appearance as a URL
// parameter (`?theme=archive|volt`). It overrides the appearance of this one
// view only: the saved `natives-theme` preference is never rewritten by it, and
// an unrecognised value falls back to whatever the user already saved.
export const THEMES = ['archive', 'volt'];

export function themeParam(search) {
  if (typeof search !== 'string') return null;
  try {
    const value = new URLSearchParams(search).get('theme');
    return THEMES.includes(value) ? value : null;
  } catch {
    return null;
  }
}

export function resolveTheme(stored, search) {
  return themeParam(search) ?? (THEMES.includes(stored) ? stored : 'archive');
}
