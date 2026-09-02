/**
 * Media Background (supports local images & video with memory safety).
 */

import { escapeHtml } from '../sanitizer.js';

export const mediaBackground = {
  key: 'background/media',
  name: 'Local Media',
  defaultData: { mediaType: 'image', dataUrl: '', loop: true, muted: true },
  render(container, data = {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    let mediaEl = null;

    if (!data.dataUrl) {
      const empty = document.createElement('div');
      empty.className = 'background-not-configured';
      empty.textContent = t('selectMediaHint', '本地媒体：请在设置中选择本地图片或视频');
      container.append(empty);
      return () => {};
    }

    if (data.mediaType === 'video' || /^data:video\//i.test(data.dataUrl)) {
      mediaEl = document.createElement('video');
      mediaEl.className = 'background-media-element';
      mediaEl.src = data.dataUrl;
      mediaEl.autoplay = true;
      mediaEl.loop = data.loop !== false;
      mediaEl.muted = data.muted !== false;
      mediaEl.playsInline = true;
      mediaEl.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;object-fit:cover;pointer-events:none;';
    } else {
      mediaEl = document.createElement('img');
      mediaEl.className = 'background-media-element';
      mediaEl.src = data.dataUrl;
      mediaEl.alt = 'Media Background';
      mediaEl.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;object-fit:cover;pointer-events:none;';
    }

    container.append(mediaEl);

    return () => {
      if (mediaEl && mediaEl.tagName === 'VIDEO') {
        mediaEl.pause();
        mediaEl.removeAttribute('src');
        mediaEl.load();
      }
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('mediaFile', '本地媒体文件')}</span>
        <input type="file" id="m-file" accept="image/*,video/*" />
      </label>
      ${data.dataUrl ? `
        <div class="inspector-actions">
          <small class="muted">${data.mediaType === 'video' ? t('videoLoaded', '已加载视频') : t('imageLoaded', '已加载图片')}</small>
          <button type="button" id="m-clear" class="danger">${t('clear', '清除')}</button>
        </div>
      ` : ''}
    `;

    const fileInput = wrap.querySelector('#m-file');
    fileInput.onchange = () => {
      const file = fileInput.files?.[0];
      if (!file) return;
      if (file.size > 30 * 1024 * 1024) {
        alert(t('mediaTooLarge', '媒体文件不能超过 30MB'));
        return;
      }
      const isVideo = /^video\//i.test(file.type);
      const reader = new FileReader();
      reader.onload = () => {
        onChange({
          ...data,
          mediaType: isVideo ? 'video' : 'image',
          dataUrl: reader.result,
        });
      };
      reader.readAsDataURL(file);
    };

    const clearBtn = wrap.querySelector('#m-clear');
    if (clearBtn) {
      clearBtn.onclick = () => {
        onChange({ ...data, dataUrl: '' });
      };
    }

    container.append(wrap);
  },
};
