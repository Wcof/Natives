'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { onThemeChange, TERMINAL_THEMES } from '@/lib/theme-engine';
import { recordTerminalActivity, followChange } from '@/lib/follow-mode';
import { recordScrollbackLine } from '@/lib/path-detector';
import { playDoneChime, playAskChime } from '@/lib/chime';
import { parseAgentAction } from '@/lib/agent-narration';
import { FONT_SIZE } from '@/lib/design-tokens';

export interface TerminalSession {
  id: string;
  label: string;
  term: unknown;
  fitAddon: unknown;
  active: boolean;
  profileId?: string;
  /** IPC listener cleanup functions (prevent memory leaks) */
  unsubscribers?: Array<() => void>;
}

export interface UseTerminalSessionsOptions {
  onSessionCreated?: (sessionId: string) => void;
  profiles: Array<{ id: number; name: string; is_default: number }>;
  muted: boolean;
}

export interface UseTerminalSessionsReturn {
  sessions: TerminalSession[];
  setSessions: React.Dispatch<React.SetStateAction<TerminalSession[]>>;
  activeSessionId: string | null;
  setActiveSessionId: React.Dispatch<React.SetStateAction<string | null>>;
  sessionMapRef: React.RefObject<Map<string, TerminalSession>>;
  terminalRef: React.RefObject<HTMLDivElement | null>;
  exitedSessionIds: React.RefObject<Set<string>>;
  createSession: (label?: string, profileId?: number) => Promise<string | undefined>;
  closeSession: (sessionId: string) => void;
  switchSession: (sessionId: string) => void;
  refreshCwd: (sessionId: string) => Promise<void>;
  isAgentBusy: boolean;
  setIsAgentBusy: React.Dispatch<React.SetStateAction<boolean>>;
}

export function useTerminalSessions({
  onSessionCreated,
  profiles,
  muted,
}: UseTerminalSessionsOptions): UseTerminalSessionsReturn {
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const terminalRef = useRef<HTMLDivElement>(null);
  const sessionMapRef = useRef<Map<string, TerminalSession>>(new Map());
  const sessionCounterRef = useRef(0);
  const exitedSessionIds = useRef<Set<string>>(new Set());
  const [isAgentBusy, setIsAgentBusy] = useState(false);

  // Create a new terminal session
  const createSession = useCallback(async (label?: string, profileId?: number) => {
    const container = terminalRef.current;
    if (!container) return;

    const [{ Terminal }, { FitAddon }, { WebLinksAddon }, { Unicode11Addon }] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
      import('@xterm/addon-web-links'),
      import('@xterm/addon-unicode11'),
    ]);

    const activeTheme = typeof document !== 'undefined' ? (document.documentElement.getAttribute('data-theme') || 'terminal-volt') : 'terminal-volt';
    const initialTerminalTheme = TERMINAL_THEMES[activeTheme] || TERMINAL_THEMES['terminal-volt']!;

    const term = new Terminal({
      allowProposedApi: true,
      fontSize: FONT_SIZE.lg,
      fontFamily: '"JetBrainsMono Nerd Font", "MesloLGS NF", "FiraCode Nerd Font", "Hack Nerd Font", Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: 'rgba(0, 0, 0, 0)',
        foreground: initialTerminalTheme.foreground,
        cursor: initialTerminalTheme.cursor,
        selectionBackground: initialTerminalTheme.selectionBackground || initialTerminalTheme.cursor + '33',
      },
      allowTransparency: true,
      cursorBlink: true,
      scrollback: 5000,
      minimumContrastRatio: 4.5,
      drawBoldTextInBrightColors: true,
    });

    const unicode11Addon = new Unicode11Addon();
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    const webLinksAddon = new WebLinksAddon((e: unknown, uri: string) => {
      if (uri.startsWith('http://') || uri.startsWith('https://')) {
        window.open(uri, '_blank');
      } else if (uri.startsWith('/')) {
        window.dispatchEvent(new CustomEvent('navigate-files', { detail: uri }));
      }
    });
    term.loadAddon(webLinksAddon);

    const placeholderId = `placeholder-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const sessionContainer = document.createElement('div');
    sessionContainer.setAttribute('data-terminal-session', placeholderId);
    sessionContainer.style.cssText = 'width:100%;height:100%;display:none;';
    container.appendChild(sessionContainer);

    term.open(sessionContainer);
    try { fitAddon.fit(); } catch { /* ignore */ }
    const realCols = term.cols;
    const realRows = term.rows;

    const api = window.nativesAPI;
    if (!api?.terminal?.create) {
      term.writeln('\x1b[31mTerminal API not available\x1b[0m');
      return;
    }

    let sessionId: string;
    try {
      const result = (await api.terminal.create(
        profileId ? String(profileId) : undefined,
        realCols,
        realRows,
      )) as unknown as { sessionId?: string; error?: string };
      if (result.error || !result.sessionId) {
        term.writeln(`\x1b[31mFailed to create terminal: ${result.error || 'unknown error'}\x1b[0m`);
        return;
      }
      sessionId = result.sessionId;
    } catch (err) {
      term.writeln(`\x1b[31mFailed to create terminal: ${err}\x1b[0m`);
      return;
    }

    sessionContainer.setAttribute('data-terminal-session', sessionId);

    const profileName = profiles.find((p) => p.id === profileId)?.name || '';
    sessionCounterRef.current += 1;
    const sessionLabel = label || `${profileName ? profileName + ' ' : ''}zsh ${sessionCounterRef.current}`;
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

    const allContainers = container.querySelectorAll('[data-terminal-session]');
    allContainers.forEach((el) => {
      (el as HTMLElement).style.display =
        el.getAttribute('data-terminal-session') === sessionId ? 'block' : 'none';
    });

    term.focus();

    const isAtBottom = () => {
      const buf = term.buffer.active;
      return buf.viewportY + term.rows >= buf.baseY + term.rows - 1;
    };

    const isComposingRef = { current: false };

    term.onData((data: string) => {
      if (isComposingRef.current && data.length <= 4) return;
      api.terminal.write(sessionId, data);
    });

    const termEl = container.querySelector('.xterm-helper-textarea') as HTMLElement | null;
    if (termEl) {
      termEl.addEventListener('compositionstart', () => { isComposingRef.current = true; });
      termEl.addEventListener('compositionend', () => { isComposingRef.current = false; });
    }

    const unsubscribers: Array<() => void> = [];

    const unsubData = api.terminal.onData((payload: { sessionId: string; data: string }) => {
      if (payload.sessionId !== sessionId) return;
      const output = payload.data;
      if (output) {
        const wasAtBottom = isAtBottom();
        term.write(output);
        if (wasAtBottom) term.scrollToBottom();
        recordTerminalActivity(sessionId);
        const lines = output.split('\n');
        for (let i = 0; i < lines.length; i++) {
          const trimmed = lines[i]!.trim();
          if (trimmed) recordScrollbackLine(trimmed);
        }
        if (output.length > 10) {
          if (/esc to interrupt/i.test(output)) {
            setIsAgentBusy(true);
          }
          if (/\? for.*options|Do you want|approve|Y\/n/i.test(output)) {
            if (!muted) playAskChime();
          }
          const action = parseAgentAction(output.split('\n'));
          if (action) {
            window.dispatchEvent(new CustomEvent('agent-action', { detail: action }));
          }
          if (/done|complete|finished|success/i.test(output) && !/undo|revert/i.test(output)) {
            if (!muted) playDoneChime();
          }
        }
      }
    });
    if (typeof unsubData === 'function') unsubscribers.push(unsubData);

    const unsubExit = api.terminal.onExit?.((payload: { sessionId: string; exitCode?: number }) => {
      if (payload.sessionId !== sessionId) return;
      exitedSessionIds.current.add(sessionId);
      const exitCode = payload.exitCode ?? 0;
      term.writeln(`\x1b[90m[Process exited with code ${exitCode}]\x1b[0m`);
    });
    if (typeof unsubExit === 'function') unsubscribers.push(unsubExit);

    const unsubTitle = api.terminal.onTitleChanged?.((payload: { sessionId: string; title: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (payload.title) {
        const s = sessionMapRef.current.get(sessionId);
        if (s) s.label = payload.title;
        setSessions((prev) => prev.map((s) => s.id === sessionId ? { ...s, label: payload.title } : s));
      }
    });
    if (typeof unsubTitle === 'function') unsubscribers.push(unsubTitle);

    const unsubPwd = api.terminal.onPwdChanged?.((payload: { sessionId: string; pwd: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (payload.pwd) {
        window.dispatchEvent(new CustomEvent('navigate-files', { detail: payload.pwd }));
      }
    });
    if (typeof unsubPwd === 'function') unsubscribers.push(unsubPwd);

    const unsubBell = api.terminal.onBell?.((payload: { sessionId: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (!muted) playAskChime();
    });
    if (typeof unsubBell === 'function') unsubscribers.push(unsubBell);

    session.unsubscribers = unsubscribers;

    term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
      api.terminal.resize(sessionId, cols, rows);
    });

    return sessionId;
  }, [onSessionCreated, profiles, muted]);

  // Switch active session
  const switchSession = useCallback((sessionId: string) => {
    const container = terminalRef.current;
    if (!container) return;

    const allContainers = container.querySelectorAll('[data-terminal-session]');
    allContainers.forEach((el) => {
      (el as HTMLElement).style.display =
        el.getAttribute('data-terminal-session') === sessionId ? 'block' : 'none';
    });

    setActiveSessionId(sessionId);

    const session = sessionMapRef.current.get(sessionId);
    if (session) {
      const fa = session.fitAddon as { fit: () => void };
      try { fa.fit(); } catch { /* ignore */ }
      const term = session.term as { focus?: () => void };
      try { term.focus?.(); } catch { /* ignore */ }
    }
  }, []);

  // Close session
  const closeSession = useCallback((sessionId: string) => {
    const session = sessionMapRef.current.get(sessionId);
    if (!session) return;

    if (session.unsubscribers) {
      for (const unsub of session.unsubscribers) {
        try { unsub(); } catch { /* ignore */ }
      }
    }

    window.nativesAPI?.terminal?.kill(sessionId);

    const term = session.term as { dispose?: () => void };
    if (term?.dispose) term.dispose();

    const container = terminalRef.current;
    if (container) {
      const el = container.querySelector(`[data-terminal-session="${sessionId}"]`);
      el?.remove();
    }

    sessionMapRef.current.delete(sessionId);
    exitedSessionIds.current.delete(sessionId);

    setSessions((prev) => {
      const remaining = prev.filter((s) => s.id !== sessionId);
      if (remaining.length === 0) {
        return [];
      }
      const nextSession = remaining[remaining.length - 1]!;
      setActiveSessionId(nextSession.id);
      if (container) {
        const nextEl = container.querySelector(`[data-terminal-session="${nextSession.id}"]`);
        if (nextEl) (nextEl as HTMLElement).style.display = 'block';
      }
      return remaining;
    });
  }, []);

  // CWD detection
  const refreshCwd = useCallback(async (sessionId: string) => {
    try {
      const result = await window.nativesAPI?.terminal?.cwd?.(sessionId) as
        { cwd?: string; source?: string } | undefined;
      if (result?.cwd) {
        followChange(result.cwd, '', result.cwd);
      }
    } catch { /* ignore */ }
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      sessionMapRef.current.forEach((session) => {
        if (session.unsubscribers) {
          for (const unsub of session.unsubscribers) {
            try { unsub(); } catch { /* ignore */ }
          }
        }
        window.nativesAPI?.terminal?.kill(session.id);
        const term = session.term as { dispose?: () => void };
        if (term?.dispose) term.dispose();
      });
      sessionMapRef.current.clear();
    };
  }, []);

  // Listen for theme changes
  useEffect(() => {
    return onThemeChange((themeId: string) => {
      const terminalTheme = TERMINAL_THEMES[themeId];
      if (!terminalTheme) return;

      sessionMapRef.current.forEach((session) => {
        const term = session.term as { setOption?: (key: string, value: unknown) => void };
        if (term?.setOption) {
          term.setOption('theme', {
            background: 'rgba(0, 0, 0, 0)',
            foreground: terminalTheme.foreground,
            cursor: terminalTheme.cursor,
            selectionBackground: terminalTheme.selectionBackground || terminalTheme.cursor + '33',
          });
        }
      });
    });
  }, []);

  // Agent breathing glow
  useEffect(() => {
    let glowTimeout: ReturnType<typeof setTimeout> | null = null;
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.status === 'idle' && terminalRef.current?.parentElement) {
        terminalRef.current.parentElement.classList.add('anim-termAwait');
        setIsAgentBusy(false);
        if (glowTimeout) clearTimeout(glowTimeout);
        glowTimeout = setTimeout(() => {
          terminalRef.current?.parentElement?.classList.remove('anim-termAwait');
        }, 3000);
      } else if (detail?.status === 'busy' || detail?.status === 'working') {
        setIsAgentBusy(true);
      }
    };
    window.addEventListener('agent-status-changed', handler);
    return () => {
      window.removeEventListener('agent-status-changed', handler);
      if (glowTimeout) clearTimeout(glowTimeout);
    };
  }, []);

  return {
    sessions,
    setSessions,
    activeSessionId,
    setActiveSessionId,
    sessionMapRef,
    terminalRef,
    exitedSessionIds,
    createSession,
    closeSession,
    switchSession,
    refreshCwd,
    isAgentBusy,
    setIsAgentBusy,
  };
}
