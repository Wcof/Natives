export function pathParts(path) {
  return path.split('/').filter(Boolean);
}

export function fileUri(path) {
  return `file://${path.split('/').map((segment, index) => (index === 0 ? '' : encodeURIComponent(segment))).join('/')}`;
}

export function normalizePathInput(raw) {
  let value = String(raw || '').trim();
  const quoted = value.match(/^("|')(.*)\1$/);
  if (quoted) value = quoted[2];
  return value.replace(/\\([ \\])/g, '$1');
}

export function parentAndName(path) {
  const separator = path.lastIndexOf('/');
  return { parent: separator > 0 ? path.slice(0, separator) : '/', name: path.slice(separator + 1) };
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
