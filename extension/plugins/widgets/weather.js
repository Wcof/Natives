/** Weather widget using the same summary/details/forecast structure as TablissNG. */

import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';
import { CHINA_REGIONS, OVERSEAS_REGIONS } from './weather-cities.js';

export const weatherWidget = {
  key: 'widget/weather',
  name: 'Weather',
  defaultData: {
    regionType: 'domestic', // 'domestic' | 'overseas'
    province: '北京市',
    city: '北京市',
    district: '东城区',
    continent: '亚洲 (Asia)',
    lat: 39.9284,
    lon: 116.4163,
    unit: 'celsius',
    showForecast: true,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    let currentData = { ...data };
    let city = currentData.city || 'Beijing';
    let lat = currentData.lat ?? 39.9042;
    let lon = currentData.lon ?? 116.4074;
    const isFahrenheit = currentData.unit === 'fahrenheit';
    const tempUnit = isFahrenheit ? '°F' : '°C';
    let disposed = false;

    container.className = 'Widget Weather';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'weather-content';

    // 1. 卡片天气摘要与温度
    const currentCard = document.createElement('div');
    currentCard.className = 'summary';
    currentCard.innerHTML = `
      <span class="weather-location">${escapeHtml(city)}</span>
      <span class="weather-icon-symbol" aria-hidden="true">☀️</span>
      <span class="temperature">--${tempUnit}</span>
      <span class="weather-condition"> </span>
    `;
    root.append(currentCard);

    // 2. 卡片体感与湿度
    const detailsRow = document.createElement('div');
    detailsRow.className = 'details';
    detailsRow.innerHTML = `
      <dl><dt class="val-feels">--${tempUnit}</dt><dd>${t('feelsLike', '体感')}</dd></dl>
      <dl><dt class="val-humidity">--%</dt><dd>${t('humidity', '湿度')}</dd></dl>
    `;
    root.append(detailsRow);

    // 3. 未来预报
    const forecastRow = document.createElement('div');
    forecastRow.className = 'forecast';
    if (currentData.showForecast !== false) {
      root.append(forecastRow);
    }

    container.append(root);

    function fetchAndRenderWeather(targetLat, targetLon) {
      const tempParam = isFahrenheit ? '&temperature_unit=fahrenheit' : '';
      const cacheKey = `weather_${targetLat.toFixed(2)}_${targetLon.toFixed(2)}_${currentData.unit || 'c'}`;

      fetchDedup(
        cacheKey,
        async () => {
          const url = `https://api.open-meteo.com/v1/forecast?latitude=${targetLat}&longitude=${targetLon}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto${tempParam}`;
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

          if (currentData.showForecast !== false && Array.isArray(daily.time)) {
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
    }

    // 初始加载天气
    fetchAndRenderWeather(lat, lon);

    return () => {
      disposed = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';

    const regionType = data.regionType || (OVERSEAS_REGIONS[data.continent] ? 'overseas' : 'domestic');
    let province = data.province || Object.keys(CHINA_REGIONS)[0];
    if (!CHINA_REGIONS[province]) province = Object.keys(CHINA_REGIONS)[0];

    const provinceData = CHINA_REGIONS[province] || {};
    const cityList = Object.keys(provinceData.cities || {});
    let city = data.city && cityList.includes(data.city) ? data.city : (cityList[0] || province);

    const cityData = provinceData.cities?.[city] || {};
    const districtList = Object.keys(cityData.districts || {});
    let district = data.district && districtList.includes(data.district) ? data.district : (districtList[0] || '');

    let continent = data.continent || Object.keys(OVERSEAS_REGIONS)[0];
    if (!OVERSEAS_REGIONS[continent]) continent = Object.keys(OVERSEAS_REGIONS)[0];
    const overseasCities = Object.keys(OVERSEAS_REGIONS[continent] || {});
    let overseasCity = data.overseasCity && overseasCities.includes(data.overseasCity) ? data.overseasCity : (overseasCities[0] || '');

    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('weatherRegionType', '地区类型')}</span>
        <select id="w-region-type">
          <option value="domestic" ${regionType === 'domestic' ? 'selected' : ''}>${t('weatherRegionDomestic', '国内 (中国大陆)')}</option>
          <option value="overseas" ${regionType === 'overseas' ? 'selected' : ''}>${t('weatherRegionOverseas', '国际 / 海外主要城市')}</option>
        </select>
      </label>

      <!-- 国内级联：省 -> 市 -> 区 -->
      <div id="w-domestic-group" style="${regionType === 'domestic' ? '' : 'display:none;'}">
        <label class="inspector-field">
          <span>${t('weatherProvince', '省份 / 直辖市')}</span>
          <select id="w-province">
            ${Object.keys(CHINA_REGIONS).map((p) => `<option value="${escapeHtml(p)}" ${p === province ? 'selected' : ''}>${escapeHtml(p)}</option>`).join('')}
          </select>
        </label>
        <label class="inspector-field">
          <span>${t('weatherCity', '城市 / 地区')}</span>
          <select id="w-city-select">
            ${cityList.map((c) => `<option value="${escapeHtml(c)}" ${c === city ? 'selected' : ''}>${escapeHtml(c)}</option>`).join('')}
          </select>
        </label>
        <label class="inspector-field">
          <span>${t('weatherDistrict', '区县')}</span>
          <select id="w-district-select">
            ${districtList.map((d) => `<option value="${escapeHtml(d)}" ${d === district ? 'selected' : ''}>${escapeHtml(d)}</option>`).join('')}
          </select>
        </label>
      </div>

      <!-- 国外级联：大洲 -> 国际城市 -->
      <div id="w-overseas-group" style="${regionType === 'overseas' ? '' : 'display:none;'}">
        <label class="inspector-field">
          <span>${t('weatherContinent', '大洲')}</span>
          <select id="w-continent">
            ${Object.keys(OVERSEAS_REGIONS).map((c) => `<option value="${escapeHtml(c)}" ${c === continent ? 'selected' : ''}>${escapeHtml(c)}</option>`).join('')}
          </select>
        </label>
        <label class="inspector-field">
          <span>${t('weatherOverseasCity', '城市')}</span>
          <select id="w-overseas-city">
            ${overseasCities.map((oc) => `<option value="${escapeHtml(oc)}" ${oc === overseasCity ? 'selected' : ''}>${escapeHtml(oc)}</option>`).join('')}
          </select>
        </label>
      </div>

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
          <input type="number" step="0.0001" id="w-lat" readonly value="${data.lat ?? 39.9284}" />
        </label>
        <label class="inspector-field">
          <span>${t('longitude', '经度')}</span>
          <input type="number" step="0.0001" id="w-lon" readonly value="${data.lon ?? 116.4163}" />
        </label>
      </div>
    `;

    const regionSelect = wrap.querySelector('#w-region-type');
    const domesticGroup = wrap.querySelector('#w-domestic-group');
    const overseasGroup = wrap.querySelector('#w-overseas-group');
    const provinceSelect = wrap.querySelector('#w-province');
    const citySelect = wrap.querySelector('#w-city-select');
    const districtSelect = wrap.querySelector('#w-district-select');
    const continentSelect = wrap.querySelector('#w-continent');
    const overseasCitySelect = wrap.querySelector('#w-overseas-city');
    const latInput = wrap.querySelector('#w-lat');
    const lonInput = wrap.querySelector('#w-lon');

    function resolveDomesticGeo(p, c, d) {
      const pData = CHINA_REGIONS[p] || {};
      const cData = pData.cities?.[c] || {};
      const dData = cData.districts?.[d];
      const lat = dData?.lat ?? cData.lat ?? pData.lat ?? 39.9042;
      const lon = dData?.lon ?? cData.lon ?? pData.lon ?? 116.4074;
      const displayName = d || c || p;
      return { lat, lon, displayName };
    }

    function resolveOverseasGeo(cont, oc) {
      const cityData = OVERSEAS_REGIONS[cont]?.[oc] || {};
      const lat = cityData.lat ?? 51.5074;
      const lon = cityData.lon ?? -0.1278;
      const displayName = oc.split(' ')[0] || oc;
      return { lat, lon, displayName };
    }

    regionSelect.onchange = () => {
      const isDomestic = regionSelect.value === 'domestic';
      domesticGroup.style.display = isDomestic ? '' : 'none';
      overseasGroup.style.display = isDomestic ? 'none' : '';
      if (isDomestic) {
        const curP = provinceSelect.value;
        const curC = citySelect.value;
        const curD = districtSelect.value;
        const { lat, lon, displayName } = resolveDomesticGeo(curP, curC, curD);
        latInput.value = lat;
        lonInput.value = lon;
        onChange({
          ...data,
          regionType: 'domestic',
          province: curP,
          city: displayName,
          district: curD,
          lat,
          lon,
        });
      } else {
        const curCont = continentSelect.value;
        const curOC = overseasCitySelect.value;
        const { lat, lon, displayName } = resolveOverseasGeo(curCont, curOC);
        latInput.value = lat;
        lonInput.value = lon;
        onChange({
          ...data,
          regionType: 'overseas',
          continent: curCont,
          overseasCity: curOC,
          city: displayName,
          lat,
          lon,
        });
      }
    };

    provinceSelect.onchange = () => {
      const nextP = provinceSelect.value;
      const pData = CHINA_REGIONS[nextP] || {};
      const nextCities = Object.keys(pData.cities || {});
      const nextC = nextCities[0] || nextP;

      citySelect.innerHTML = nextCities.map((c) => `<option value="${escapeHtml(c)}">${escapeHtml(c)}</option>`).join('');
      citySelect.value = nextC;

      const cData = pData.cities?.[nextC] || {};
      const nextDistricts = Object.keys(cData.districts || {});
      const nextD = nextDistricts[0] || '';
      districtSelect.innerHTML = nextDistricts.map((d) => `<option value="${escapeHtml(d)}">${escapeHtml(d)}</option>`).join('');
      districtSelect.value = nextD;

      const { lat, lon, displayName } = resolveDomesticGeo(nextP, nextC, nextD);
      latInput.value = lat;
      lonInput.value = lon;
      onChange({
        ...data,
        regionType: 'domestic',
        province: nextP,
        city: displayName,
        district: nextD,
        lat,
        lon,
      });
    };

    citySelect.onchange = () => {
      const curP = provinceSelect.value;
      const nextC = citySelect.value;
      const cData = CHINA_REGIONS[curP]?.cities?.[nextC] || {};
      const nextDistricts = Object.keys(cData.districts || {});
      const nextD = nextDistricts[0] || '';

      districtSelect.innerHTML = nextDistricts.map((d) => `<option value="${escapeHtml(d)}">${escapeHtml(d)}</option>`).join('');
      districtSelect.value = nextD;

      const { lat, lon, displayName } = resolveDomesticGeo(curP, nextC, nextD);
      latInput.value = lat;
      lonInput.value = lon;
      onChange({
        ...data,
        regionType: 'domestic',
        province: curP,
        city: displayName,
        district: nextD,
        lat,
        lon,
      });
    };

    districtSelect.onchange = () => {
      const curP = provinceSelect.value;
      const curC = citySelect.value;
      const nextD = districtSelect.value;
      const { lat, lon, displayName } = resolveDomesticGeo(curP, curC, nextD);
      latInput.value = lat;
      lonInput.value = lon;
      onChange({
        ...data,
        regionType: 'domestic',
        province: curP,
        city: displayName,
        district: nextD,
        lat,
        lon,
      });
    };

    continentSelect.onchange = () => {
      const nextCont = continentSelect.value;
      const nextCities = Object.keys(OVERSEAS_REGIONS[nextCont] || {});
      const nextOC = nextCities[0] || '';
      overseasCitySelect.innerHTML = nextCities.map((c) => `<option value="${escapeHtml(c)}">${escapeHtml(c)}</option>`).join('');
      overseasCitySelect.value = nextOC;

      const { lat, lon, displayName } = resolveOverseasGeo(nextCont, nextOC);
      latInput.value = lat;
      lonInput.value = lon;
      onChange({
        ...data,
        regionType: 'overseas',
        continent: nextCont,
        overseasCity: nextOC,
        city: displayName,
        lat,
        lon,
      });
    };

    overseasCitySelect.onchange = () => {
      const curCont = continentSelect.value;
      const nextOC = overseasCitySelect.value;
      const { lat, lon, displayName } = resolveOverseasGeo(curCont, nextOC);
      latInput.value = lat;
      lonInput.value = lon;
      onChange({
        ...data,
        regionType: 'overseas',
        continent: curCont,
        overseasCity: nextOC,
        city: displayName,
        lat,
        lon,
      });
    };

    wrap.querySelector('#w-unit').onchange = (e) => onChange({ ...data, unit: e.target.value });
    wrap.querySelector('#w-forecast').onchange = (e) => onChange({ ...data, showForecast: e.target.checked });

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

