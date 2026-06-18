'use client';

import { useState, useCallback, useEffect } from 'react';
import { Edit2, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface PromptItem {
  id: string;
  title: string;
  content: string;
  tags: string[];
  createdAt: number;
}

const STORAGE_KEY = 'natives:prompts';

function loadPrompts(): PromptItem[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch { return []; }
}

function savePrompts(prompts: PromptItem[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prompts));
  } catch { /* ignore */ }
}

export default function PromptLibrary() {
  const [prompts, setPrompts] = useState<PromptItem[]>([]);
  const [search, setSearch] = useState('');
  const [showEditor, setShowEditor] = useState(false);
  const [editTitle, setEditTitle] = useState('');
  const [editContent, setEditContent] = useState('');
  const [editTags, setEditTags] = useState('');
  const [editId, setEditId] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
    setPrompts(loadPrompts());
  }, []);

  const handleSave = useCallback(() => {
    if (!editTitle.trim() || !editContent.trim()) return;
    const tags = editTags.split(',').map((t) => t.trim()).filter(Boolean);
    if (editId) {
      setPrompts((prev) => {
        const next = prev.map((p) => p.id === editId ? { ...p, title: editTitle.trim(), content: editContent.trim(), tags } : p);
        savePrompts(next);
        return next;
      });
    } else {
      const newPrompt: PromptItem = {
        id: Date.now().toString(36),
        title: editTitle.trim(),
        content: editContent.trim(),
        tags,
        createdAt: Date.now(),
      };
      setPrompts((prev) => {
        const next = [newPrompt, ...prev];
        savePrompts(next);
        return next;
      });
    }
    setShowEditor(false);
    setEditId(null);
    setEditTitle('');
    setEditContent('');
    setEditTags('');
  }, [editTitle, editContent, editTags, editId]);

  const handleEdit = useCallback((prompt: PromptItem) => {
    setEditId(prompt.id);
    setEditTitle(prompt.title);
    setEditContent(prompt.content);
    setEditTags(prompt.tags.join(', '));
    setShowEditor(true);
  }, []);

  const handleDelete = useCallback((id: string) => {
    setPrompts((prev) => {
      const next = prev.filter((p) => p.id !== id);
      savePrompts(next);
      return next;
    });
  }, []);

  const filtered = prompts.filter((p) => {
    if (!search) return true;
    const q = search.toLowerCase();
    return p.title.toLowerCase().includes(q) || p.content.toLowerCase().includes(q) || p.tags.some((t) => t.toLowerCase().includes(q));
  });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{ padding: '8px 10px', borderBottom: '1px solid var(--vibe-btn-border)' }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 6 }}>
          {t(locale,'aiWorkbench.promptLibrary.title')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs }}>
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
placeholder={t(locale,'aiWorkbench.promptLibrary.searchPlaceholder')}
            style={{
              flex: 1, fontSize: FONT_SIZE.sm, padding: '4px 8px',
              background: 'var(--vibe-content-bg)', border: '1px solid var(--vibe-btn-border)',
              borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', outline: 'none',
            }}
          />
          <button className="btn btn-primary" onClick={() => { setEditId(null); setEditTitle(''); setEditContent(''); setEditTags(''); setShowEditor(true); }} style={{ fontSize: FONT_SIZE.xs, padding: '3px 8px' }}>
            {t(locale,'aiWorkbench.promptLibrary.newPrompt')}
          </button>
        </div>
      </div>

      {/* Prompt list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {filtered.length === 0 ? (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
            {search ? t(locale,'aiWorkbench.promptLibrary.noMatching') : t(locale,'aiWorkbench.promptLibrary.empty')}
          </div>
        ) : (
          filtered.map((prompt) => (
            <div key={prompt.id} style={{
              padding: '8px 10px', marginBottom: SPACING.xs, borderRadius: BORDER_RADIUS.md,
              border: '1px solid var(--vibe-btn-border)', background: 'var(--vibe-toolbar-bg)',
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: SPACING.xs }}>
                <div style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text)' }}>{prompt.title}</div>
                <div style={{ display: 'flex', gap: SPACING.xs, alignItems: 'center' }}>
                  <button className="btn-ghost" onClick={() => handleEdit(prompt)} style={{ padding: '2px 4px', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }} title="Edit">
                    <Edit2 size={10} />
                  </button>
                  <button className="btn-ghost" onClick={() => handleDelete(prompt.id)} style={{ padding: '2px 4px', color: 'var(--danger)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }} title="Delete">
                    <Trash2 size={10} />
                  </button>
                </div>
              </div>
              <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', marginBottom: SPACING.xs, whiteSpace: 'pre-wrap', maxHeight: 40, overflow: 'hidden' }}>
                {prompt.content.slice(0, 120)}{prompt.content.length > 120 ? '...' : ''}
              </div>
              <div style={{ display: 'flex', gap: SPACING.xs, flexWrap: 'wrap' }}>
                {prompt.tags.map((tag) => (
                  <span key={tag} style={{
                    fontSize: 9, padding: '1px 5px', borderRadius: BORDER_RADIUS.sm,
                    background: 'var(--accent-soft)', color: 'var(--accent)',
                  }}>
                    #{tag}
                  </span>
                ))}
              </div>
            </div>
          ))
        )}
      </div>

      {/* Editor dialog */}
      {showEditor && (
        <div style={{
          position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60,
        }} onClick={() => setShowEditor(false)}>
          <div style={{
            background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
            borderRadius: BORDER_RADIUS.xl, padding: 'var(--space-md)', width: 440, maxWidth: '90vw',
            boxShadow: 'var(--vibe-toolbar-shadow)',
            backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
            WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          }} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 'var(--fs-md)', fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.md }}>
              {editId ? 'Edit Prompt' : 'New Prompt'}
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-sm)' }}>
              <input
                type="text"
                value={editTitle}
                onChange={(e) => setEditTitle(e.target.value)}
                placeholder="Prompt title"
                style={{
                  width: '100%', padding: '6px 8px', fontSize: 'var(--fs-sm)',
                  background: 'var(--vibe-content-bg)', border: '1px solid var(--vibe-btn-border)',
                  borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', outline: 'none',
                }}
                autoFocus
              />
              <textarea
                value={editContent}
                onChange={(e) => setEditContent(e.target.value)}
                placeholder="Prompt content..."
                rows={6}
                style={{
                  width: '100%', padding: '6px 8px', fontSize: FONT_SIZE.sm, fontFamily: 'var(--font-mono)',
                  background: 'var(--vibe-content-bg)', border: '1px solid var(--vibe-btn-border)',
                  borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', outline: 'none', resize: 'vertical',
                }}
              />
              <input
                type="text"
                value={editTags}
                onChange={(e) => setEditTags(e.target.value)}
                placeholder="Tags (comma separated: code-review, typescript)"
                style={{
                  width: '100%', padding: '6px 8px', fontSize: FONT_SIZE.sm,
                  background: 'var(--vibe-content-bg)', border: '1px solid var(--vibe-btn-border)',
                  borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', outline: 'none',
                }}
              />
            </div>
            <div style={{ display: 'flex', gap: 'var(--space-sm)', marginTop: SPACING.md, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setShowEditor(false)} style={{ fontSize: FONT_SIZE.sm }}>Cancel</button>
              <button className="btn btn-primary" onClick={handleSave} style={{ fontSize: FONT_SIZE.sm }} disabled={!editTitle.trim() || !editContent.trim()}>
                Save
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
