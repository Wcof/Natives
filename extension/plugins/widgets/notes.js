/**
 * Notes Widget.
 * Clean direct Markdown/plain text preview with click-to-edit mode matching TablissNG Notes.sass.
 */

import { escapeHtml } from '../sanitizer.js';

export const notesWidget = {
  key: 'widget/notes',
  name: 'Notes',
  defaultData: {
    content: '',
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k, onDataChange } = {}) {
    container.className = 'Widget Notes';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'notes-content';

    const content = (data.content || '').trim();

    const statsEl = document.createElement('div');
    statsEl.className = 'note-stats-bar';
    statsEl.textContent = computeNoteStats(content);

    // View element (Rendered view)
    const viewEl = document.createElement('div');
    viewEl.className = 'notes-view';
    if (content) {
      viewEl.innerHTML = formatNoteContent(content);
    } else {
      viewEl.innerHTML = `<span class="placeholder"><svg class="icon" aria-hidden="true" style="width:13px;height:13px;display:inline-block;vertical-align:-2px;"><use href="#i-pen" /></svg> <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
    }

    // Edit textarea (hidden by default)
    const editArea = document.createElement('textarea');
    editArea.className = 'notes-textarea';
    editArea.placeholder = t('clickToWriteNote', '点击此处记录便签...');
    editArea.value = data.content || '';
    editArea.hidden = true;

    function enterEdit() {
      viewEl.hidden = true;
      editArea.hidden = false;
      editArea.style.height = `${Math.max(80, viewEl.offsetHeight + 10)}px`;
      editArea.focus();
    }

    function exitEdit() {
      editArea.hidden = true;
      viewEl.hidden = false;
      const nextContent = editArea.value.trim();
      statsEl.textContent = computeNoteStats(nextContent);
      if (nextContent) {
        viewEl.innerHTML = formatNoteContent(nextContent);
      } else {
        viewEl.innerHTML = `<span class="placeholder"><svg class="icon" aria-hidden="true" style="width:13px;height:13px;display:inline-block;vertical-align:-2px;"><use href="#i-pen" /></svg> <span>${escapeHtml(t('clickToWriteNote', '点击此处记录便签...'))}</span></span>`;
      }
      if (nextContent !== (data.content || '').trim() && onDataChange) {
        onDataChange({ ...data, content: editArea.value });
      }
    }

    viewEl.onclick = (e) => {
      e.stopPropagation();
      enterEdit();
    };

    editArea.onblur = () => exitEdit();
    editArea.oninput = () => {
      editArea.style.height = 'auto';
      editArea.style.height = `${Math.max(80, editArea.scrollHeight)}px`;
      statsEl.textContent = computeNoteStats(editArea.value);
      if (onDataChange) {
        onDataChange({ ...data, content: editArea.value });
      }
    };

    if (content) root.append(statsEl);
    root.append(viewEl, editArea);

    const toolbar = document.createElement('div');
    toolbar.className = 'notes-quick-bar';
    toolbar.innerHTML = `
      <button type="button" class="notes-chip-btn" data-act="prompt" title="快速插入通用 AI 提示词框架">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-bolt" /></svg>
        <span>Prompt 框架</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="arch" title="快速插入架构方案设计模板">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-grid" /></svg>
        <span>方案设计</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="diag" title="快速插入 Bug 根因排查模板">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-alert" /></svg>
        <span>根因排查</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="review" title="快速插入代码审查 Prompt">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-target" /></svg>
        <span>代码审查</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="code" title="快速插入代码块草稿">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-code" /></svg>
        <span>代码草稿</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="copy" title="一键复制全部便签内容">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-copy" /></svg>
        <span>复制</span>
      </button>
      <button type="button" class="notes-chip-btn" data-act="export" title="导出为 Markdown 文件">
        <svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-download" /></svg>
        <span>导出 .md</span>
      </button>
    `;

    toolbar.onclick = (e) => {
      const btn = e.target.closest('.notes-chip-btn');
      if (!btn) return;
      e.stopPropagation();
      const act = btn.dataset.act;
      if (act === 'copy') {
        const curText = editArea.value || data.content || '';
        if (curText) {
          navigator.clipboard?.writeText(curText);
          const span = btn.querySelector('span');
          if (span) span.textContent = '已复制';
          setTimeout(() => { if (span) span.textContent = '复制'; }, 1500);
        }
      } else if (act === 'prompt') {
        const tpl = `## 角色设定\n你是一名资深的软件架构师与全栈工程师。\n\n## 任务目标\n\n## 上下文与约束\n- 遵循生产环境质量与安全标准\n- 确保代码具备可测试性与向后兼容性\n`;
        applyTemplate(tpl);
      } else if (act === 'arch') {
        const tpl = `## 技术方案设计\n### 1. 背景与业务诉求\n\n### 2. 核心设计原则与边界（Seam）\n\n### 3. 数据流与关键时序\n\n### 4. 容灾降级与性能考虑\n\n### 5. 验证指标与回滚计划\n`;
        applyTemplate(tpl);
      } else if (act === 'diag') {
        const tpl = `## Bug 根因排查\n### 1. 现象与复现步骤\n\n### 2. 异常日志与报错堆栈\n\n### 3. 根因推演与最小假设\n\n### 4. 防御性修复策略\n`;
        applyTemplate(tpl);
      } else if (act === 'code') {
        const tpl = `\`\`\`typescript\n// 快速测试脚本 / AI 生成代码片段\nfunction executeTask() {\n  \n}\n\`\`\``;
        applyTemplate(tpl);
      } else if (act === 'review') {
        const tpl = `## 代码审查指令\n请审查以下代码，关注：\n1. 边界异常与防御性处理\n2. 潜在竞态与并发安全性\n3. 算法复杂度与内存泄漏隐患\n\n\`\`\`\n\n\`\`\``;
        applyTemplate(tpl);
      } else if (act === 'export') {
        const curText = editArea.value || data.content || '';
        if (curText) {
          const blob = new Blob([curText], { type: 'text/markdown;charset=utf-8' });
          const url = URL.createObjectURL(blob);
          const a = document.createElement('a');
          a.href = url;
          a.download = `note-${new Date().toISOString().slice(0, 10)}.md`;
          a.click();
          URL.revokeObjectURL(url);
          const span = btn.querySelector('span');
          if (span) span.textContent = '已导出';
          setTimeout(() => { if (span) span.textContent = '导出 .md'; }, 1500);
        }
      }
    };

    function applyTemplate(tpl) {
      const nextVal = editArea.value ? `${editArea.value}\n\n${tpl}` : tpl;
      editArea.value = nextVal;
      viewEl.innerHTML = formatNoteContent(nextVal);
      statsEl.textContent = computeNoteStats(nextVal);
      if (!root.contains(statsEl)) root.prepend(statsEl);
      onDataChange?.({ ...data, content: nextVal });
    }

    root.append(toolbar);
    container.append(root);

    return () => container.replaceChildren();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('notesNotice', '便签支持在 Dashboard 直接点击浏览与实时编辑，失去焦点自动保存。');
    container.append(notice);
  },
  styles: `
    .Notes:not(.markdown-content), .Notes .notes-view { white-space:pre-wrap; }
    .Notes .notes-view { cursor:pointer; overflow-wrap:anywhere; }
    .Notes .placeholder { display:flex; align-items:center; gap:.5em; font-style:italic; }
    .Notes .note-stats-bar {
      font-size: 11px;
      opacity: 0.65;
      text-align: right;
      margin-bottom: 4px;
      font-variant-numeric: tabular-nums;
    }
    .Notes .notes-textarea {
      width: 100%;
      min-height: 80px;
      background:transparent;
      border:1px solid currentColor;
      color:inherit;
      font-family: inherit;
      font-size:inherit;
      resize: vertical;
      box-sizing: border-box;
    }
    .Notes .notes-quick-bar {
      display: flex;
      gap: 6px;
      margin-top: 8px;
      flex-wrap: wrap;
    }
    .Notes .notes-chip-btn {
      min-height: 22px;
      height: 22px;
      padding: 0 7px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.25);
      background: rgba(255,255,255,0.08);
      color: inherit;
      font-size: 11px;
      cursor: pointer;
      opacity: 0.75;
      transition: all .15s ease;
      display: inline-flex;
      align-items: center;
      gap: 4px;
    }
    .Notes .notes-chip-btn:hover {
      opacity: 1;
      background: rgba(255,255,255,0.18);
      border-color: rgba(255,255,255,0.4);
    }
    .Notes .note-code-block {
      background: rgba(0,0,0,0.3);
      padding: 8px 10px;
      border-radius: 6px;
      font-family: monospace;
      font-size: 12px;
      overflow-x: auto;
      margin: 6px 0;
      border: 1px solid rgba(255,255,255,0.15);
      text-align: left;
    }
    .Notes .note-inline-code {
      background: rgba(255,255,255,0.15);
      padding: 1px 4px;
      border-radius: 3px;
      font-family: monospace;
      font-size: 0.9em;
    }
    .Notes .note-task {
      display: flex;
      align-items: center;
      gap: 6px;
      margin: 2px 0;
      text-align: left;
    }
    .Notes .note-task.done {
      text-decoration: line-through;
      opacity: 0.6;
    }
    .Notes .note-bullet {
      display: flex;
      align-items: baseline;
      gap: 6px;
      margin: 2px 0;
      text-align: left;
    }
    .Notes h2, .Notes h3, .Notes h4 {
      margin: 6px 0 2px 0;
      font-weight: 600;
    }
  `,
};

function computeNoteStats(text) {
  const clean = (text || '').trim();
  if (!clean) return '';
  const len = clean.length;
  const mins = Math.max(1, Math.ceil(len / 300));
  return `${len} 字 · 约 ${mins} 分钟`;
}

function formatNoteContent(text) {
  if (!text) return '';
  const escaped = escapeHtml(text);
  const withCodeBlocks = escaped.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (match, lang, code) => {
    return `<pre class="note-code-block"><code>${code.trim()}</code></pre>`;
  });
  const lines = withCodeBlocks.split('\n');
  const formatted = lines.map((line) => {
    if (/^###\s+(.+)$/.test(line)) return line.replace(/^###\s+(.+)$/, '<h4>$1</h4>');
    if (/^##\s+(.+)$/.test(line)) return line.replace(/^##\s+(.+)$/, '<h3>$1</h3>');
    if (/^#\s+(.+)$/.test(line)) return line.replace(/^#\s+(.+)$/, '<h2>$1</h2>');
    if (/^[-*]\s+\[ \]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+\[ \]\s+(.+)$/, '<div class="note-task"><input type="checkbox" disabled /> <span>$1</span></div>');
    }
    if (/^[-*]\s+\[[xX]\]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+\[[xX]\]\s+(.+)$/, '<div class="note-task done"><input type="checkbox" checked disabled /> <span>$1</span></div>');
    }
    if (/^[-*]\s+(.+)$/.test(line)) {
      return line.replace(/^[-*]\s+(.+)$/, '<div class="note-bullet">• <span>$1</span></div>');
    }
    return line;
  });
  let html = formatted.join('<br>');
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/`([^`]+)`/g, '<code class="note-inline-code">$1</code>');
  return html;
}
