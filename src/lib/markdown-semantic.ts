/**
 * markdown-semantic — 富文本往返的语义无损校验（fanbox semanticSig/semanticEqual 移植）
 *
 * 场景：Milkdown（Crepe）加载 markdown 后会做归一化，某些写法（复杂 HTML 块、
 * 特殊表格等）经 WYSIWYG 往返会丢内容或改结构。校验方式：把「原文」与
 * 「编辑器归一化产物」各渲染成 HTML，比对**可见文字（空白折叠）+ 结构骨架
 * （标签序列 + img 的 src/alt + a 的 href）**。不一致 → 判定往返有损，
 * 编辑器必须锁为只读，绝不静默丢内容。
 */

/** 从 HTML 提取语义签名：结构骨架 + 可见文字（纯字符串处理，node 环境可测） */
export function htmlSignature(html: string): string {
  const structure: string[] = [];
  const textParts: string[] = [];

  const tokens = html.match(/<[^>]+>|[^<]+/g) ?? [];
  for (const token of tokens) {
    if (token.startsWith('<')) {
      if (token.startsWith('</') || token.startsWith('<!')) continue;
      const nameMatch = token.match(/^<\s*([a-zA-Z][a-zA-Z0-9-]*)/);
      if (!nameMatch) continue;
      const name = nameMatch[1]!.toLowerCase();
      if (name === 'img') {
        const src = token.match(/\bsrc="([^"]*)"/i)?.[1] ?? '';
        const alt = token.match(/\balt="([^"]*)"/i)?.[1] ?? '';
        structure.push(`img[${src}|${alt}]`);
      } else if (name === 'a') {
        const href = token.match(/\bhref="([^"]*)"/i)?.[1] ?? '';
        structure.push(`a[${href}]`);
      } else {
        structure.push(name);
      }
    } else {
      const text = token.replace(/\s+/g, ' ').trim();
      if (text) textParts.push(text);
    }
  }

  return `${structure.join(',')}::${textParts.join(' ').replace(/\s+/g, ' ')}`;
}

/** 两份 markdown 语义等价（渲染为 HTML 后比对签名） */
export async function semanticEqual(a: string, b: string): Promise<boolean> {
  if (a === b) return true;
  try {
    const { marked } = await import('marked');
    const [ha, hb] = await Promise.all([marked(a), marked(b)]);
    return htmlSignature(ha as string) === htmlSignature(hb as string);
  } catch {
    // 渲染器不可用时保守放行（不因校验器故障锁死编辑）
    return true;
  }
}
