'use client';

import { useEffect, useRef, useCallback } from 'react';

interface MilkdownEditorProps {
  content: string;
  filePath: string;
  onSave: (content: string) => void;
}

/**
 * Milkdown Crepe WYSIWYG Markdown editor.
 * Auto-saves after 0.8s idle. Cmd+S saves immediately.
 * YAML frontmatter is preserved (stripped before Crepe, prepended back on save).
 */
export default function MilkdownEditor({ content, onSave }: MilkdownEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<any>(null);
  const getValueRef = useRef<() => string>(() => content);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const baselineRef = useRef<string>(content);

  // YAML frontmatter protection
  const frontmatterMatch = content.match(/^(---\r?\n[\s\S]*?\r?\n---\r?\n)/);
  const frontmatter = frontmatterMatch?.[1] || '';
  const bodyContent = frontmatter ? content.slice(frontmatter.length) : content;

  const queueSave = useCallback(() => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      const fullContent = frontmatter + getValueRef.current();
      if (fullContent !== baselineRef.current) {
        baselineRef.current = fullContent;
        onSave(fullContent);
      }
    }, 800);
  }, [frontmatter, onSave]);

  const flushSave = useCallback(() => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    const fullContent = frontmatter + getValueRef.current();
    if (fullContent !== baselineRef.current) {
      baselineRef.current = fullContent;
      onSave(fullContent);
    }
  }, [frontmatter, onSave]);

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
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (editorRef.current) {
        try { editorRef.current.destroy(); } catch { /* no-op */ }
        editorRef.current = null;
      }
    };
  }, [bodyContent, frontmatter, queueSave, flushSave]);

  return (
    <div
      ref={hostRef}
      className="milkdown-host"
      style={{ minHeight: '100%', overflow: 'auto' }}
    />
  );
}
