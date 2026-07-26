'use client';

import { useEffect, useRef, useCallback, useState } from 'react';
import { semanticEqual } from '@/lib/markdown-semantic';
import { t, type Locale } from '@/i18n';

interface MilkdownEditorProps {
  content: string;
  filePath: string;
  onSave: (content: string) => void;
  /** 输入即置脏、写盘后清脏；供外部变更热重载判断是否可以静默重读 */
  onDirtyChange?: (dirty: boolean) => void;
  /** 有损锁定横幅的文案语言 */
  locale?: Locale;
}

/**
 * Milkdown Crepe WYSIWYG Markdown editor.
 * Auto-saves after 0.8s idle. Cmd+S saves immediately; unmount flushes
 * pending edits (guardDirty — 切文件/关预览不丢内容).
 * YAML frontmatter is preserved (stripped before Crepe, prepended back on save).
 * 语义无损校验（fanbox semanticSig）：Crepe 归一化产物与原文渲染比对，
 * 往返有损 → 锁只读并禁写盘，绝不静默丢内容（源码可用代码模式改）。
 */
export default function MilkdownEditor({ content, onSave, onDirtyChange, locale = 'zh' }: MilkdownEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<any>(null);
  const getValueRef = useRef<() => string>(() => content);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const baselineRef = useRef<string>(content);
  /** 往返有损锁：true 时禁止一切写盘 */
  const lossyRef = useRef(false);
  const [lossyLocked, setLossyLocked] = useState(false);

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
          color: 'var(--warning, #b8860b)',
          background: 'color-mix(in srgb, var(--warning, #b8860b) 10%, transparent)',
          borderBottom: '1px solid color-mix(in srgb, var(--warning, #b8860b) 30%, transparent)',
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
