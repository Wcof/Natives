'use client';

import { FollowMode, useFollowMode } from '@/lib/follow-mode';

export default function FollowModeUI({
  currentDir,
  onNavigate,
}: {
  currentDir: string;
  onNavigate: (path: string) => void;
}) {
  const { mode, cycleMode, terminalFollows, fileBrowserFollows } = useFollowMode();

  const modeLabel: Record<FollowMode, string> = {
    off: 'Follow: Off',
    'terminal-follow': 'Follow: Terminal → File',
    'file-follow': 'Follow: File → Terminal',
  };

  const modeIcon: Record<FollowMode, string> = {
    off: '○',
    'terminal-follow': '⇄',
    'file-follow': '⇄',
  };

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 6,
      padding: '4px 8px', fontSize: 11,
    }}>
      <button
        className="btn btn-ghost"
        onClick={cycleMode}
        title={modeLabel[mode]}
        style={{
          padding: '2px 8px', fontSize: 11,
          color: mode !== 'off' ? 'var(--accent)' : 'var(--text-dim)',
          border: mode !== 'off' ? '1px solid var(--accent)' : '1px solid var(--border)',
        }}
      >
        {modeIcon[mode]} {mode !== 'off' ? 'ON' : 'OFF'}
      </button>

      {terminalFollows && (
        <span style={{ color: 'var(--text-faint)', fontSize: 10 }}>
          cd → {currentDir || '/'}
        </span>
      )}

      {fileBrowserFollows && (
        <span style={{ color: 'var(--text-faint)', fontSize: 10 }}>
          Terminal cd syncs browser
        </span>
      )}
    </div>
  );
}
