export const workHoursWidget = {
  key: 'widget/workHours',
  name: 'Work Hours',
  defaultData: {
    startTime: '09:00',
    endTime: '17:00',
    days: [1, 2, 3, 4, 5],
    flipPercentage: false,
  },
  render(container, data = {}) {
    container.className = 'Widget WorkHours';
    const update = () => {
      container.replaceChildren();
      const now = new Date();
      const days = Array.isArray(data.days) ? data.days : [1, 2, 3, 4, 5];
      if (!days.includes(now.getDay())) return;
      const start = timeToday(data.startTime || data.start || '09:00', now);
      const end = timeToday(data.endTime || data.end || '17:00', now);
      if (start > end) start.setDate(start.getDate() - 1);
      const total = end.getTime() - start.getTime();
      const elapsed = Math.min(total, Math.max(0, now.getTime() - start.getTime()));
      let percentage = total > 0 ? Math.floor((elapsed / total) * 100) : 100;
      if (data.flipPercentage) percentage = 100 - percentage;
      const heading = document.createElement('h2');
      heading.textContent = `${percentage}%`;
      container.append(heading);
    };
    update();
    const intervalId = setInterval(update, 10_000);
    return () => clearInterval(intervalId);
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('startTime', '开始时间')}</span><input id="wh-start" type="time" value="${data.startTime || data.start || '09:00'}" /></label>
        <label class="inspector-field"><span>${t('endTime', '结束时间')}</span><input id="wh-end" type="time" value="${data.endTime || data.end || '17:00'}" /></label>
        <label class="inspector-checkbox"><input id="wh-flip" type="checkbox" ${data.flipPercentage ? 'checked' : ''} /><span>${t('flipPercentage', '显示剩余百分比')}</span></label>
      </div>`;
    const update = () => onChange({
      ...data,
      startTime: container.querySelector('#wh-start').value,
      endTime: container.querySelector('#wh-end').value,
      flipPercentage: container.querySelector('#wh-flip').checked,
      days: Array.isArray(data.days) ? data.days : [1, 2, 3, 4, 5],
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: '',
};

function timeToday(value, now) {
  const [hours, minutes] = String(value).split(':').map(Number);
  const result = new Date(now);
  result.setHours(hours || 0, minutes || 0, 0, 0);
  return result;
}
