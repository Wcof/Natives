'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { onThemeChange, TERMINAL_THEMES } from '@/lib/theme-engine';
import { recordTerminalActivity, followSetScope } from '@/lib/follow-mode';
import { recordScrollbackLine, detectFilePaths, verifyCandidates, locateCandidate, getSessionPwd, recordPwdChange } from '@/lib/path-detector';
import { FILE_EVENTS, dispatchFileEvent, navigateToFiles } from '@/lib/file-events';
import { playDoneChime, playAskChime } from '@/lib/chime';

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
  // muted 经 ref 供各会话监听器读取：监听器在会话创建时捕获闭包，
  // 直接引用 muted 会导致既有标签的提示音开关永远停在创建时的值
  const mutedRef = useRef(muted);
  useEffect(() => { mutedRef.current = muted; }, [muted]);
  // agent 忙碌信号只有「出现」没有「结束」事件（agent-status-changed 全项目
  // 无发射方，旧实现 isAgentBusy 一旦置 true 即永久脉动）——改为静默超时归位
  const agentBusyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const markAgentBusy = useCallback(() => {
    setIsAgentBusy(true);
    if (agentBusyTimerRef.current) clearTimeout(agentBusyTimerRef.current);
    agentBusyTimerRef.current = setTimeout(() => {
      setIsAgentBusy(false);
      const parent = terminalRef.current?.parentElement;
      if (parent) {
        parent.classList.add('anim-termAwait');
        setTimeout(() => parent.classList.remove('anim-termAwait'), 3000);
      }
    }, 8000);
  }, []);
  useEffect(() => () => {
    if (agentBusyTimerRef.current) clearTimeout(agentBusyTimerRef.current);
  }, []);

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

    const activeTheme = typeof document !== 'undefined' ? (document.documentElement.getAttribute('data-theme') || 'dark') : 'dark';
    const initialTerminalTheme = TERMINAL_THEMES[activeTheme] || TERMINAL_THEMES.dark!;

    const term = new Terminal({
      allowProposedApi: true,
      fontSize: 14,
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
        dispatchFileEvent(FILE_EVENTS.navigateFiles, uri);
      }
    });
    term.loadAddon(webLinksAddon);

    const pendingSessionId = `pending-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const sessionContainer = document.createElement('div');
    sessionContainer.setAttribute('data-terminal-session', pendingSessionId);
    sessionContainer.style.cssText = 'width:100%;height:100%;display:none;';
    container.appendChild(sessionContainer);

    term.open(sessionContainer);
    try { fitAddon.fit(); } catch { /* ignore */ }
    const realCols = term.cols;
    const realRows = term.rows;

    // 创建失败时的清理：旧实现把红字写进 display:none 的容器（用户什么都
    // 看不到），且 xterm 实例与 DOM 节点双泄漏
    const failCreate = (message: string) => {
      try { term.dispose(); } catch { /* ignore */ }
      sessionContainer.remove();
      window.dispatchEvent(new CustomEvent('terminal-create-failed', { detail: message }));
    };

    const api = window.nativesAPI;
    if (!api?.terminal?.create) {
      failCreate('Terminal API not available');
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
        failCreate(result.error || 'unknown error');
        return;
      }
      sessionId = result.sessionId;
    } catch (err) {
      failCreate(err instanceof Error ? err.message : String(err));
      return;
    }

    sessionContainer.setAttribute('data-terminal-session', sessionId);

    // ── 终端路径链接（W11，fanbox 移植）──
    // 逐行探测路径候选 → 划线前批量验真（截断路径「…」除外，点击时再定位）
    // → 点击走 locateCandidate（后端四级兜底 + 前端 scrollback 回扫）。
    // 随 term.dispose() 一起销毁，无需单独清理。
    try {
      (term as unknown as {
        registerLinkProvider: (p: {
          provideLinks: (line: number, cb: (links: unknown[] | undefined) => void) => void;
        }) => void;
      }).registerLinkProvider({
        provideLinks: (lineNumber: number, callback: (links: unknown[] | undefined) => void) => {
          const buffer = (term as unknown as {
            buffer: { active: { getLine: (i: number) => { translateToString: (trim: boolean) => string } | undefined } };
          }).buffer;
          const lineText = buffer.active.getLine(lineNumber - 1)?.translateToString(true) ?? '';
          if (!lineText.trim()) { callback(undefined); return; }
          const cwd = getSessionPwd(sessionId) || '/';
          const candidates = detectFilePaths(lineText, cwd);
          if (candidates.length === 0) { callback(undefined); return; }
          void (async () => {
            let usable = candidates;
            const toVerify = candidates.filter((c) => c.verified !== 'truncated').map((c) => c.path);
            if (toVerify.length > 0) {
              const existing = await verifyCandidates(toVerify);
              if (existing) {
                usable = candidates.filter((c) => c.verified === 'truncated' || existing.has(c.path));
              }
            }
            if (usable.length === 0) { callback(undefined); return; }
            callback(usable.map((c) => ({
              range: {
                start: { x: c.start + 1, y: lineNumber },
                end: { x: c.end, y: lineNumber },
              },
              text: c.path,
              activate: () => {
                void locateCandidate(c.path, getSessionPwd(sessionId) || cwd).then((resolved) => {
                  navigateToFiles(resolved ?? c.path);
                });
              },
            })));
          })();
        },
      });
    } catch { /* 老版本 xterm 无 registerLinkProvider 时静默跳过 */ }

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
      // 进程已退出后 PTY 会话在后端已删除：继续写只会刷 unhandled rejection
      if (exitedSessionIds.current.has(sessionId)) return;
      void Promise.resolve(api.terminal.write(sessionId, data)).catch(() => {
        // 写失败（如后端刚回收）静默丢弃；退出提示已由 onExit 打印
      });
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
            markAgentBusy();
          }
          if (/\? for.*options|Do you want|approve|Y\/n/i.test(output)) {
            if (!mutedRef.current) playAskChime();
          }
          if (/done|complete|finished|success/i.test(output) && !/undo|revert/i.test(output)) {
            if (!mutedRef.current) playDoneChime();
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
        // 终端路径链接的相对路径解析基准 + 跟随作用域（此前 recordPwdChange
        // 无任何调用方，getSessionPwd 恒空，链接解析基准永远落在 '/'）
        recordPwdChange(sessionId, payload.pwd);
        followSetScope(payload.pwd, sessionId);
        dispatchFileEvent(FILE_EVENTS.navigateFiles, payload.pwd);
      }
    });
    if (typeof unsubPwd === 'function') unsubscribers.push(unsubPwd);

    const unsubBell = api.terminal.onBell?.((payload: { sessionId: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (!mutedRef.current) playAskChime();
    });
    if (typeof unsubBell === 'function') unsubscribers.push(unsubBell);

    session.unsubscribers = unsubscribers;

    term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
      api.terminal.resize(sessionId, cols, rows);
    });

    return sessionId;
  }, [onSessionCreated, profiles, markAgentBusy]);

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
        // 跟随作用域随绑定终端的 cwd 移动（followSetScope 内做归属过滤）。
        // 旧写法 followChange(cwd, '', cwd) 把 cwd 当"变更文件"喂状态机，属误用。
        followSetScope(result.cwd, sessionId);
        // 终端链接的相对路径解析基准（此前 recordPwdChange 无调用方，
        // getSessionPwd 恒空——链接接线的又一处断点）
        recordPwdChange(sessionId, result.cwd);
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

  // 注：原「agent-status-changed」监听器已删除——该事件全项目零发射方，
  // 忙碌→空闲的归位与呼吸光效改由 markAgentBusy 的静默超时驱动。

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
