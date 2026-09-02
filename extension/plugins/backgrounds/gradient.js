/**
 * Gradient Background.
 */

export const gradientBackground = {
  key: 'background/gradient',
  name: 'Gradient',
  defaultData: { from: '#1a1a2e', to: '#16213e', angle: 135 },
  render(container, data) {
    container.replaceChildren();
    container.style.backgroundColor = 'transparent';
    container.style.backgroundImage = `linear-gradient(${data.angle || 135}deg, ${data.from || '#1a1a2e'}, ${data.to || '#16213e'})`;
    container.style.backgroundSize = 'cover';
  },
  renderSettings(container, data, onChange) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>起始色</span><input type="color" id="g-from" value="${data.from || '#1a1a2e'}" /></label>
        <label class="inspector-field"><span>终止色</span><input type="color" id="g-to" value="${data.to || '#16213e'}" /></label>
      </div>
    `;
    const update = () => onChange({
      ...data,
      from: container.querySelector('#g-from').value,
      to: container.querySelector('#g-to').value,
    });
    container.querySelector('#g-from').onchange = update;
    container.querySelector('#g-to').onchange = update;
  },
};
