import { escapeHtml } from '../sanitizer.js';
export const trelloWidget = {
  key: 'widget/trello',
  name: 'Trello',
  defaultData: {
    boardId: '',
    listId: '',
    boardName: '',
    listName: '',
    cards: [],
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k, onDataChange } = {}) {
    container.className = 'Widget Trello';
    container.replaceChildren();
    const root = document.createElement('div');
    root.className = 'widget-container';
    const header = document.createElement('div');
    header.className = 'trello-header';
    header.innerHTML = `
      <div class="trello-brand">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor"><rect x="3" y="3" width="18" height="18" rx="2"></rect><rect x="6" y="6" width="4" height="10" rx="1" fill="rgba(0,0,0,0.6)"></rect><rect x="14" y="6" width="4" height="6" rx="1" fill="rgba(0,0,0,0.6)"></rect></svg>
        <span class="trello-title">${escapeHtml(data.listName || data.boardName || 'Trello')}</span>
      </div>
    `;
    root.append(header);
    const cards = Array.isArray(data.cards) ? data.cards : [];
    const listEl = document.createElement('div');
    listEl.className = 'trello-cards-list';
    if (cards.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'trello-empty';
      empty.textContent = t('noTrelloCards', '暂无任务卡片');
      listEl.append(empty);
    } else {
      cards.forEach((card, idx) => {
        const cardEl = document.createElement('div');
        cardEl.className = `trello-card-item ${card.closed ? 'is-done' : ''}`;
        cardEl.innerHTML = `
          <input type="checkbox" class="trello-card-check" ${card.closed ? 'checked' : ''} />
          <div class="trello-card-body">
            <span class="trello-card-name">${escapeHtml(card.name)}</span>
            ${card.due ? `<small class="trello-card-due">📅 ${escapeHtml(card.due)}</small>` : ''}
          </div>
        `;
        const chk = cardEl.querySelector('.trello-card-check');
        chk.onchange = () => {
          const nextCards = [...cards];
          nextCards[idx] = { ...card, closed: chk.checked };
          onDataChange?.({ ...data, cards: nextCards });
        };
        listEl.append(cardEl);
      });
    }
    root.append(listEl);
    const addForm = document.createElement('form');
    addForm.className = 'trello-add-form';
    addForm.innerHTML = `
      <input type="text" placeholder="+ ${t('addCard', '添加卡片...')}" />
    `;
    addForm.onsubmit = (e) => {
      e.preventDefault();
      const input = addForm.querySelector('input');
      const val = input.value.trim();
      if (!val) return;
      const nextCards = [...cards, { id: crypto.randomUUID(), name: val, closed: false }];
      input.value = '';
      onDataChange?.({ ...data, cards: nextCards });
    };
    root.append(addForm);
    container.append(root);
    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('boardName', '看板名称')}</span>
        <input type="text" id="t-board" value="${escapeHtml(data.boardName || '')}" placeholder="My Board" />
      </label>
      <label class="inspector-field">
        <span>${t('listName', '列表名称')}</span>
        <input type="text" id="t-list" value="${escapeHtml(data.listName || '')}" placeholder="To Do" />
      </label>
    `;
    wrap.querySelector('#t-board').onchange = (e) => onChange({ ...data, boardName: e.target.value.trim() });
    wrap.querySelector('#t-list').onchange = (e) => onChange({ ...data, listName: e.target.value.trim() });
    container.append(wrap);
  },
  styles: `
    .Trello .widget-container {
      height:30vh;
      display:flex;
      flex-direction: column;
      justify-content:center;
      white-space:normal;
      gap: 8px;
    }
    .Trello .trello-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding-bottom: 6px;
    }
    .Trello .trello-brand {
      display: flex;
      align-items: center;
      gap: 6px;
      font-size: 13px;
      font-weight: 600;
    }
    .Trello .trello-cards-list {
      display: flex;
      flex-direction: column;
      gap: 4px;
      max-height: 240px;
      overflow-y: auto;
    }
    .Trello .trello-card-item {
      display: flex;
      align-items: flex-start;
      gap: 8px;
      padding: 6px 8px;
      font-size: 12.5px;
      line-height: 1.35;
    }
    .Trello .trello-card-check {
      margin-top: 2px;
      cursor: pointer;
      accent-color: var(--accent, #cdf24b);
      flex: 0 0 auto;
    }
    .Trello .trello-card-body {
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }
    .Trello .trello-card-item.is-done .trello-card-name {
      text-decoration: line-through;
      opacity: 0.55;
    }
    .Trello .trello-card-due {
      font-size: 10.5px;
      opacity: 0.65;
    }
    .Trello .trello-add-form input {
      width: 100%;
      height: 28px;
      padding: 0 8px;
      border: 1px solid currentColor;
      background: transparent;
      color: inherit;
      font-size: 12px;
      outline: none;
      box-sizing: border-box;
    }
    .Trello .trello-add-form input:focus {
      border-color: var(--accent, #cdf24b);
    }
    .Trello .trello-empty {
      padding: 12px 0;
      text-align: center;
      opacity: 0.6;
      font-size: 12px;
    }
  `,
};
