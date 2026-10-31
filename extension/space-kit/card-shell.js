/**
 * 卡片外壳：统一的标题、图标、计数、动作与页脚。
 *
 * 只负责结构与无障碍，不读取空间快照、不发写入请求。卡片负责填充 body，
 * 数据持久化仍由 Dashboard 注入的 onDataChange 完成。
 */

import { createIcon } from './card-icons.js';

function textNode(tag, className, text = '') {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

/** 创建一个动作按钮（图标 + 可访问名称）。 */
export function createCardAction({ icon, label, onClick, className = '' }) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = `space-card-action ${className}`.trim();
  button.title = label;
  button.setAttribute('aria-label', label);
  button.append(createIcon(icon, { size: 14 }));
  if (typeof onClick === 'function') button.addEventListener('click', onClick);
  return button;
}

/**
 * @param {object} options
 * @param {string} options.title 卡片标题（纯文本）
 * @param {string} [options.icon] card-icons 里的图标名
 * @param {string|number} [options.count] 头部计数
 * @param {Array<{icon:string,label:string,onClick:Function}>} [options.actions]
 * @param {string} [options.className] 追加到根节点的类名
 */
export function createCardShell({ title, icon = '', count = '', actions = [], className = '' } = {}) {
  const root = document.createElement('section');
  root.className = `space-card ${className}`.trim();
  root.setAttribute('aria-label', title);

  const head = document.createElement('header');
  head.className = 'space-card-head';

  if (icon) {
    const iconHost = document.createElement('span');
    iconHost.className = 'space-card-icon';
    iconHost.append(createIcon(icon, { size: 15 }));
    head.append(iconHost);
  }

  const titleEl = textNode('span', 'space-card-title', title);
  const countEl = textNode('span', 'space-card-count', count === 0 || count === '' ? '' : String(count));
  countEl.hidden = !countEl.textContent;
  const actionsEl = document.createElement('span');
  actionsEl.className = 'space-card-actions';

  head.append(titleEl, countEl, actionsEl);

  const body = document.createElement('div');
  body.className = 'space-card-body';

  const foot = document.createElement('footer');
  foot.className = 'space-card-foot';
  foot.hidden = true;

  root.append(head, body, foot);

  function setTitle(next) {
    titleEl.textContent = next;
    root.setAttribute('aria-label', next);
  }

  function setCount(next) {
    countEl.textContent = next === 0 || next === undefined || next === null ? '' : String(next);
    countEl.hidden = !countEl.textContent;
  }

  function setFooter(content) {
    foot.replaceChildren();
    if (content === undefined || content === null || content === '') {
      foot.hidden = true;
      return;
    }
    foot.hidden = false;
    if (content && typeof content === 'object' && 'nodeType' in content) foot.append(content);
    else foot.textContent = String(content);
  }

  function addAction(action) {
    const button = createCardAction(action);
    actionsEl.append(button);
    return button;
  }

  for (const action of actions) addAction(action);

  return { root, head, body, foot, setTitle, setCount, setFooter, addAction };
}
