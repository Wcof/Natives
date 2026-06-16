'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, useLocale, type Locale } from '@/i18n';
import { Clipboard, X, Plus, Zap, Link2 } from 'lucide-react';
import { onThemeChange, TERMINAL_THEMES } from '@/lib/theme-engine';
import { recordTerminalActivity, setFileFollow, followChange } from '@/lib/follow-mode';
import { playDoneChime, playAskChime } from '@/lib/chime';
import { parseAgentAction } from '@/lib/agent-narration';
import { copyToClipboard } from '@/lib/clipboard';

interface TerminalSession {
  id: string;
  label: string;
  term: unknown;
  fitAddon: unknown;
  active: boolean;
  profileId?: string;
}

interface TerminalPanelProps {
  isCollapsed: boolean;
  onToggle: () => void;
  height: number;
  onResize: (height: number) => void;
  isMaximized: boolean;
  onMaximizeToggle: () => void;
  onSessionCreated?: (sessionId: string) => void;
  followMode?: boolean;
  onFollowModeToggle?: () => void;
}

export default function TerminalPanel({
  isCollapsed,
  onToggle,
  height,
  onResize,
  isMaximized,
  onMaximizeToggle,
  onSessionCreated,
  followMode,
  onFollowModeToggle,
}: TerminalPanelProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [isDragOver, setIsDragOver] = useState(false);
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const locale = useLocale();
  const terminalRef = useRef<HTMLDivElement>(null);
  const sessionMapRef = useRef<Map<string, TerminalSession>>(new Map());

  // Profile state (US26)
  const [profiles, setProfiles] = useState<Array<{ id: number; name: string; is_default: number }>>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<number | null>(null);
  const [isAgentBusy, setIsAgentBusy] = useState(false);

  // Load environment profiles (US26)
  useEffect(() => {
    async function loadProfiles() {
      try {
        const api = window.nativesAPI;
        if (!api?.env) return;
        const list = await api.env.listProfiles();
        const profileList = list as Array<{ id: number; name: string; is_default: number }>;
        setProfiles(profileList);
        // Auto-select default profile
        const defaultProfile = profileList.find((p) => p.is_default === 1);
        if (defaultProfile) {
          setSelectedProfileId(defaultProfile.id);
        } else if (profileList.length > 0) {
          setSelectedProfileId(profileList[0]!.id);
        }
      } catch { /* ignore */ }
    }
    loadProfiles();
  }, []);

  // Terminal breathing glow animation on agent idle + tabpulse busy
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.status === 'idle' && terminalRef.current?.parentElement) {
        terminalRef.current.parentElement.classList.add('anim-termAwait');
        setIsAgentBusy(false);
        setTimeout(() => {
          terminalRef.current?.parentElement?.classList.remove('anim-termAwait');
        }, 3000);
      } else if (detail?.status === 'busy' || detail?.status === 'working') {
        setIsAgentBusy(true);
      }
    };
    window.addEventListener('agent-status-changed', handler);
    return () => window.removeEventListener('agent-status-changed', handler);
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startY = e.clientY;
    const startH = height;

    const handleMove = (ev: MouseEvent) => {
      const delta = startY - ev.clientY;
      const newHeight = Math.max(200, Math.min(window.innerHeight * 0.5, startH + delta));
      onResize(newHeight);
    };
    const handleUp = () => {
      setIsDragging(false);
      document.removeEventListener('mousemove', handleMove);
      document.removeEventListener('mouseup', handleUp);
    };
    document.addEventListener('mousemove', handleMove);
    document.addEventListener('mouseup', handleUp);
  }, [height, onResize]);

  // Create a new terminal session
  const createSession = useCallback(async (label?: string, profileId?: number) => {
    const container = terminalRef.current;
    if (!container) return;

    const [{ Terminal }, { FitAddon }, { WebLinksAddon }] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
      import('@xterm/addon-web-links'),
    ]);

    const term = new Terminal({
      fontSize: 13,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: 'var(--terminal-bg, #131410)',
        foreground: 'var(--terminal-fg, #f2f2ea)',
        cursor: 'var(--terminal-cursor, #FFF5E6)',
        selectionBackground: 'var(--accent-soft, #FFF5E633)',
      },
      cursorBlink: true,
      scrollback: 5000,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Clickable links (HTTP/HTTPS URLs and file paths)
    const webLinksAddon = new WebLinksAddon((e, uri) => {
      if (uri.startsWith('http://') || uri.startsWith('https://')) {
        window.open(uri, '_blank');
      } else if (uri.startsWith('/')) {
        // File path — navigate file browser
        window.dispatchEvent(new CustomEvent('navigate-files', { detail: uri }));
      }
    });
    term.loadAddon(webLinksAddon);

    // Create PTY session via IPC
    const api = window.nativesAPI;
    if (!api?.terminal?.create) {
      term.writeln('\x1b[31mTerminal API not available\x1b[0m');
      return;
    }

    const { sessionId, error } = await api.terminal.create(profileId ? String(profileId) : undefined);
    if (error || !sessionId) {
      term.writeln(`\x1b[31mFailed to create terminal: ${error || 'unknown error'}\x1b[0m`);
      return;
    }

    const profileName = profiles.find((p) => p.id === profileId)?.name || '';
    const sessionLabel = label || `${profileName ? profileName + ' ' : ''}zsh ${sessions.length + 1}`;
    const session: TerminalSession = {
      id: sessionId,
      label: sessionLabel,
      term,
      fitAddon,
      active: true,
      profileId: profileId ? String(profileId) : undefined,
    };

    sessionMapRef.current.set(sessionId, session);
    setSessions((prev) => [...prev, session]);
    setActiveSessionId(sessionId);
    onSessionCreated?.(sessionId);

    // Hide all other terminal containers, show this one
    const allContainers = container.querySelectorAll('[data-terminal-session]');
    allContainers.forEach((el) => { (el as HTMLElement).style.display = 'none'; });

    // Create container for this session
    const sessionContainer = document.createElement('div');
    sessionContainer.setAttribute('data-terminal-session', sessionId);
    sessionContainer.style.cssText = 'width:100%;height:100%;';
    container.appendChild(sessionContainer);

    term.open(sessionContainer);
    try { fitAddon.fit(); } catch { /* ignore */ }

    // Send user input to PTY
    term.onData((data: string) => {
      api.terminal.write(sessionId, data);
    });

    // Receive PTY output
    const termBuffer: string[] = [];
    api.onDbStateChanged((_event: unknown, channel: string, data: unknown) => {
      if (channel === 'terminal:data' && (data as { sessionId?: string })?.sessionId === sessionId) {
        const output = (data as { data?: string })?.data;
        if (output) {
          term.write(output);
          // Track activity for follow mode
          recordTerminalActivity(sessionId);
          // Buffer lines for narration parsing
          termBuffer.push(output);
          if (termBuffer.length > 50) termBuffer.splice(0, termBuffer.length - 50);
          // Check for completion/approval patterns
          if (/esc to interrupt/i.test(output)) {
            setIsAgentBusy(true);
          }
          if (/\? for.*options|Do you want|approve|Y\/n/i.test(output)) {
            playAskChime();
          }
        }
      }
      // Terminal exit
      if (channel === 'terminal:exit' && (data as { sessionId?: string })?.sessionId === sessionId) {
        setIsAgentBusy(false);
        playDoneChime();
      }
    });

    // Resize handling
    term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
      api.terminal.resize(sessionId, cols, rows);
    });
  }, [sessions.length, onSessionCreated, profiles]);

  // Auto-create first session when terminal opens
  useEffect(() => {
    if (isCollapsed) return;
    if (sessions.length === 0) {
      createSession('zsh', selectedProfileId ?? undefined);
    }
  }, [isCollapsed, sessions.length, createSession, selectedProfileId]);

  // Switch active session
  const switchSession = useCallback((sessionId: string) => {
    const container = terminalRef.current;
    if (!container) return;

    // Hide all, show target
    const allContainers = container.querySelectorAll('[data-terminal-session]');
    allContainers.forEach((el) => {
      (el as HTMLElement).style.display =
        el.getAttribute('data-terminal-session') === sessionId ? 'block' : 'none';
    });

    setActiveSessionId(sessionId);

    // Fit the newly visible terminal
    const session = sessionMapRef.current.get(sessionId);
    if (session) {
      const fa = session.fitAddon as { fit: () => void };
      try { fa.fit(); } catch { /* ignore */ }
    }
  }, []);

  // Close session
  const closeSession = useCallback((sessionId: string) => {
    const session = sessionMapRef.current.get(sessionId);
    if (!session) return;

    // Kill PTY
    window.nativesAPI?.terminal?.kill(sessionId);

    // Dispose terminal
    const term = session.term as { dispose?: () => void };
    if (term?.dispose) term.dispose();

    // Remove DOM element
    const container = terminalRef.current;
    if (container) {
      const el = container.querySelector(`[data-terminal-session="${sessionId}"]`);
      el?.remove();
    }

    // Remove from map
    sessionMapRef.current.delete(sessionId);

    // Update state
    setSessions((prev) => {
      const remaining = prev.filter((s) => s.id !== sessionId);
      if (remaining.length === 0) {
        // No sessions left, collapse terminal
        onToggle();
        return [];
      }
      // Switch to the last remaining session
      const nextSession = remaining[remaining.length - 1]!;
      setActiveSessionId(nextSession.id);
      // Show the next session's container
      if (container) {
        const nextEl = container.querySelector(`[data-terminal-session="${nextSession.id}"]`);
        if (nextEl) (nextEl as HTMLElement).style.display = 'block';
      }
      return remaining;
    });
  }, [onToggle]);

  // Fit on resize
  useEffect(() => {
    if (isCollapsed || !activeSessionId) return;
    const session = sessionMapRef.current.get(activeSessionId);
    if (session) {
      const fa = session.fitAddon as { fit: () => void };
      try { fa.fit(); } catch { /* ignore */ }
    }
  }, [height, isCollapsed, isMaximized, activeSessionId]);

  // Terminal keyboard shortcuts: Cmd+T new tab, Cmd+W close tab, Cmd+Shift+]/[ switch tabs
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      // Cmd+T: new terminal tab
      if (e.key === 't' && !e.shiftKey && !e.ctrlKey) {
        // Only if terminal panel is focused or visible
        const termEl = terminalRef.current;
        if (termEl && (termEl.contains(document.activeElement) || !isCollapsed)) {
          e.preventDefault();
          createSession();
        }
      }
      // Cmd+W: close current tab
      if (e.key === 'w' && !e.shiftKey) {
        const termEl = terminalRef.current;
        if (termEl && termEl.contains(document.activeElement) && sessions.length > 1 && activeSessionId) {
          e.preventDefault();
          closeSession(activeSessionId);
        }
      }
      // Cmd+Shift+] / Cmd+Shift+[: next/prev tab
      if (e.key === '}' || e.key === '{') {
        const termEl = terminalRef.current;
        if (termEl && (termEl.contains(document.activeElement) || !isCollapsed)) {
          e.preventDefault();
          const idx = sessions.findIndex(s => s.id === activeSessionId);
          if (idx < 0) return;
          const nextIdx = e.key === '}' ? (idx + 1) % sessions.length : (idx - 1 + sessions.length) % sessions.length;
          if (sessions[nextIdx]) switchSession(sessions[nextIdx].id);
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [sessions, activeSessionId, isCollapsed, createSession, closeSession, switchSession]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      sessionMapRef.current.forEach((session) => {
        window.nativesAPI?.terminal?.kill(session.id);
        const term = session.term as { dispose?: () => void };
        if (term?.dispose) term.dispose();
      });
      sessionMapRef.current.clear();
    };
  }, []);

  // Listen for theme changes and update xterm ANSI colors dynamically
  useEffect(() => {
    return onThemeChange((themeId: string) => {
      const terminalTheme = TERMINAL_THEMES[themeId];
      if (!terminalTheme) return;

      sessionMapRef.current.forEach((session) => {
        const term = session.term as { setOption?: (key: string, value: unknown) => void };
        if (term?.setOption) {
          term.setOption('theme', {
            background: terminalTheme.background,
            foreground: terminalTheme.foreground,
            cursor: terminalTheme.cursor,
            selectionBackground: terminalTheme.selectionBackground || terminalTheme.cursor + '33',
          });
        }
      });
    });
  }, []);

  // Agent launch: reuse current tab if it's a plain shell, otherwise create new
  const launchAgent = useCallback(async (cmd: string) => {
    if (!activeSessionId) {
      // No session, create one then send command
      await createSession(undefined, selectedProfileId ?? undefined);
      // After creation, the new session becomes active
      return;
    }
    // Send command to current session
    window.nativesAPI?.terminal?.write?.(activeSessionId, cmd + '\r');
  }, [activeSessionId, createSession, selectedProfileId]);

  // CWD detection (macOS — lsof)
  const refreshCwd = useCallback(async (sessionId: string) => {
    try {
      const result = await window.nativesAPI?.terminal?.cwd?.(sessionId);
      if (result?.ok && result.cwd) {
        // Dispatch CWD change for follow mode
        followChange(result.cwd, '', result.cwd);
      }
    } catch { /* ignore */ }
  }, []);

  // Drag-drop file support
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);

    if (!activeSessionId) return;
    const session = sessionMapRef.current.get(activeSessionId);
    if (!session) return;

    // Get file paths from the drop
    const files = Array.from(e.dataTransfer.files);
    const paths = files.map((f) => (f as unknown as { path?: string }).path || f.name).filter(Boolean);

    if (paths.length > 0) {
      // Insert quoted paths into the terminal
      const pathStr = paths.map((p) => `"${p}"`).join(' ');
      const api = window.nativesAPI;
      if (api?.terminal?.write) {
        api.terminal.write(activeSessionId, pathStr);
      }
    }
  }, [activeSessionId]);

  return (
    <div
      className={`terminal-panel ${isCollapsed ? 'collapsed' : ''} ${isMaximized ? 'terminal-maximized' : ''}`}
      style={{ height: isMaximized ? '100%' : isCollapsed ? 0 : height }}
      role="terminal"
      aria-label={t(locale, 'terminal.title')}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Drag handle */}
      <div
        className={`terminal-drag-handle ${isDragging ? 'active' : ''}`}
        onMouseDown={handleMouseDown}
      />

      <div className="terminal-header">
        <div className="terminal-tabs">
          {sessions.map((session) => (
            <div
              key={session.id}
              className={`terminal-tab ${session.id === activeSessionId ? 'active' : ''}${isAgentBusy && session.id === activeSessionId ? ' anim-tabpulse' : ''}`}
              onClick={() => switchSession(session.id)}
              style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4 }}
            >
              <span>{session.label}</span>
              {sessions.length > 1 && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    closeSession(session.id);
                  }}
                  style={{
                    background: 'none', border: 'none', color: 'inherit',
                    cursor: 'pointer', padding: 0, fontSize: 10, opacity: 0.5,
                    lineHeight: 1,
                  }}
                  title={t(locale, 'terminal.closeTab')}
                >
                  <X size={10} />
                </button>
              )}
            </div>
          ))}
          <button
            className="terminal-tab"
            onClick={() => createSession(undefined, selectedProfileId ?? undefined)}
            style={{ cursor: 'pointer', opacity: 0.6, fontSize: 14, padding: '2px 8px' }}
            title={t(locale, 'terminal.newTab')}
          >
            <Plus size={14} />
          </button>
        </div>

        {/* Environment profile selector (US26) */}
        {profiles.length > 0 && (
          <select
            value={selectedProfileId ?? ''}
            onChange={(e) => setSelectedProfileId(e.target.value ? Number(e.target.value) : null)}
            style={{
              fontSize: 10, padding: '1px 4px', marginRight: 8,
              background: 'var(--bg-3,#1c1e17)', color: 'var(--text)',
              border: '1px solid var(--border,#262920)', borderRadius: 4,
              cursor: 'pointer', maxWidth: 120,
            }}
            aria-label={t(locale, 'terminal.selectProfile')}
          >
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        )}

        <div className="terminal-actions">
          {/* Agent launch buttons */}
          <button
            className="btn-ghost"
            onClick={() => launchAgent('claude --dangerously-skip-permissions')}
            style={{
              fontSize: 10, padding: '2px 6px', borderRadius: 4,
              color: 'var(--text-dim,#9b9d8c)',
            }}
            title="Launch Claude Code"
          >
            <Zap size={11} style={{ marginRight: 2 }} /> Claude
          </button>
          <button
            className="btn-ghost"
            onClick={() => launchAgent('codex')}
            style={{
              fontSize: 10, padding: '2px 6px', borderRadius: 4,
              color: 'var(--text-dim,#9b9d8c)',
            }}
            title="Launch Codex"
          >
            <Zap size={11} style={{ marginRight: 2 }} /> Codex
          </button>
          {/* Follow mode toggle */}
          {onFollowModeToggle && (
            <button
              className="btn-ghost"
              onClick={onFollowModeToggle}
              style={{
                fontSize: 10, padding: '2px 6px', borderRadius: 4,
                color: followMode ? 'var(--accent,#FFF5E6)' : 'var(--text-faint,#62655a)',
                background: followMode ? 'var(--accent-soft,#FFF5E61f)' : 'transparent',
              }}
              title={followMode ? t(locale, 'terminal.followModeOn') : t(locale, 'terminal.followModeOff')}
              aria-label={t(locale, 'terminal.ariaToggleFollowMode')}
            >
              <Link2 size={12} />
            </button>
          )}
          {/* Copy selection to clipboard */}
          <button
            className="btn-ghost"
            onClick={async () => {
              const session = sessions.find(s => s.id === activeSessionId);
              if (!session) return;
              const term = session.term as { selectAll?: () => void; getSelection?: () => string; clearSelection?: () => void } | undefined;
              if (!term) return;
              try {
                term.selectAll?.();
                const text = term.getSelection?.() || '';
                term.clearSelection?.();
                if (text) {
                  const ok = await copyToClipboard(text);
                  if (ok) playDoneChime();
                }
              } catch { /* ignore */ }
            }}
            style={{ fontSize: 10, padding: '2px 6px', borderRadius: 4, color: 'var(--text-dim,#9b9d8c)' }}
            title="Copy selection"
            aria-label="Copy selection"
          >
            <Clipboard size={12} />
          </button>
          <button className="btn-ghost" onClick={onMaximizeToggle} aria-label={isMaximized ? t(locale, 'terminal.ariaRestore') : t(locale, 'terminal.ariaMaximize')}>
            <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              {isMaximized ? (
                <path d="M8 3v3a2 2 0 01-2 2H3m18 0h-3a2 2 0 01-2-2V3m0 18v-3a2 2 0 012-2h3M3 16h3a2 2 0 012 2v3" />
              ) : (
                <path d="M8 3H5a2 2 0 00-2 2v3m18 0V5a2 2 0 00-2-2h-3m0 18h3a2 2 0 002-2v-3M3 16v3a2 2 0 002 2h3" />
              )}
            </svg>
          </button>
          <button className="btn-ghost" onClick={onToggle} aria-label={isCollapsed ? t(locale, 'terminal.ariaOpen') : t(locale, 'terminal.ariaClose')}>
            <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              {isCollapsed ? (
                <path d="M18 15l-6-6-6 6" />
              ) : (
                <path d="M6 9l6 6 6-6" />
              )}
            </svg>
          </button>
        </div>
      </div>

      <div style={{ position: 'relative', width: '100%', height: '100%' }}>
        <div
          ref={terminalRef}
          className="terminal-body"
          id="terminal-container"
          style={{ width: '100%', height: '100%', overflow: 'hidden' }}
        />
        {/* Drop overlay */}
        {isDragOver && (
          <div style={{
            position: 'absolute', inset: 0,
            background: 'var(--accent-soft,#FFF5E61f)',
            border: '2px dashed var(--accent,#FFF5E6)',
            borderRadius: 4,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'var(--accent,#FFF5E6)', fontSize: 13, fontWeight: 600,
            zIndex: 10, pointerEvents: 'none',
          }}>
            {t(locale, 'terminal.dropPrompt')}
          </div>
        )}
      </div>
    </div>
  );
}
