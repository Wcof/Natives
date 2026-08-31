import assert from 'node:assert/strict';

// 测试环境 Mock
globalThis.$ = () => null;

function renderInlineMarkdown(source, container) {
  if (!source) return;
  const pattern = /(`[^`]+`|!?\[[^\]]*\]\([^) \t\r\n]+(?:\s+["'][^"']*["'])?\)|(?:\*\*|__)(?:[^*_]+|\*(?!\*)|\_(?!\_))+(?:\*\*|__)|~~[^~]+~~|(?:\*|_)(?:[^*_]+)+(?:\*|_))/g;
  let lastIndex = 0; let match;
  while ((match = pattern.exec(source)) !== null) {
    if (match.index > lastIndex) container.append({ type: 'text', text: source.slice(lastIndex, match.index) });
    const token = match[0]; lastIndex = pattern.lastIndex;
    if (token.startsWith('`') && token.endsWith('`') && token.length >= 2) {
      container.append({ type: 'code', text: token.slice(1, -1) }); continue;
    }
    if (token.startsWith('![') || token.startsWith('[')) {
      const isImg = token.startsWith('!');
      const linkMatch = token.match(/^!?\[([^\]]*)\]\(([^)\s]+)(?:\s+["']([^"']*)["'])?\)$/);
      if (linkMatch) {
        const text = linkMatch[1]; const rawUrl = linkMatch[2];
        if (isImg) {
          container.append({ type: 'img', alt: text, src: rawUrl });
        } else if (/^https?:\/\//i.test(rawUrl)) {
          container.append({ type: 'a', href: rawUrl, text: text || rawUrl });
        } else {
          container.append({ type: 'text', text: token });
        }
        continue;
      }
    }
    if ((token.startsWith('**') && token.endsWith('**')) || (token.startsWith('__') && token.endsWith('__'))) {
      const child = { type: 'strong', children: [] };
      child.append = (n) => child.children.push(n);
      renderInlineMarkdown(token.slice(2, -2), child);
      container.append(child); continue;
    }
    if (token.startsWith('~~') && token.endsWith('~~')) {
      const child = { type: 'del', children: [] };
      child.append = (n) => child.children.push(n);
      renderInlineMarkdown(token.slice(2, -2), child);
      container.append(child); continue;
    }
    if ((token.startsWith('*') && token.endsWith('*')) || (token.startsWith('_') && token.endsWith('_'))) {
      const child = { type: 'em', children: [] };
      child.append = (n) => child.children.push(n);
      renderInlineMarkdown(token.slice(1, -1), child);
      container.append(child); continue;
    }
    container.append({ type: 'text', text: token });
  }
  if (lastIndex < source.length) container.append({ type: 'text', text: source.slice(lastIndex) });
}

// 1. 测试 '**内网禅道**' 独立加粗
{
  const container = { children: [], append(n) { this.children.push(n); } };
  renderInlineMarkdown('**内网禅道**', container);
  assert.equal(container.children.length, 1);
  assert.equal(container.children[0].type, 'strong');
  assert.equal(container.children[0].children[0].text, '内网禅道');
}

// 2. 测试在列表中嵌套加粗及前后文本: '- **内网禅道**：这是管理系统'
{
  const line = '- **内网禅道**：这是管理系统'.replace(/^\s*[-*+]\s+/, '');
  const container = { children: [], append(n) { this.children.push(n); } };
  renderInlineMarkdown(line, container);
  assert.equal(container.children[0].type, 'strong');
  assert.equal(container.children[0].children[0].text, '内网禅道');
  assert.equal(container.children[1].type, 'text');
  assert.equal(container.children[1].text, '：这是管理系统');
}

// 3. 测试行内代码与链接混合: '参考 `code` 和 [禅道](https://example.com)'
{
  const container = { children: [], append(n) { this.children.push(n); } };
  renderInlineMarkdown('参考 `code` 和 [禅道](https://example.com)', container);
  assert.equal(container.children[0].type, 'text');
  assert.equal(container.children[1].type, 'code');
  assert.equal(container.children[1].text, 'code');
  assert.equal(container.children[3].type, 'a');
  assert.equal(container.children[3].text, '禅道');
  assert.equal(container.children[3].href, 'https://example.com');
}

console.log('✓ files markdown unit tests passed');
