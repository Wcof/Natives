

export const TEXT_KINDS = new Set(['text']);
export const TEXT_EXTENSIONS = /\.(txt|md|mdx|markdown|json|jsonc|yaml|yml|toml|xml|csv|log|ini|cfg|conf|env|gitignore|dockerignore|editorconfig|graphql|gql|sql|vue|svelte|astro|ts|tsx|js|jsx|mjs|cjs|py|pyw|rb|rs|go|java|c|cpp|h|hpp|cs|swift|kt|kts|sh|bash|zsh|fish|ps1|bat|cmd|php|scala|html|htm|css|scss|sass|less)$/i;
export const TEXT_FILENAMES = new Set(['Dockerfile', 'Makefile', 'Gemfile', 'Rakefile', 'CHANGELOG', 'README', 'LICENSE', 'VERSION', 'Procfile', '.env', '.gitignore', '.dockerignore', '.editorconfig']);

export function htmlEscape(value) { return String(value).replace(/[&<>\"]/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[character])); }

export function iconElement(name, className = '') { const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); svg.setAttribute('class', className ? `icon ${className}` : 'icon'); svg.setAttribute('aria-hidden', 'true'); const use = document.createElementNS('http://www.w3.org/2000/svg', 'use'); use.setAttribute('href', `#i-${name}`); svg.append(use); return svg; }

const DOC_TYPES = {
  pdf: ['PDF', 5, '#E64A3B', '#C23E31'],
  md: ['MD', 7, '#3B82F6', '#2E68C8'], markdown: ['MD', 7, '#3B82F6', '#2E68C8'], mdx: ['MDX', 5, '#3B82F6', '#2E68C8'],
  html: ['<>', 7, '#E8662A', '#C4541F'], htm: ['<>', 7, '#E8662A', '#C4541F'],
  css: ['CSS', 5, '#2D6FD6', '#2459AC'], scss: ['SCSS', 4, '#CF649A', '#A94E7C'], less: ['LESS', 4, '#2D5B8A', '#244A70'],
  json: ['{ }', 7, '#A6824C', '#856A3E'], json5: ['{ }', 7, '#A6824C', '#856A3E'], jsonl: ['{ }', 7, '#A6824C', '#856A3E'],
  yml: ['YML', 5, '#9C5BD6', '#7E49AC'], yaml: ['YAML', 4.2, '#9C5BD6', '#7E49AC'], toml: ['TOML', 4.2, '#9C5BD6', '#7E49AC'],
  xml: ['XML', 5, '#5E8A3E', '#4A6E31'], svg: ['SVG', 5, '#E8923A', '#C4761F'],
  csv: ['CSV', 5, '#1FAE5A', '#188F4A'], tsv: ['TSV', 5, '#1FAE5A', '#188F4A'],
  sql: ['SQL', 5, '#C77D2E', '#A4661F'],
  doc: ['DOC', 5, '#2B579A', '#21457A'], docx: ['DOC', 5, '#2B579A', '#21457A'],
  xls: ['XLS', 5, '#1D6F42', '#155632'], xlsx: ['XLS', 5, '#1D6F42', '#155632'],
  ppt: ['PPT', 5, '#C43E1C', '#9E3216'], pptx: ['PPT', 5, '#C43E1C', '#9E3216'],
  log: ['LOG', 5, '#7A8290', '#626977'], txt: ['TXT', 5, '#7A8290', '#626977'],
  ini: ['INI', 5, '#7A8290', '#626977'], conf: ['CONF', 4, '#7A8290', '#626977'],
  env: ['ENV', 5, '#A6824C', '#856A3E'],
};
const CODE_BADGES = {
  js: ['JS', 8, '#F0DB4F', '#1A1A1A'], mjs: ['JS', 8, '#F0DB4F', '#1A1A1A'], cjs: ['JS', 8, '#F0DB4F', '#1A1A1A'],
  jsx: ['JSX', 6, '#61DAFB', '#1A1A1A'],
  ts: ['TS', 8, '#3178C6', '#fff'], tsx: ['TSX', 6, '#3178C6', '#fff'],
  py: ['PY', 8, '#3776AB', '#FFE05B'],
  go: ['GO', 7.5, '#00ACD7', '#fff'], rs: ['RS', 8, '#CE7B43', '#fff'],
  java: ['JV', 8, '#E7700E', '#fff'], kt: ['KT', 8, '#A97BFF', '#fff'],
  rb: ['RB', 8, '#CC342D', '#fff'], php: ['PHP', 6, '#7A86B8', '#fff'],
  c: ['C', 9, '#5C6BC0', '#fff'], h: ['H', 9, '#5C6BC0', '#fff'], cpp: ['C++', 6, '#5C6BC0', '#fff'], cc: ['C++', 6, '#5C6BC0', '#fff'],
  hpp: ['H++', 6, '#5C6BC0', '#fff'], cs: ['C#', 7, '#68217A', '#fff'],
  vue: ['Vue', 6, '#41B883', '#fff'], swift: ['SW', 8, '#F05138', '#fff'], dart: ['DT', 8, '#0A9EDC', '#fff'],
  lua: ['Lua', 6, '#000080', '#fff'],
  sh: ['>_', 8, '#33373D', '#3FD46A'], bash: ['>_', 8, '#33373D', '#3FD46A'], zsh: ['>_', 8, '#33373D', '#3FD46A'],
};
const ARCHIVE_EXT = new Set(['zip', 'rar', '7z', 'gz', 'tar', 'tgz', 'bz2', 'xz', 'dmg', 'iso']);
const AUDIO_EXT = new Set(['mp3', 'wav', 'm4a', 'flac', 'aac', 'ogg', 'wma']);
const VIDEO_EXT = new Set(['mp4', 'mov', 'webm', 'mkv', 'avi', 'flv', 'wmv']);
const IMAGE_EXT = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif', 'heic', 'heif', 'tiff', 'tif']);

let iconTheme = 'archive';
export function setIconTheme(theme) { iconTheme = theme === 'archive' || theme === 'volt' ? theme : 'archive'; }

function parseSvg(xml) {
  if (typeof DOMParser !== 'undefined') {
    try {
      const doc = new DOMParser().parseFromString(xml, 'image/svg+xml');
      const el = doc.documentElement;
      if (el && el.nodeName !== 'parsererror' && el.namespaceURI === 'http://www.w3.org/2000/svg') {
        return document.importNode ? document.importNode(el, true) : el;
      }
    } catch {}
  }
  const div = document.createElement('div');
  div.innerHTML = xml;
  return div.firstElementChild || document.createElementNS('http://www.w3.org/2000/svg', 'svg');
}

function richIcon(item, size = 20) {
  if (item.isDir) {
    const folderColor = iconTheme === 'archive' ? '#c0714f' : '#6d8bff';
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><path d="M3.6 5.5h4.4a1.2 1.2 0 0 1 .85.35l1.3 1.3a1.2 1.2 0 0 0 .85.35H20a1.6 1.6 0 0 1 1.6 1.6v8.45A1.6 1.6 0 0 1 20 19.1H4A1.6 1.6 0 0 1 2.4 17.5V6.7A1.2 1.2 0 0 1 3.6 5.5z" fill="${folderColor}"/></svg>`);
  }
  const ext = (item.name.split('.').pop() || '').toLowerCase();
  if (DOC_TYPES[ext]) {
    const [l, fs, c, f] = DOC_TYPES[ext];
    const safeLabel = htmlEscape(l);
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><path d="M5 3.6A1.6 1.6 0 0 1 6.6 2H14l5 5v11.4A1.6 1.6 0 0 1 17.4 20H6.6A1.6 1.6 0 0 1 5 18.4z" fill="${c}"/><path d="M14 2l5 5h-3.4A1.6 1.6 0 0 1 14 5.4z" fill="${f}"/><text x="11.6" y="16.6" text-anchor="middle" font-family="-apple-system,'Helvetica Neue',Arial,sans-serif" font-weight="800" font-size="${fs}" letter-spacing="0.1" fill="#fff">${safeLabel}</text></svg>`);
  }
  if (CODE_BADGES[ext]) {
    const [l, fs, c, t] = CODE_BADGES[ext];
    const safeLabel = htmlEscape(l);
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="18" height="18" rx="5" fill="${c}"/><text x="12" y="15.7" text-anchor="middle" font-family="-apple-system,'Helvetica Neue',Arial,sans-serif" font-weight="800" font-size="${fs}" fill="${t}">${safeLabel}</text></svg>`);
  }
  if (ARCHIVE_EXT.has(ext)) {
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect x="4" y="3.5" width="16" height="17" rx="2.2" fill="#E0A23B"/><rect x="4" y="3.5" width="16" height="17" rx="2.2" fill="#000" opacity="0.06"/><rect x="10.6" y="3.5" width="2.8" height="17" fill="#C8862A"/><rect x="10.6" y="8" width="2.8" height="3" rx="0.5" fill="#fff8e6"/><rect x="11.4" y="11" width="1.2" height="3.4" rx="0.6" fill="#fff8e6"/></svg>`);
  }
  if (item.kind === 'audio' || AUDIO_EXT.has(ext)) {
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="18" height="18" rx="5" fill="#E0457B"/><g stroke="#fff" stroke-width="1.5" stroke-linecap="round"><line x1="8" y1="10" x2="8" y2="14"/><line x1="10.7" y1="8" x2="10.7" y2="16"/><line x1="13.3" y1="9.5" x2="13.3" y2="14.5"/><line x1="16" y1="7.5" x2="16" y2="16.5"/></g></svg>`);
  }
  if (item.kind === 'video' || VIDEO_EXT.has(ext)) {
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="18" height="18" rx="5" fill="#7C5CE0"/><path d="M10 8.5l5 3.5-5 3.5z" fill="#fff"/></svg>`);
  }
  if (item.kind === 'image' || IMAGE_EXT.has(ext)) {
    return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="18" height="18" rx="5" fill="#2BB6A3"/><circle cx="9" cy="9.5" r="1.6" fill="#fff"/><path d="M5 16l3.5-3.5 2.5 2.5L14.5 11 19 16z" fill="#fff"/></svg>`);
  }
  const labelText = ext ? ext.slice(0, 4).toUpperCase() : (item.name.startsWith('.') ? item.name.slice(1, 5).toUpperCase() : '');
  const safeLabel = htmlEscape(labelText);
  const labelSvg = safeLabel ? `<text x="11.6" y="16.6" text-anchor="middle" font-family="-apple-system,'Helvetica Neue',Arial,sans-serif" font-weight="800" font-size="${safeLabel.length > 3 ? 4.2 : 5}" letter-spacing="0.1" fill="#fff">${safeLabel}</text>` : '';
  return parseSvg(`<svg xmlns="http://www.w3.org/2000/svg" class="rich-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><path d="M5 3.6A1.6 1.6 0 0 1 6.6 2H14l5 5v11.4A1.6 1.6 0 0 1 17.4 20H6.6A1.6 1.6 0 0 1 5 18.4z" fill="#7A8290"/><path d="M14 2l5 5h-3.4A1.6 1.6 0 0 1 14 5.4z" fill="#626977"/>${labelSvg}</svg>`);
}

export function entryIcon(item) {
  return richIcon(item, 20);
}

// Derive a coarse kind from the file name when the Host entry lacks one.
export function kindFromName(name) {
  const ext = (String(name || '').split('.').pop() || '').toLowerCase();
  if (!ext || ext === String(name || '').toLowerCase()) return 'other';
  if (IMAGE_EXT.has(ext)) return 'image';
  if (VIDEO_EXT.has(ext)) return 'video';
  if (AUDIO_EXT.has(ext)) return 'audio';
  if (ext === 'pdf') return 'pdf';
  if (ARCHIVE_EXT.has(ext)) return 'archive';
  if (TEXT_EXTENSIONS.test(`.${ext}`) || TEXT_KINDS.has(ext)) return 'text';
  return 'other';
}

export function isTextFile(item) { return TEXT_KINDS.has(item.kind) || TEXT_EXTENSIONS.test(item.name) || TEXT_FILENAMES.has(item.name); }
