/**
 * Shared shiki syntax highlighting utilities.
 * Used by FilePreview, FollowRenderer, and any component needing code highlighting.
 */

// Extension → shiki language identifier
const EXT_TO_LANG: Record<string, string> = {
  ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx', mjs: 'javascript', cjs: 'javascript',
  py: 'python', rs: 'rust', go: 'go', java: 'java', rb: 'ruby', php: 'php',
  c: 'c', cpp: 'cpp', cc: 'cpp', h: 'c', hpp: 'cpp', cs: 'csharp',
  sh: 'bash', bash: 'bash', zsh: 'bash', fish: 'fish',
  json: 'json', json5: 'json5', jsonc: 'json',
  yaml: 'yaml', yml: 'yaml', toml: 'toml', xml: 'xml', html: 'html', htm: 'html',
  css: 'css', scss: 'scss', less: 'less', vue: 'vue', svelte: 'svelte',
  md: 'markdown', markdown: 'markdown', sql: 'sql', graphql: 'graphql',
  swift: 'swift', lua: 'lua', kt: 'kotlin', dart: 'dart', r: 'r',
  ini: 'ini', conf: 'ini', env: 'ini', txt: 'text', log: 'text',
};

export function extToLanguage(ext: string): string {
  return EXT_TO_LANG[ext.toLowerCase()] || 'text';
}

export function detectLanguage(filename: string): string {
  const lastDot = filename.lastIndexOf('.');
  if (lastDot <= 0) return 'text';
  const ext = filename.substring(lastDot + 1).toLowerCase();
  return extToLanguage(ext);
}

/**
 * Highlight code with shiki (lazy-loaded).
 * Returns HTML string with syntax highlighting.
 */
export async function highlightCode(code: string, lang: string): Promise<string> {
  try {
    const { codeToHtml } = await import('shiki');
    return await codeToHtml(code, { lang: lang || 'text', theme: 'dark-plus' });
  } catch {
    return `<pre style="margin:0;font-size:12px;line-height:1.6;font-family:var(--font-mono,monospace);color:var(--text);white-space:pre-wrap;word-break:break-all;background:var(--bg-2,#131410);padding:12px;border-radius:4px">${escapeHtmlForCode(code)}</pre>`;
  }
}

function escapeHtmlForCode(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
