/**
 * space-kit 基础样式：卡片外壳与通用原语。
 *
 * 只注入一次（buildDashboardStyles 的 cardStyles），卡片插件不再重复声明。
 * 表面/边界/填充消费 Dashboard 解析出的 --space-widget-* 局部角色，
 * 因此能同时适配深色壁纸与浅色壁纸，也不在组件层现算层级。
 */
export const SPACE_CARD_STYLES = `
  .space-card {
    --space-card-line: var(--space-widget-line, rgba(255, 255, 255, 0.18));
    --space-card-surface: var(--space-widget-surface, rgba(255, 255, 255, 0.07));
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 280px;
    max-width: min(320px, 78vw);
    padding: 12px 14px 11px;
    border: 1px solid var(--space-card-line);
    border-radius: 16px;
    background: var(--space-card-surface);
    -webkit-backdrop-filter: blur(14px) saturate(1.08);
    backdrop-filter: blur(14px) saturate(1.08);
    box-shadow: var(--natives-elev-2, 0 2px 4px rgba(0, 0, 0, 0.04), 0 6px 16px rgba(0, 0, 0, 0.06));
    color: inherit;
    text-align: left;
    line-height: 1.45;
    user-select: text;
  }
  .space-card-head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 18px;
    font-size: 10.5px;
    letter-spacing: 0.06em;
    opacity: 0.78;
  }
  .space-card-icon { display: inline-flex; flex: none; }
  .space-card-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 650;
  }
  .space-card-count {
    flex: none;
    font-variant-numeric: tabular-nums;
    opacity: 0.85;
  }
  .space-card-actions { display: inline-flex; flex: none; gap: 2px; }
  .space-card-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.6;
    transition: opacity 0.15s ease, background 0.15s ease;
  }
  .space-card-action:hover { opacity: 1; background: var(--space-widget-surface-strong); }
  .space-card-action:focus-visible,
  .space-inline-submit:focus-visible,
  .space-row-check:focus-visible,
  .space-row-remove:focus-visible,
  .space-chip:focus-visible,
  .space-habit-check:focus-visible {
    outline: 2px solid currentColor;
    outline-offset: 1px;
  }
  .space-card-body {
    display: flex;
    flex-direction: column;
    gap: 7px;
    font-size: 12.5px;
  }
  .space-card-foot {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    opacity: 0.62;
  }
  .space-card-foot[hidden] { display: none; }
  .space-card button { font: inherit; color: inherit; }
  .space-empty { padding: 3px 0; font-size: 11.5px; opacity: 0.55; }

  .space-inline-form {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-bottom: 3px;
    border-bottom: 1px solid var(--space-card-line);
  }
  .space-card .space-inline-input {
    flex: 1;
    min-width: 0;
    padding: 2px 0;
    border: 0;
    outline: none;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 12.5px;
  }
  .space-card .space-inline-input::placeholder { color: inherit; opacity: 0.42; }
  .space-inline-submit {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.62;
  }
  .space-inline-submit:hover { opacity: 1; background: var(--space-widget-surface-strong); }

  .space-row { display: flex; align-items: center; gap: 7px; min-height: 24px; }
  .space-row-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid var(--space-card-line);
    border-radius: 5px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.45;
  }
  .space-row-check svg { opacity: 0; }
  .space-row.is-done .space-row-check {
    opacity: 1;
    background: var(--space-widget-surface-strong);
  }
  .space-row.is-done .space-row-check svg { opacity: 1; }
  .space-row-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .space-row.is-done .space-row-label { text-decoration: line-through; opacity: 0.55; }
  .space-row.is-wrap { align-items: flex-start; }
  .space-row.is-wrap .space-row-check { margin-top: 2px; }
  .space-row.is-wrap .space-row-label {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .space-row.is-wrap .space-row-remove { margin-top: 1px; }
  .space-row-meta {
    flex: none;
    font-size: 10px;
    opacity: 0.5;
    font-variant-numeric: tabular-nums;
  }
  .space-row-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s ease;
  }
  .space-row:hover .space-row-remove,
  .space-row:focus-within .space-row-remove { opacity: 0.55; }
  .space-row-remove:hover { opacity: 1 !important; }

  .space-progress {
    display: grid;
    grid-template-columns: 54px minmax(0, 1fr) 38px;
    align-items: center;
    gap: 8px;
    font-size: 11px;
  }
  .space-progress-label { opacity: 0.75; }
  .space-progress-track {
    display: block;
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--space-widget-line);
  }
  .space-progress-fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--space-widget-fill, currentColor);
  }
  .space-progress-value {
    text-align: right;
    font-variant-numeric: tabular-nums;
    opacity: 0.72;
  }

  .space-card .space-textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: 72px;
    padding: 7px 8px;
    border: 1px solid var(--space-card-line);
    border-radius: 10px;
    outline: none;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 12px;
    line-height: 1.5;
    resize: vertical;
  }
  .space-card .space-textarea::placeholder { color: inherit; opacity: 0.42; }
  .space-card .space-textarea:focus {
    border-color: var(--space-widget-line-strong);
  }
  .space-status { min-height: 12px; font-size: 10px; opacity: 0.55; }
  .space-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border: 1px solid var(--space-card-line);
    border-radius: 999px;
    background: transparent;
    color: inherit;
    font-size: 10.5px;
    cursor: pointer;
    opacity: 0.75;
  }
  .space-chip:hover { opacity: 1; background: var(--space-widget-surface-strong); }

  .space-habit-head,
  .space-habit-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) repeat(7, 12px) 38px;
    align-items: center;
    gap: 4px;
  }
  .space-habit-head { font-size: 9px; opacity: 0.5; }
  .space-habit-head span { text-align: center; }
  .space-habit-head span:first-child { text-align: left; }
  .space-habit-row { font-size: 11.5px; }
  .space-habit-actions { display: flex; align-items: center; justify-content: flex-end; gap: 2px; }
  .space-habit-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .space-habit-dot {
    justify-self: center;
    width: 9px;
    height: 9px;
    border-radius: 3px;
    background: var(--space-widget-line);
  }
  .space-habit-dot.is-part { background: var(--space-widget-fill); }
  .space-habit-dot.is-full { background: currentColor; opacity: 0.8; }
  .space-habit-day.is-today { opacity: 1; font-weight: 700; }
  .space-habit-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--space-card-line);
    border-radius: 5px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.7;
  }
  .space-habit-check.is-done {
    opacity: 1;
    background: var(--space-widget-surface-strong);
  }
  .space-habit-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0;
  }
  .space-habit-row:hover .space-habit-remove { opacity: 0.5; }

  .space-clock-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: baseline;
    gap: 8px;
    font-size: 11.5px;
  }
  .space-clock-city { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; opacity: 0.72; }
  .space-clock-offset { font-size: 9px; opacity: 0.45; }
  .space-clock-time { font-variant-numeric: tabular-nums; font-weight: 600; letter-spacing: 0.02em; }

  .space-question { font-size: 12.5px; line-height: 1.55; opacity: 0.9; }

  @media (hover: none) {
    .space-row-remove, .space-habit-remove { opacity: 0.5; }
  }
`;
