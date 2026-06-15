'use client';

import { useState, useEffect } from 'react';
import { useLocale } from '@/i18n';
import { t } from '@/i18n';

interface Shortcut {
  keys: string[];
  description: string;
}

const SHORTCUTS: Shortcut[] = [
  { keys: ['⌘', 'B'], description: 'Toggle sidebar' },
  { keys: ['⌘', 'K'], description: 'Command palette' },
  { keys: ['⌘', 'Shift', 'K'], description: 'Focus sidebar' },
  { keys: ['⌘', 'N'], description: 'Notifications' },
  { keys: ['⌘', '['], description: 'Back (File Browser)' },
  { keys: ['⌘', ']'], description: 'Forward (File Browser)' },
  { keys: ['⌘', 'T'], description: 'New terminal tab' },
  { keys: ['⌘', 'W'], description: 'Close terminal tab' },
  { keys: ['⌘', 'Shift', ']'], description: 'Next terminal tab' },
  { keys: ['⌘', 'Shift', '['], description: 'Previous terminal tab' },
  { keys: ['Escape'], description: 'Close preview / palette' },
];

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
        background: 'var(--bg-2, #131410)',
        border: '1px solid var(--border, #262920)',
        borderRadius: 12,
        padding: '24px 32px',
        maxWidth: 480,
        width: '90%',
        boxShadow: '0 20px 60px rgba(0,0,0,0.5)',
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          marginBottom: 20,
        }}>
          <h2 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text)', margin: 0 }}>
            ⌨️ Keyboard Shortcuts
          </h2>
          <button
            onClick={() => setVisible(false)}
            style={{
              background: 'none', border: 'none', color: 'var(--text-faint)',
              fontSize: 18, cursor: 'pointer', padding: '0 4px',
            }}
          >
            ✕
          </button>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {SHORTCUTS.map((s, i) => (
            <div key={i} style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              padding: '6px 0',
              borderBottom: i < SHORTCUTS.length - 1 ? '1px solid var(--border, #262920)' : 'none',
            }}>
              <span style={{ fontSize: 13, color: 'var(--text-dim, #9b9d8c)' }}>
                {s.description}
              </span>
              <div style={{ display: 'flex', gap: 4 }}>
                {s.keys.map((key, j) => (
                  <kbd key={j} style={{
                    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                    minWidth: 24, height: 22, padding: '0 6px',
                    background: 'var(--bg-3, #1c1e17)',
                    border: '1px solid var(--border, #262920)',
                    borderRadius: 4,
                    fontSize: 11, fontWeight: 600, color: 'var(--text)',
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
          fontSize: 11, color: 'var(--text-faint, #62655a)',
        }}>
          Press <kbd style={{
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            minWidth: 20, height: 18, padding: '0 4px',
            background: 'var(--bg-3, #1c1e17)',
            border: '1px solid var(--border, #262920)',
            borderRadius: 3, fontSize: 10, fontWeight: 600, color: 'var(--text)',
            fontFamily: 'var(--font-mono)', margin: '0 2px',
          }}>⌘</kbd>+<kbd style={{
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            minWidth: 20, height: 18, padding: '0 4px',
            background: 'var(--bg-3, #1c1e17)',
            border: '1px solid var(--border, #262920)',
            borderRadius: 3, fontSize: 10, fontWeight: 600, color: 'var(--text)',
            fontFamily: 'var(--font-mono)', margin: '0 2px',
          }}>/</kbd> to toggle
        </div>
      </div>
    </div>
  );
}
