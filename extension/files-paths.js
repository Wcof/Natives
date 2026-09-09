export function pathParts(path) {
  return path.split('/').filter(Boolean);
}

export function fileUri(path) {
  return `file://${path.split('/').map((segment, index) => (index === 0 ? '' : encodeURIComponent(segment))).join('/')}`;
}

export const MARKDOWN_EXTENSIONS = /\.(md|markdown|mdx)$/i;

export function normalizePathInput(raw) {
  let value = String(raw || '').trim();
  const quoted = value.match(/^("|')(.*)\1$/);
  if (quoted) value = quoted[2];
  value = value.replace(/\\([ \\])/g, '$1');
  if (/^file:[/\\]*/i.test(value)) {
    try {
      let urlStr = value;
      if (!/^file:\/\//i.test(urlStr)) {
        urlStr = urlStr.replace(/^file:[/\\]*/i, 'file:///');
      }
      const url = new URL(urlStr);
      let pathname = decodeURIComponent(url.pathname);
      if (/^\/[A-Za-z]:(\/|$)/.test(pathname)) {
        pathname = pathname.slice(1);
      }
      if (url.host && url.host !== 'localhost') {
        pathname = `//${url.host}${pathname}`;
      }
      value = pathname;
    } catch {
      // Keep value if URL parsing fails
    }
  } else if (value.includes('%')) {
    try {
      value = decodeURIComponent(value);
    } catch {
      // Keep value if decode fails
    }
  }
  return value;
}

export function parseOpenMarkdownUrl(rawUrl) {
  if (typeof rawUrl !== 'string' || !rawUrl.trim()) return null;
  const trimmed = rawUrl.trim();
  if (!/^file:\/\/\/.+/i.test(trimmed)) return null;
  try {
    const url = new URL(trimmed);
    if (url.protocol !== 'file:') return null;
    if (!MARKDOWN_EXTENSIONS.test(url.pathname)) return null;
    const normalized = normalizePathInput(trimmed);
    if (!normalized || (!normalized.startsWith('/') && !/^[A-Za-z]:[/\\]/.test(normalized))) {
      return null;
    }
    return normalized;
  } catch {
    return null;
  }
}

export function parentAndName(path) {
  const separator = path.lastIndexOf('/');
  if (separator <= 0) return { parent: '/', name: path.slice(separator + 1) };
  const parent = path.slice(0, separator);
  return { parent: /^[A-Za-z]:$/.test(parent) ? `${parent}/` : parent, name: path.slice(separator + 1) };
}

export function renderFilesBreadcrumb(box, path, navigate) {
  if (!box) return;
  box.replaceChildren();
  if (!path) return;
  const parts = pathParts(path);
  let value = path.startsWith('/') ? '/' : '';
  const root = document.createElement('button');
  root.className = `crumb${parts.length === 0 ? ' last' : ''}`;
  root.textContent = path.startsWith('/') ? '/' : path;
  root.onclick = () => path.startsWith('/') && navigate('/');
  box.append(root);
  parts.forEach((part, index) => {
    value = `${value.replace(/\/$/, '')}/${part}`;
    const crumbPath = value;
    const separator = document.createElement('span');
    separator.className = 'crumb-separator';
    separator.textContent = ' / ';
    box.append(separator);
    const button = document.createElement('button');
    button.className = `crumb${index === parts.length - 1 ? ' last' : ''}`;
    button.textContent = part;
    button.onclick = () => navigate(crumbPath);
    box.append(button);
  });
}
