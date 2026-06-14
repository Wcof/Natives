'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';

interface TerminalSession {
  id: string;
  label: string;
  term: unknown;
  fitAddon: unknown;
  active: boolean;
}

interface TerminalPanelProps {
  isCollapsed: boolean;
  onToggle: () => void;
  height: number;
  onResize: (height: number) => void;
  isMaximized: boolean;
  onMaximizeToggle: () => void;
  onSessionCreated?: (sessionId: string) => void;
}

export default function TerminalPanel({
  isCollapsed,
  onToggle,
  height,
  onResize,
  isMaximized,
  onMaximizeToggle,
  onSessionCreated,
}: TerminalPanelProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const terminalRef = useRef<HTMLDivElement>(null);
  const sessionMapRef = useRef<Map<string, TerminalSession>>(new Map());

  // Load locale
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
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
  const createSession = useCallback(async (label?: string) => {
    const container = terminalRef.current;
    if (!container) return;

    const [{ Terminal }, { FitAddon }] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
    ]);

    const term = new Terminal({
      fontSize: 13,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#131410',
        foreground: '#f2f2ea',
        cursor: '#cdf24b',
        selectionBackground: '#cdf24b33',
      },
      cursorBlink: true,
      scrollback: 5000,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Create PTY session via IPC
    const api = window.nativesAPI;
    if (!api?.terminal?.create) {
      term.writeln('\x1b[31mTerminal API not available\x1b[0m');
      return;
    }

    const { sessionId, error } = await api.terminal.create();
    if (error || !sessionId) {
      term.writeln(`\x1b[31mFailed to create terminal: ${error || 'unknown error'}\x1b[0m`);
      return;
    }

    const sessionLabel = label || `zsh ${sessions.length + 1}`;
    const session: TerminalSession = {
      id: sessionId,
      label: sessionLabel,
      term,
      fitAddon,
      active: true,
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
    api.onDbStateChanged((_event: unknown, channel: string, data: unknown) => {
      if (channel === 'terminal:data' && (data as { sessionId?: string })?.sessionId === sessionId) {
        const output = (data as { data?: string })?.data;
        if (output) term.write(output);
      }
    });

    // Resize handling
    term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
      api.terminal.resize(sessionId, cols, rows);
    });
  }, [sessions.length, onSessionCreated]);

  // Auto-create first session when terminal opens
  useEffect(() => {
    if (isCollapsed) return;
    if (sessions.length === 0) {
      createSession('zsh');
    }
  }, [isCollapsed, sessions.length, createSession]);

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

  return (
    <div
      className={`terminal-panel ${isCollapsed ? 'collapsed' : ''} ${isMaximized ? 'terminal-maximized' : ''}`}
      style={{ height: isMaximized ? '100%' : isCollapsed ? 0 : height }}
      role="region"
      aria-label="Terminal"
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
              className={`terminal-tab ${session.id === activeSessionId ? 'active' : ''}`}
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
                  ✕
                </button>
              )}
            </div>
          ))}
          <button
            className="terminal-tab"
            onClick={() => createSession()}
            style={{ cursor: 'pointer', opacity: 0.6, fontSize: 14, padding: '2px 8px' }}
            title={t(locale, 'terminal.newTab')}
          >
            +
          </button>
        </div>
        <div className="terminal-actions">
          <button className="btn-ghost" onClick={onMaximizeToggle} aria-label={isMaximized ? 'Restore terminal' : 'Maximize terminal'}>
            <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              {isMaximized ? (
                <path d="M8 3v3a2 2 0 01-2 2H3m18 0h-3a2 2 0 01-2-2V3m0 18v-3a2 2 0 012-2h3M3 16h3a2 2 0 012 2v3" />
              ) : (
                <path d="M8 3H5a2 2 0 00-2 2v3m18 0V5a2 2 0 00-2-2h-3m0 18h3a2 2 0 002-2v-3M3 16v3a2 2 0 002 2h3" />
              )}
            </svg>
          </button>
          <button className="btn-ghost" onClick={onToggle} aria-label={isCollapsed ? 'Open terminal' : 'Close terminal'}>
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

      <div
        ref={terminalRef}
        className="terminal-body"
        id="terminal-container"
        style={{ width: '100%', height: '100%', overflow: 'hidden', position: 'relative' }}
      />
    </div>
  );
}
