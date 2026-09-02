/** Weather widget using the same summary/details/forecast structure as TablissNG. */

import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';

export const weatherWidget = {
  key: 'widget/weather',
  name: 'Weather',
  defaultData: {
    city: 'Beijing',
    lat: 39.9042,
    lon: 116.4074,
    unit: 'celsius',
    showForecast: true,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const city = data.city || 'Beijing';
    const lat = data.lat ?? 39.9042;
    const lon = data.lon ?? 116.4074;
    const isFahrenheit = data.unit === 'fahrenheit';
    const tempUnit = isFahrenheit ? '°F' : '°C';
    let disposed = false;

    container.className = 'Widget Weather';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'weather-content';

    const currentCard = document.createElement('div');
    currentCard.className = 'summary';
    currentCard.innerHTML = `
      <span class="weather-location">${escapeHtml(city)}</span>
      <span class="weather-icon-symbol" aria-hidden="true">☀️</span>
      <span class="temperature">--${tempUnit}</span>
      <span class="weather-condition"> </span>
    `;
    root.append(currentCard);

    const detailsRow = document.createElement('div');
    detailsRow.className = 'details';
    detailsRow.innerHTML = `
      <dl><dt class="val-feels">--${tempUnit}</dt><dd>${t('feelsLike', '体感')}</dd></dl>
      <dl><dt class="val-humidity">--%</dt><dd>${t('humidity', '湿度')}</dd></dl>
    `;
    root.append(detailsRow);

    const forecastRow = document.createElement('div');
    forecastRow.className = 'forecast';
    if (data.showForecast !== false) {
      root.append(forecastRow);
    }

    container.append(root);

    const tempParam = isFahrenheit ? '&temperature_unit=fahrenheit' : '';
    const cacheKey = `weather_${lat.toFixed(2)}_${lon.toFixed(2)}_${data.unit || 'c'}`;

    fetchDedup(
      cacheKey,
      async () => {
        const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto${tempParam}`;
        const res = await fetch(url);
        if (!res.ok) throw new Error('Weather API error');
        return res.json();
      },
      15 * 60 * 1000,
    )
      .then((json) => {
        if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;

        const current = json?.current || {};
        const daily = json?.daily || {};

        const temp = current.temperature_2m ?? '--';
        const humidity = current.relative_humidity_2m ?? '--';
        const feels = current.apparent_temperature ?? temp;
        const code = current.weather_code ?? 0;
        const { icon, label } = interpretWeather(code, t);

        currentCard.querySelector('.weather-icon-symbol').textContent = icon;
        currentCard.querySelector('.temperature').textContent = `${Math.round(temp)}${tempUnit}`;
        currentCard.querySelector('.weather-condition').textContent = label;
        currentCard.title = label;

        detailsRow.querySelector('.val-humidity').textContent = `${humidity}%`;
        detailsRow.querySelector('.val-feels').textContent = `${Math.round(feels)}${tempUnit}`;

        if (data.showForecast !== false && Array.isArray(daily.time)) {
          forecastRow.innerHTML = daily.time.slice(1, 6).map((dayStr, i) => {
            const idx = i + 1;
            const d = new Date(dayStr);
            const weekday = d.toLocaleDateString(undefined, { weekday: 'short' });
            const dayCode = daily.weather_code?.[idx] ?? 0;
            const dayMax = Math.round(daily.temperature_2m_max?.[idx] ?? 0);
            const dayMin = Math.round(daily.temperature_2m_min?.[idx] ?? 0);
            const dayWeather = interpretWeather(dayCode, t);

            return `
              <dl class="day">
                <dt>${escapeHtml(weekday)}</dt>
                <dd class="condition" title="${escapeHtml(dayWeather.label)}">${dayWeather.icon}</dd>
                <dd class="temperatures"><span>${dayMax}°</span><span class="low">${dayMin}°</span></dd>
              </dl>
            `;
          }).join('');
        }
      })
      .catch(() => {
        if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;
        currentCard.querySelector('.temperature').textContent = '-';
        currentCard.title = t('failed', '获取失败');
      });

    return () => {
      disposed = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('cityName', '城市名称')}</span>
        <input type="text" id="w-city" value="${escapeHtml(data.city || 'Beijing')}" placeholder="Beijing / London / Tokyo" />
      </label>
      <label class="inspector-field">
        <span>${t('temperatureUnit', '温度单位')}</span>
        <select id="w-unit">
          <option value="celsius" ${data.unit !== 'fahrenheit' ? 'selected' : ''}>摄氏度 (°C)</option>
          <option value="fahrenheit" ${data.unit === 'fahrenheit' ? 'selected' : ''}>华氏度 (°F)</option>
        </select>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="w-forecast" ${data.showForecast !== false ? 'checked' : ''} />
        <span>${t('showFiveDayForecast', '显示 5 天天气预报')}</span>
      </label>
      <div class="inspector-fields-row">
        <label class="inspector-field">
          <span>${t('latitude', '纬度')}</span>
          <input type="number" step="0.0001" id="w-lat" value="${data.lat ?? 39.9042}" />
        </label>
        <label class="inspector-field">
          <span>${t('longitude', '经度')}</span>
          <input type="number" step="0.0001" id="w-lon" value="${data.lon ?? 116.4074}" />
        </label>
      </div>
    `;

    const cityInput = wrap.querySelector('#w-city');
    cityInput.onchange = async () => {
      const newCity = cityInput.value.trim();
      if (!newCity) return;
      try {
        const geoRes = await fetch(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(newCity)}&count=1`);
        const geoJson = await geoRes.json();
        if (geoJson.results && geoJson.results[0]) {
          const res = geoJson.results[0];
          onChange({
            ...data,
            city: res.name || newCity,
            lat: res.latitude,
            lon: res.longitude,
          });
          return;
        }
      } catch {}
      onChange({ ...data, city: newCity });
    };

    wrap.querySelector('#w-unit').onchange = (e) => onChange({ ...data, unit: e.target.value });
    wrap.querySelector('#w-forecast').onchange = (e) => onChange({ ...data, showForecast: e.target.checked });
    wrap.querySelector('#w-lat').onchange = (e) => onChange({ ...data, lat: Number(e.target.value) });
    wrap.querySelector('#w-lon').onchange = (e) => onChange({ ...data, lon: Number(e.target.value) });

    container.append(wrap);
  },
  styles: `
    .Weather .summary { cursor:pointer; display:inline-flex; align-items:center; }
    .Weather .summary .weather-icon-symbol { margin:0 .5em; }
    .Weather .details { font-size:1rem; line-height:1.5; }
    .Weather .details dt { font-weight:700; }
    .Weather .details dd { margin:0; }
    .Weather .forecast { display:inline-flex; gap:3rem; align-items:center; margin-top:.5rem; }
    .Weather .forecast .day { margin:0; display:inline-flex; flex-direction:column; align-items:center; }
    .Weather .forecast .condition, .Weather .forecast .temperatures { margin:0; }
    .Weather .forecast .temperatures { display:flex; gap:.4rem; }
    .Weather .low { opacity:.7; }
  `,
};

function interpretWeather(code, t = (k, f) => f || k) {
  if (code === 0) return { icon: '☀️', label: t('weatherClear', '晴') };
  if ([1, 2].includes(code)) return { icon: '🌤️', label: t('weatherPartlyCloudy', '少云') };
  if (code === 3) return { icon: '☁️', label: t('weatherOvercast', '阴') };
  if ([45, 48].includes(code)) return { icon: '🌫️', label: t('weatherFoggy', '雾') };
  if ([51, 53, 55].includes(code)) return { icon: '🌦️', label: t('weatherDrizzle', '毛毛雨') };
  if ([61, 63, 65, 80, 81, 82].includes(code)) return { icon: '🌧️', label: t('weatherRainy', '雨') };
  if ([71, 73, 75, 77, 85, 86].includes(code)) return { icon: '🌨️', label: t('weatherSnowy', '雪') };
  if ([95, 96, 99].includes(code)) return { icon: '⛈️', label: t('weatherThunder', '雷暴') };
  return { icon: '🌤️', label: t('weatherCloudy', '多云') };
}
