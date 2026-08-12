'use client';

import { useEffect, useRef, useCallback, useState } from 'react';
import type { Crepe } from '@milkdown/crepe';
import { semanticEqual } from '@/lib/markdown-semantic';
import { findMatches, FIND_MATCH_LIMIT } from '@/lib/find-replace';
import { t, type Locale } from '@/i18n';

/** 审计收口 #12：Milkdown 查找替换的只读接口（ProseMirror transaction 改内存模型）。 */
export interface MilkdownFindReplace {
  find(query: string, caseSensitive?: boolean): { index: number; count: number };
  replaceOne(replacement: string): boolean;
  replaceAll(replacement: string): number;
}

interface MilkdownEditorProps {
  content: string;
  filePath: string;
  onSave: (content: string) => void;
  /** 输入即置脏、写盘后清脏；供外部变更热重载判断是否可以静默重读 */
  onDirtyChange?: (dirty: boolean) => void;
  /** 有损锁定横幅的文案语言 */
  locale?: Locale;
  /** 审计收口 #12：把查找替换 handle 交给父级（FindReplaceBar 驱动）。 */
  onFindReplaceReady?: (handle: MilkdownFindReplace | null) => void;
}

/**
 * Milkdown Crepe WYSIWYG Markdown editor.
 * Auto-saves after 0.8s idle. Cmd+S saves immediately; unmount flushes
 * pending edits (guardDirty — 切文件/关预览不丢内容).
 * YAML frontmatter is preserved (stripped before Crepe, prepended back on save).
 * 语义无损校验（fanbox semanticSig）：Crepe 归一化产物与原文渲染比对，
 * 往返有损 → 锁只读并禁写盘，绝不静默丢内容（源码可用代码模式改）。
 */
export default function MilkdownEditor({ content, onSave, onDirtyChange, locale = 'zh', onFindReplaceReady }: MilkdownEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<Crepe | null>(null);
  const getValueRef = useRef<() => string>(() => content);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const baselineRef = useRef<string>(content);
  /** 往返有损锁：true 时禁止一切写盘 */
  const lossyRef = useRef(false);
  const [lossyLocked, setLossyLocked] = useState(false);
  /** 审计收口 #12：查找替换 handle 的稳定引用（避免 effect 反复重建）。 */
  const onFindReplaceReadyRef = useRef(onFindReplaceReady);
  onFindReplaceReadyRef.current = onFindReplaceReady;

  // YAML frontmatter protection
  const frontmatterMatch = content.match(/^(---\r?\n[\s\S]*?\r?\n---\r?\n)/);
  const frontmatter = frontmatterMatch?.[1] || '';
  const bodyContent = frontmatter ? content.slice(frontmatter.length) : content;

  const queueSave = useCallback(() => {
    if (lossyRef.current) return; // 有损锁：禁写盘
    onDirtyChange?.(true);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      const fullContent = frontmatter + getValueRef.current();
      if (fullContent !== baselineRef.current) {
        baselineRef.current = fullContent;
        onSave(fullContent);
      }
      onDirtyChange?.(false);
    }, 800);
  }, [frontmatter, onSave, onDirtyChange]);

  const flushSave = useCallback(() => {
    if (lossyRef.current) return; // 有损锁：禁写盘
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    const fullContent = frontmatter + getValueRef.current();
    if (fullContent !== baselineRef.current) {
      baselineRef.current = fullContent;
      onSave(fullContent);
    }
    onDirtyChange?.(false);
  }, [frontmatter, onSave, onDirtyChange]);

  useEffect(() => {
    if (!hostRef.current) return;
    let disposed = false;
    const host = hostRef.current;

    (async () => {
      try {
        const { Crepe } = await import('@milkdown/crepe');
        if (disposed) return;

        const editor = new Crepe({
          root: host,
          defaultValue: bodyContent,
        });

        await editor.create();
        if (disposed) { try { editor.destroy(); } catch { /* no-op */ } return; }

        editorRef.current = editor;
        getValueRef.current = () => editor.getMarkdown();

        // Set baseline after Crepe normalizes content
        baselineRef.current = frontmatter + editor.getMarkdown();

        // 审计收口 #12：Milkdown 查找替换——ProseMirror transaction 改内存模型，
        // 替换后走 dirty→autosave→expectedMtime 冲突链（queueSave），不直接改 DOM。
        try {
          const { editorViewCtx } = await import('@milkdown/core');
          // CrepeBuilder 的 `editor` getter 返回 @milkdown/kit/core Editor，
          // 其 `action` 可从 ctx 读取 ProseMirror EditorView。
          const view = editor.editor.action((ctx) => ctx.get(editorViewCtx));
          if (view) {
            const matchesRef: Array<{ from: number; to: number }> = [];
            let activeIndex = -1;
            const handle: MilkdownFindReplace = {
              find(query, caseSensitive = false) {
                matchesRef.length = 0;
                activeIndex = -1;
                const trimmed = query.trim();
                if (!trimmed) return { index: -1, count: 0 };
                // 收集全部 text node 及其 doc position（不直接改 DOM）。
                const doc = view.state.doc;
                const textNodes: Array<{ text: string; from: number }> = [];
                doc.descendants((node, pos) => {
                  if (node.isText && node.text != null) {
                    textNodes.push({ text: node.text, from: pos });
                  }
                  return true;
                });
                let full = '';
                const starts: number[] = [];
                for (const t of textNodes) {
                  starts.push(full.length);
                  full += t.text;
                }
                const matches = findMatches(full, trimmed, { caseSensitive, limit: FIND_MATCH_LIMIT });
                for (const m of matches) {
                  // 定位偏移落在哪个 text node → doc position。
                  const idx = starts.findIndex((s, i) => m.start < s + (textNodes[i]?.text.length ?? 0));
                  const segIndex = idx === -1 ? starts.length - 1 : idx;
                  const node = textNodes[segIndex];
                  if (!node) continue;
                  const nodeOffset = m.start - (starts[segIndex] ?? 0);
                  const from = node.from + nodeOffset;
                  const to = node.from + Math.min(m.end - m.start, node.text.length - nodeOffset);
                  matchesRef.push({ from, to });
                }
                if (matchesRef.length > 0) activeIndex = 0;
                return { index: activeIndex, count: matchesRef.length };
              },
              replaceOne(replacement) {
                if (activeIndex < 0 || activeIndex >= matchesRef.length) return false;
                const { from, to } = matchesRef[activeIndex]!;
                const tr = view.state.tr.insertText(replacement, from, to);
                view.dispatch(tr);
                queueSave();
                matchesRef.splice(activeIndex, 1);
                if (activeIndex >= matchesRef.length) activeIndex = matchesRef.length - 1;
                return true;
              },
              replaceAll(replacement) {
                if (matchesRef.length === 0) return 0;
                let count = 0;
                // 从后往前替换，避免偏移失效。
                for (let i = matchesRef.length - 1; i >= 0; i -= 1) {
                  const { from, to } = matchesRef[i]!;
                  const tr = view.state.tr.insertText(replacement, from, to);
                  view.dispatch(tr);
                  count += 1;
                }
                queueSave();
                matchesRef.length = 0;
                activeIndex = -1;
                return count;
              },
            };
            onFindReplaceReadyRef.current?.(handle);
          }
        } catch {
          // ProseMirror view 不可用（老版本/异常）→ 不提供替换，只读 Preview 查找兜底。
        }

        // 语义无损校验：Crepe 归一化产物 vs 原文。有损 → 锁只读 + 禁写盘
        void semanticEqual(bodyContent, editor.getMarkdown()).then((ok) => {
          if (disposed || ok) return;
          lossyRef.current = true;
          setLossyLocked(true);
          try { (editor as { setReadonly?: (v: boolean) => void }).setReadonly?.(true); } catch { /* 老版本无此 API，靠禁写盘兜底 */ }
        });

        // Listen for input
        host.addEventListener('input', queueSave, true);

        // Cmd+S
        host.addEventListener('keydown', (e: KeyboardEvent) => {
          if ((e.metaKey || e.ctrlKey) && e.key === 's') {
            e.preventDefault();
            e.stopPropagation();
            flushSave();
          }
        }, true);
      } catch (err) {
        console.warn('[MilkdownEditor] Crepe load failed:', err);
        // Fallback: render as plain editable div
        if (!disposed && host) {
          host.contentEditable = 'true';
          host.textContent = bodyContent;
          host.style.cssText = 'padding:16px;font-family:var(--font-mono,monospace);font-size:13px;line-height:1.7;color:var(--text);outline:none;min-height:100%;white-space:pre-wrap;';
          host.addEventListener('input', () => {
            getValueRef.current = () => (host.textContent || '');
            queueSave();
          });
        }
      }
    })();

    return () => {
      disposed = true;
      // guardDirty：卸载前静默 flush 未保存内容（切文件/关预览不丢字）
      try { flushSave(); } catch { /* no-op */ }
      if (editorRef.current) {
        try { editorRef.current.destroy(); } catch { /* no-op */ }
        editorRef.current = null;
      }
    };
  }, [bodyContent, frontmatter, queueSave, flushSave]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      {lossyLocked && (
        <div style={{
          padding: '6px 12px', fontSize: 12, lineHeight: 1.5,
          color: 'var(--warning)',
          background: 'color-mix(in srgb, var(--warning) 10%, transparent)',
          borderBottom: '1px solid color-mix(in srgb, var(--warning) 30%, transparent)',
        }}>
          {t(locale, 'filePreview.lossyLocked')}
        </div>
      )}
      <div
        ref={hostRef}
        className="milkdown-host"
        style={{ flex: 1, overflow: 'auto' }}
      />
    </div>
  );
}
