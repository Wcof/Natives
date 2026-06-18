'use client';

import { useState, useEffect } from 'react';
import { Keyboard, X } from 'lucide-react';
import { useLocale } from '@/i18n';
import { t } from '@/i18n';

interface Shortcut {
  keys: string[];
  description: string;
}

function getShortcuts(locale: string): Shortcut[] {
  return [
    { keys: ['⌘', 'B'], description: t(locale, 'shortcuts.toggleSidebar') },
    { keys: ['⌘', 'K'], description: t(locale, 'shortcuts.commandPalette') },
    { keys: ['⌘', 'Shift', 'K'], description: t(locale, 'shortcuts.focusSidebar') },
    { keys: ['⌘', 'N'], description: t(locale, 'shortcuts.notifications') },
    { keys: ['⌘', '['], description: t(locale, 'shortcuts.back') },
    { keys: ['⌘', ']'], description: t(locale, 'shortcuts.forward') },
    { keys: ['⌘', 'T'], description: t(locale, 'shortcuts.newTerminalTab') },
    { keys: ['⌘', 'W'], description: t(locale, 'shortcuts.closeTerminalTab') },
    { keys: ['⌘', 'Shift', ']'], description: t(locale, 'shortcuts.nextTerminalTab') },
    { keys: ['⌘', 'Shift', '['], description: t(locale, 'shortcuts.prevTerminalTab') },
    { keys: ['Escape'], description: t(locale, 'shortcuts.closePreview') },
  ];
}

/**
 * Shortcut Help Overlay — shows all keyboard shortcuts.
 * Triggered by Cmd+/ or via the sidebar help button.
 */
export default function ShortcutHelp() {
  const [visible, setVisible] = useState(false);
  const locale = useLocale();

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Cmd+/ to toggle
      if (e.metaKey && e.key === '/') {
        e.preventDefault();
        setVisible(v => !v);
      }
      // Escape to close
      if (e.key === 'Escape' && visible) {
        setVisible(false);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [visible]);

  if (!visible) return null;

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 10002,
        background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(8px)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}
      onClick={(e) => { if (e.target === e.currentTarget) setVisible(false); }}
    >
      <div style={{
        background: 'var(--vibe-toolbar-bg)',
        border: '0.0625rem solid var(--vibe-toolbar-border)',
        borderRadius: '1rem',
        padding: '24px 32px',
        maxWidth: 480,
        width: '90%',
        boxShadow: 'var(--vibe-toolbar-shadow)',
        backdropFilter: 'blur(28px) saturate(145%)',
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          marginBottom: 20,
        }}>
          <h2 style={{ fontSize: 16, fontWeight: 600, color: 'var(--vibe-brand-text)', margin: 0 }}>
            <Keyboard size={16} /> {t(locale, 'shortcuts.title')}
          </h2>
          <button
            onClick={() => setVisible(false)}
            style={{
              background: 'none', border: 'none', color: 'var(--text-faint)',
              fontSize: 18, cursor: 'pointer', padding: '0 4px',
            }}
          >
            <X size={16} />
          </button>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {getShortcuts(locale).map((s, i) => (
            <div key={i} style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              padding: '6px 0',
              borderBottom: i < getShortcuts(locale).length - 1 ? '0.0625rem solid var(--vibe-toolbar-border)' : 'none',
            }}>
              <span style={{ fontSize: 13, color: 'var(--vibe-btn-text)' }}>
                {s.description}
              </span>
              <div style={{ display: 'flex', gap: 4 }}>
                {s.keys.map((key, j) => (
                  <kbd key={j} style={{
                    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                    minWidth: 24, height: 22, padding: '0 6px',
                    background: 'var(--vibe-btn-bg)',
                    border: '0.0625rem solid var(--vibe-btn-border)',
                    borderRadius: 4,
                    fontSize: 11, fontWeight: 600, color: 'var(--vibe-btn-text)',
                    fontFamily: 'var(--font-mono)',
                  }}>
                    {key}
                  </kbd>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div style={{
          marginTop: 16, textAlign: 'center',
          fontSize: 11, color: 'var(--vibe-btn-text)',
        }}>
          Press <kbd style={{
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            minWidth: 20, height: 18, padding: '0 4px',
            background: 'var(--vibe-btn-bg)',
            border: '0.0625rem solid var(--vibe-btn-border)',
            borderRadius: 3, fontSize: 10, fontWeight: 600, color: 'var(--vibe-btn-text)',
            fontFamily: 'var(--font-mono)', margin: '0 2px',
          }}>⌘</kbd>+<kbd style={{
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            minWidth: 20, height: 18, padding: '0 4px',
            background: 'var(--vibe-btn-bg)',
            border: '0.0625rem solid var(--vibe-btn-border)',
            borderRadius: 3, fontSize: 10, fontWeight: 600, color: 'var(--vibe-btn-text)',
            fontFamily: 'var(--font-mono)', margin: '0 2px',
          }}>/</kbd> to toggle
        </div>
      </div>
    </div>
  );
}
