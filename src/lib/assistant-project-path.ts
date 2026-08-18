/**
 * Normalize project paths for Renderer-side identity comparisons.
 *
 * The Host owns filesystem canonicalization. This only makes equivalent
 * serialized path forms (whitespace and trailing slashes) compare equally
 * when legacy daemon conversations are projected into the sidebar.
 */
export function normalizeAssistantProjectPath(path: string): string {
  const trimmed = path.trim();
  if (trimmed === '/') return trimmed;
  return trimmed.replace(/\/+$/, '');
}
