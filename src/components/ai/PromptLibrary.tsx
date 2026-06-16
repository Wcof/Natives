'use client';

import { useState, useCallback, useEffect } from 'react';
import { t, type Locale } from '@/i18n';

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
      <div style={{ padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)' }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 6 }}>
          {t(locale,'aiWorkbench.promptLibrary.title')}
        </div>
        <div style={{ display: 'flex', gap: 4 }}>
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
placeholder={t(locale,'aiWorkbench.promptLibrary.searchPlaceholder')}
            style={{
              flex: 1, fontSize: 11, padding: '4px 8px',
              background: 'var(--bg,#0b0c0a)', border: '1px solid var(--border,#262920)',
              borderRadius: 4, color: 'var(--text)', outline: 'none',
            }}
          />
          <button className="btn btn-primary" onClick={() => { setEditId(null); setEditTitle(''); setEditContent(''); setEditTags(''); setShowEditor(true); }} style={{ fontSize: 10, padding: '3px 8px' }}>
            {t(locale,'aiWorkbench.promptLibrary.newPrompt')}
          </button>
        </div>
      </div>

      {/* Prompt list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {filtered.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {search ? t(locale,'aiWorkbench.promptLibrary.noMatching') : t(locale,'aiWorkbench.promptLibrary.empty')}
          </div>
        ) : (
          filtered.map((prompt) => (
            <div key={prompt.id} style={{
              padding: '8px 10px', marginBottom: 4, borderRadius: 6,
              border: '1px solid var(--border,#262920)', background: 'var(--bg-2,#131410)',
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 4 }}>
                <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{prompt.title}</div>
                <div style={{ display: 'flex', gap: 4 }}>
                  <button className="btn-ghost" onClick={() => handleEdit(prompt)} style={{ fontSize: 9, padding: '1px 4px' }}>✏️</button>
                  <button className="btn-ghost" onClick={() => handleDelete(prompt.id)} style={{ fontSize: 9, padding: '1px 4px', color: 'var(--danger)' }}>🗑</button>
                </div>
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-dim)', marginBottom: 4, whiteSpace: 'pre-wrap', maxHeight: 40, overflow: 'hidden' }}>
                {prompt.content.slice(0, 120)}{prompt.content.length > 120 ? '...' : ''}
              </div>
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {prompt.tags.map((tag) => (
                  <span key={tag} style={{
                    fontSize: 9, padding: '1px 5px', borderRadius: 3,
                    background: 'var(--accent-soft,#cdf24b1f)', color: 'var(--accent,#cdf24b)',
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
            background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
            borderRadius: 10, padding: 16, width: 440, maxWidth: '90vw',
          }} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 12 }}>
              {editId ? 'Edit Prompt' : 'New Prompt'}
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <input
                type="text"
                value={editTitle}
                onChange={(e) => setEditTitle(e.target.value)}
                placeholder="Prompt title"
                style={{
                  width: '100%', padding: '6px 8px', fontSize: 12,
                  background: 'var(--bg,#0b0c0a)', border: '1px solid var(--border,#262920)',
                  borderRadius: 4, color: 'var(--text)', outline: 'none',
                }}
                autoFocus
              />
              <textarea
                value={editContent}
                onChange={(e) => setEditContent(e.target.value)}
                placeholder="Prompt content..."
                rows={6}
                style={{
                  width: '100%', padding: '6px 8px', fontSize: 11, fontFamily: 'var(--font-mono)',
                  background: 'var(--bg,#0b0c0a)', border: '1px solid var(--border,#262920)',
                  borderRadius: 4, color: 'var(--text)', outline: 'none', resize: 'vertical',
                }}
              />
              <input
                type="text"
                value={editTags}
                onChange={(e) => setEditTags(e.target.value)}
                placeholder="Tags (comma separated: code-review, typescript)"
                style={{
                  width: '100%', padding: '6px 8px', fontSize: 11,
                  background: 'var(--bg,#0b0c0a)', border: '1px solid var(--border,#262920)',
                  borderRadius: 4, color: 'var(--text)', outline: 'none',
                }}
              />
            </div>
            <div style={{ display: 'flex', gap: 8, marginTop: 12, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setShowEditor(false)} style={{ fontSize: 11 }}>Cancel</button>
              <button className="btn btn-primary" onClick={handleSave} style={{ fontSize: 11 }} disabled={!editTitle.trim() || !editContent.trim()}>
                Save
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
