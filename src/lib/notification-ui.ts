import { type ClassifiedError, classifyError } from './error-classifier';

// ── Toast Notifications ──

let toastContainer: HTMLDivElement | null = null;

function ensureToastContainer(): HTMLDivElement {
  if (!toastContainer) {
    toastContainer = document.createElement('div');
    toastContainer.style.cssText =
      'position:fixed;bottom:24px;left:50%;transform:translateX(-50%);z-index:200;display:flex;flex-direction:column;gap:8px;pointer-events:none;';
    document.body.appendChild(toastContainer);
  }
  return toastContainer;
}

export function showToast(message: string, level: 'info' | 'error' = 'info'): void {
  const container = ensureToastContainer();
  const el = document.createElement('div');
  el.style.cssText = `
    padding:10px 18px;border-radius:10px;font-size:13px;
    background:var(--bg-3,#1c1e17);border:1px solid ${level === 'error' ? '#d9534f' : 'var(--border,#262920)'};
    color:var(--text,#f2f2ea);opacity:0;transition:opacity 0.2s ease;pointer-events:auto;
  `;
  el.textContent = message;
  container.appendChild(el);

  // Fade in
  requestAnimationFrame(() => { el.style.opacity = '1'; });

  // Auto-hide after 2200ms
  setTimeout(() => {
    el.style.opacity = '0';
    setTimeout(() => el.remove(), 200);
  }, 2200);
}

export function showErrorToast(error: unknown, moduleId?: string): void {
  const classified = classifyError(error, moduleId);
  showToast(classified.userMessage, 'error');
}

// ── Notification Center ──

export function renderNotificationCenter(
  container: HTMLElement,
  notifications: Array<{
    id: number;
    moduleId?: string;
    title: string;
    body?: string;
    level: string;
    read: number;
    createdAt: string;
  }>,
  onMarkRead: (id: number) => void,
  onMarkAllRead: () => void,
): void {
  container.replaceChildren();

  const header = document.createElement('div');
  header.style.cssText = 'display:flex;justify-content:space-between;align-items:center;padding:10px 16px;border-bottom:1px solid var(--border,#262920);';
  const titleSpan = document.createElement('span');
  titleSpan.style.cssText = 'font-size:13px;font-weight:600;color:var(--text-dim,#9b9d8c)';
  titleSpan.textContent = 'Notifications';
  header.appendChild(titleSpan);
  const markAllBtn = document.createElement('button');
  markAllBtn.className = 'btn-ghost';
  markAllBtn.textContent = 'Mark all read';
  markAllBtn.style.cssText = 'font-size:11px;color:var(--accent,#FFF5E6);background:none;border:none;cursor:pointer;';
  markAllBtn.onclick = onMarkAllRead;
  header.appendChild(markAllBtn);
  container.appendChild(header);

  if (notifications.length === 0) {
    const empty = document.createElement('div');
    empty.style.cssText = 'padding:40px 20px;text-align:center;color:var(--text-faint,#62655a);font-size:13px;';
    empty.textContent = 'No notifications';
    container.appendChild(empty);
    return;
  }

  const list = document.createElement('div');
  for (const notif of notifications) {
    const item = document.createElement('div');
    item.style.cssText = `
      padding:9px 16px;border-bottom:1px solid var(--border,#262920);
      ${notif.read ? 'opacity:0.6' : ''}
    `;

    const levelColors: Record<string, string> = {
      info: 'var(--accent,#FFF5E6)',
      warning: '#e6b800',
      error: '#d9534f',
    };

    // Build notification item using DOM API (safe from XSS)
    const content = document.createElement('div');
    content.style.cssText = 'display:flex;justify-content:space-between;align-items:flex-start;';

    const left = document.createElement('div');
    const titleEl = document.createElement('span');
    titleEl.style.cssText = 'font-size:12px;font-weight:600;color:var(--text,#f2f2ea)';
    titleEl.textContent = notif.title;
    left.appendChild(titleEl);

    if (notif.body) {
      const bodyEl = document.createElement('p');
      bodyEl.style.cssText = 'font-size:12px;color:var(--text-dim,#9b9d8c);margin:2px 0 0';
      bodyEl.textContent = notif.body;
      left.appendChild(bodyEl);
    }

    const timeEl = document.createElement('span');
    timeEl.style.cssText = 'font-size:10px;color:var(--text-faint,#62655a);margin-top:4px;display:block';
    timeEl.textContent = notif.createdAt;
    left.appendChild(timeEl);

    content.appendChild(left);

    const right = document.createElement('div');
    right.style.cssText = 'display:flex;flex-direction:column;align-items:flex-end;gap:4px;';

    const dot = document.createElement('span');
    dot.style.cssText = `width:8px;height:8px;border-radius:50%;background:${levelColors[notif.level] || levelColors.info}`;
    right.appendChild(dot);

    if (!notif.read) {
      const markBtn = document.createElement('button');
      markBtn.className = 'btn-ghost';
      markBtn.style.cssText = 'font-size:10px;color:var(--accent,#FFF5E6);background:none;border:none;cursor:pointer;';
      markBtn.textContent = 'Mark read';
      markBtn.addEventListener('click', () => onMarkRead(notif.id));
      right.appendChild(markBtn);
    }

    content.appendChild(right);
    item.appendChild(content);
    list.appendChild(item);
  }

  container.appendChild(list);
}

// ── Error Page ──

export function renderErrorPage(
  container: HTMLElement,
  error: ClassifiedError,
  onReload: () => void,
): void {
  container.replaceChildren();
  container.style.cssText = 'display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;padding:40px;text-align:center;';

  const icon = document.createElement('div');
  icon.style.cssText = 'font-size:44px;margin-bottom:16px;';
  icon.textContent = '⚠️';
  container.appendChild(icon);

  const title = document.createElement('h2');
  title.style.cssText = 'font-size:18px;font-weight:600;color:var(--text,#f2f2ea);margin-bottom:8px;';
  title.textContent = error.userMessage;
  container.appendChild(title);

  if (error.actionHint) {
    const hint = document.createElement('p');
    hint.style.cssText = 'font-size:13px;color:var(--text-dim,#9b9d8c);margin-bottom:24px;';
    hint.textContent = error.actionHint;
    container.appendChild(hint);
  }

  const reloadBtn = document.createElement('button');
  reloadBtn.className = 'btn btn-primary';
  reloadBtn.textContent = 'Reload';
  reloadBtn.style.cssText = 'padding:8px 20px;';
  reloadBtn.onclick = onReload;
  container.appendChild(reloadBtn);
}
