const THEME_PALETTES = {
  green: ['#161b22', '#0e4429', '#006d32', '#26a641', '#39d353'],
  orange: ['#161b22', '#451e11', '#7c2d12', '#ea580c', '#fb923c'],
  blue: ['#161b22', '#0c2d48', '#145da0', '#2e8bc0', '#b1d4e0'],
  purple: ['#161b22', '#2e1065', '#581c87', '#9333ea', '#c084fc'],
  gold: ['#161b22', '#3f2e04', '#785b09', '#d97706', '#fbbf24'],
};

export function renderActivityCalendar({
  container,
  data = [],
  totalCount = 0,
  totalLabel = 'contributions in the last year',
  theme = 'green',
  profileUrl = '',
  showLegend = true,
  t,
}) {
  container.replaceChildren();

  const colors = THEME_PALETTES[theme] || THEME_PALETTES.green;
  const wrapper = document.createElement(profileUrl ? 'a' : 'div');
  wrapper.className = 'activity-calendar-root';
  if (profileUrl) {
    wrapper.href = profileUrl;
    wrapper.target = '_blank';
    wrapper.rel = 'noopener noreferrer';
  }

  const svgWidth = 700;
  const svgHeight = 104;
  const cellSize = 10;
  const cellGap = 2.5;
  const colStep = cellSize + cellGap;

  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'activity-calendar-svg');
  svg.setAttribute('viewBox', `0 0 ${svgWidth} ${svgHeight}`);
  svg.setAttribute('width', '100%');

  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const monthG = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  monthG.setAttribute('class', 'activity-calendar-months');

  const now = new Date();
  for (let m = 0; m < 12; m++) {
    const mDate = new Date(now.getFullYear(), now.getMonth() - 11 + m, 1);
    const mName = months[mDate.getMonth()];
    const mText = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    const xPos = 24 + m * 56;
    mText.setAttribute('x', String(xPos));
    mText.setAttribute('y', '10');
    mText.setAttribute('font-size', '9');
    mText.setAttribute('fill', 'rgba(255,255,255,0.6)');
    mText.textContent = mName;
    monthG.append(mText);
  }
  svg.append(monthG);

  const daysG = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  daysG.setAttribute('class', 'activity-calendar-weekdays');
  const dayLabels = [{ name: 'Mon', y: 32 }, { name: 'Wed', y: 57 }, { name: 'Fri', y: 82 }];
  for (const d of dayLabels) {
    const dText = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    dText.setAttribute('x', '0');
    dText.setAttribute('y', String(d.y));
    dText.setAttribute('font-size', '8');
    dText.setAttribute('fill', 'rgba(255,255,255,0.5)');
    dText.textContent = d.name;
    daysG.append(dText);
  }
  svg.append(daysG);

  const cellsG = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  cellsG.setAttribute('transform', 'translate(22, 16)');

  const dataMap = new Map();
  for (const item of data) {
    dataMap.set(item.date, item);
  }

  const daysCount = 53 * 7;
  const startDate = new Date();
  startDate.setDate(startDate.getDate() - daysCount + (7 - startDate.getDay()));

  for (let col = 0; col < 53; col++) {
    for (let row = 0; row < 7; row++) {
      const cur = new Date(startDate);
      cur.setDate(cur.getDate() + col * 7 + row);
      if (cur > now) continue;

      const dateStr = cur.toISOString().slice(0, 10);
      const entry = dataMap.get(dateStr) || { count: 0, level: 0 };
      const level = Math.max(0, Math.min(4, entry.level || (entry.count > 0 ? (entry.count > 8 ? 4 : entry.count > 4 ? 3 : entry.count > 2 ? 2 : 1) : 0)));
      const color = colors[level];

      const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
      rect.setAttribute('x', String(col * colStep));
      rect.setAttribute('y', String(row * colStep));
      rect.setAttribute('width', String(cellSize));
      rect.setAttribute('height', String(cellSize));
      rect.setAttribute('rx', '2');
      rect.setAttribute('fill', color);
      rect.setAttribute('data-date', dateStr);
      rect.setAttribute('data-count', String(entry.count));

      const title = document.createElementNS('http://www.w3.org/2000/svg', 'title');
      title.textContent = `${entry.count} on ${dateStr}`;
      rect.append(title);

      cellsG.append(rect);
    }
  }
  svg.append(cellsG);
  wrapper.append(svg);

  const footer = document.createElement('div');
  footer.className = 'activity-calendar-footer';
  const total = document.createElement('span');
  total.className = 'activity-calendar-total';
  total.textContent = `${totalCount.toLocaleString()} ${totalLabel}`;
  footer.append(total);
  if (showLegend) {
    const legend = document.createElement('span');
    legend.className = 'activity-calendar-legend';
    legend.innerHTML = `<span>${t?.('less', 'Less') || 'Less'}</span>${colors.map(c => `<span class="legend-box" style="background-color:${c}"></span>`).join('')}<span>${t?.('more', 'More') || 'More'}</span>`;
    footer.append(legend);
  }
  wrapper.append(footer);

  container.append(wrapper);
}

export const activityCalendarStyles = `
  .activity-calendar-root {
    display: inline-flex;
    flex-direction: column;
    gap: 0.35em;
    max-width: 720px;
    text-align: left;
    color: inherit;
    text-decoration: none;
  }
  .activity-calendar-svg {
    display: block;
    overflow: visible;
  }
  .activity-calendar-svg rect {
    transition: transform 0.1s ease, stroke 0.1s ease;
    cursor: pointer;
  }
  .activity-calendar-svg rect:hover {
    stroke: rgba(255, 255, 255, 0.8);
    stroke-width: 1px;
  }
  .activity-calendar-footer {
    display: flex;
    justify-content: space-between;
    gap: 1em;
    font-size: 11px;
    opacity: 0.8;
  }
  .activity-calendar-legend {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .legend-box {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
  }
`;
