

export const colourBackground = {
  key: 'background/colour',
  name: 'Colour',
  defaultData: { colour: '#101010' },
  render(container, data) {
    container.replaceChildren();
    container.style.backgroundColor = data.colour || '#101010';
    container.style.backgroundImage = 'none';
  },
  renderSettings(container, data, onChange) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>颜色</span><input type="color" value="${data.colour || '#101010'}" /></label>
      </div>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, colour: e.target.value });
  },
};
