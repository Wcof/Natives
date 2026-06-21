'use client';

import '@xterm/xterm/css/xterm.css';

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, useLocale, type Locale } from '@/i18n';
import { Terminal as TerminalIcon, Clipboard, X, Plus, Link2, VolumeX, Volume2, Crosshair, Maximize2, Minimize2, ChevronDown, ChevronUp, Check, Sparkles, Code2 } from 'lucide-react';
import { onThemeChange, TERMINAL_THEMES } from '@/lib/theme-engine';
import { recordTerminalActivity, setFileFollow, followChange } from '@/lib/follow-mode';
import { recordScrollbackLine } from '@/lib/path-detector';
import { playDoneChime, playAskChime } from '@/lib/chime';
import { parseAgentAction } from '@/lib/agent-narration';
import { copyToClipboard } from '@/lib/clipboard';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';

interface TerminalSession {
  id: string;
  label: string;
  term: unknown;
  fitAddon: unknown;
  active: boolean;
  profileId?: string;
  /** IPC listener 清理函数（防止内存泄漏） */
  unsubscribers?: Array<() => void>;
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
  const sessionCounterRef = useRef(0);

  // Tab label fallback：尝试从 foreground_process / cwd 推断 label
  const getFallbackLabel = useCallback((sessionId: string): string => {
    const session = sessionMapRef.current.get(sessionId);
    if (!session) return 'zsh';
    // 检查 foreground_process（来自 session.term 的额外数据不可用，通过 sessionId 查 proc）
    // 但 foreground_process 是异步的，这里用同步的 label 后备
    // 用 session 已有的初始 label（shell 名）作为保底
    return session.label || 'zsh';
  }, []);

  // Profile state (US26)
  const [profiles, setProfiles] = useState<Array<{ id: number; name: string; is_default: number }>>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<number | null>(null);
  const [isAgentBusy, setIsAgentBusy] = useState(false);
  const [profileMenuOpen, setProfileMenuOpen] = useState(false);
  const profileRef = useRef<HTMLDivElement>(null);

  // Click outside listener for custom profile dropdown
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (profileRef.current && !profileRef.current.contains(event.target as Node)) {
        setProfileMenuOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, []);

  // Mute state（持久化到 localStorage）
  const [muted, setMuted] = useState(() => {
    if (typeof window === 'undefined') return false;
    return localStorage.getItem('natives_term_muted') === '1';
  });
  // 持久化 mute 变更
  useEffect(() => {
    localStorage.setItem('natives_term_muted', muted ? '1' : '0');
  }, [muted]);
  // Process exit message tracking
  const exitedSessionIds = useRef<Set<string>>(new Set());

  // Load environment profiles (US26)
  useEffect(() => {
    async function loadProfiles() {
      try {
        const api = window.nativesAPI;
        if (!api?.env) return;
        const list = await api.env.listProfiles();
        const profileList = list as unknown as Array<{ id: number; name: string; is_default: number }>;
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
      // Nerd Font 优先（Starship、powerline 图标正确显示，避免 tofu 方块）
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
      // WCAG AA: auto-adjust dim agent output to readable contrast (Natives2: 4.5)
      minimumContrastRatio: 4.5,
      drawBoldTextInBrightColors: true,
    });

    // Unicode11Addon: correct CJK wide character width (Natives2)
    const unicode11Addon = new Unicode11Addon();
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // WebGL addon is bypassed to support transparent/frosted-glass backgrounds
    // (xterm.js WebGL renderer does not support alpha channel transparency)

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

    // Create container for this session FIRST (before PTY creation)
    const placeholderId = `placeholder-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const sessionContainer = document.createElement('div');
    sessionContainer.setAttribute('data-terminal-session', placeholderId);
    sessionContainer.style.cssText = 'width:100%;height:100%;display:none;';
    container.appendChild(sessionContainer);

    // Mount xterm and fit to get real cols/rows BEFORE creating PTY
    term.open(sessionContainer);
    try { fitAddon.fit(); } catch { /* ignore */ }
    const realCols = term.cols;
    const realRows = term.rows;

    // Create PTY session via IPC with real terminal dimensions
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

    // Fix up the data-terminal-session attribute with the real sessionId
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

    // Use a placeholder ID initially, then replace with real sessionId
    sessionMapRef.current.set(sessionId, session);
    setSessions((prev) => [...prev, session]);
    setActiveSessionId(sessionId);
    onSessionCreated?.(sessionId);

    // Show this session's container, hide others
    const allContainers = container.querySelectorAll('[data-terminal-session]');
    allContainers.forEach((el) => {
      (el as HTMLElement).style.display =
        el.getAttribute('data-terminal-session') === sessionId ? 'block' : 'none';
    });

    // Focus the new terminal so user can type immediately
    term.focus();

    // Scroll desync self-healing (xterm 5.5.0 viewport bug):
    // If already at bottom, force scroll after write to stay at bottom
    const isAtBottom = () => {
      const buf = term.buffer.active;
      return buf.viewportY + term.rows >= buf.baseY + term.rows - 1;
    };

    // IME composition flag — 防止 compositionupdate 双写（fanbox 风格）
    const isComposingRef = { current: false };

    // Send user input to PTY
    term.onData((data: string) => {
      // IME 合成期间跳过修饰键产生的重复写入
      if (isComposingRef.current && data.length <= 4) return;
      api.terminal.write(sessionId, data);
    });

    // Listen for composition events on xterm's textarea
    const termEl = container.querySelector('.xterm-helper-textarea') as HTMLElement | null;
    if (termEl) {
      termEl.addEventListener('compositionstart', () => { isComposingRef.current = true; });
      termEl.addEventListener('compositionend', () => { isComposingRef.current = false; });
    }

    // Receive PTY output via dedicated channels（对标 CodePilot — 绕过 db-state-changed 事件风暴）
    const unsubscribers: Array<() => void> = [];

    const unsubData = api.terminal.onData((payload: { sessionId: string; data: string }) => {
      if (payload.sessionId !== sessionId) return;
      const output = payload.data;
      if (output) {
        const wasAtBottom = isAtBottom();
        term.write(output);
        // Scroll desync self-healing: if was at bottom, stay at bottom
        if (wasAtBottom) term.scrollToBottom();
        // Track activity for follow mode
        recordTerminalActivity(sessionId);
        // Record scrollback for path detection (split once, trim once)
        const lines = output.split('\n');
        for (let i = 0; i < lines.length; i++) {
          const trimmed = lines[i]!.trim();
          if (trimmed) recordScrollbackLine(trimmed);
        }
        // Check for completion/approval patterns (only when output is substantial)
        if (output.length > 10) {
          if (/esc to interrupt/i.test(output)) {
            setIsAgentBusy(true);
          }
          if (/\? for.*options|Do you want|approve|Y\/n/i.test(output)) {
            if (!muted) playAskChime();
          }
          // 假静默护栏：检测 "still running" 则延迟完成通知
          if (/\d+ shell still running|background\s*(job|process)/i.test(output)) {
            // Agent 输出表明有后台任务在跑，不做完成通知
            return;
          }
        }
      }
    });
    if (typeof unsubData === 'function') unsubscribers.push(unsubData);

    const unsubExit = api.terminal.onExit((payload: { sessionId: string; exitCode: number }) => {
      if (payload.sessionId !== sessionId) return;
      setIsAgentBusy(false);
      if (!muted) playDoneChime();
      // 写入退出提示（fanbox 风格）
      const codeStr = payload.exitCode !== 0 ? ` (code ${payload.exitCode})` : '';
      term.write(`\r\n\x1b[2m[进程已退出${codeStr} — 回车重开，或关闭此 tab]\x1b[0m\r\n`);
      // 回车重开
      const reopenHandler = (data: string) => {
        if (data === '\r') {
          term.dispose?.();
    // eslint-disable-next-line react-hooks/immutability
          createSession(undefined, selectedProfileId ?? undefined);
          // 移除监听
        }
      };
      const disposable = term.onData(reopenHandler);
      // 3 秒后自动移除回车监听，防止堆积
      setTimeout(() => { try { disposable.dispose(); } catch { /* */ } }, 3000);
    });
    if (typeof unsubExit === 'function') unsubscribers.push(unsubExit);

    // ── Ghostty VT 事件订阅（feature gate — adapter 端安全 fallback） ──

    // Title changed → update tab label（优先级：OSC title → foreground process → cwd → shell）
    const unsubTitle = api.terminal.onTitleChanged?.((payload: { sessionId: string; title: string }) => {
      if (payload.sessionId !== sessionId) return;
      const newLabel = payload.title || getFallbackLabel(sessionId);
      setSessions((prev) =>
        prev.map((s) => (s.id === sessionId ? { ...s, label: newLabel } : s))
      );
    });
    if (typeof unsubTitle === 'function') unsubscribers.push(unsubTitle);

    // PWD changed → update cwd + notify file browser
    const unsubPwd = api.terminal.onPwdChanged?.((payload: { sessionId: string; pwd: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (payload.pwd) {
        window.dispatchEvent(new CustomEvent('navigate-files', { detail: payload.pwd }));
      }
    });
    if (typeof unsubPwd === 'function') unsubscribers.push(unsubPwd);

    // Bell → play chime
    const unsubBell = api.terminal.onBell?.((payload: { sessionId: string }) => {
      if (payload.sessionId !== sessionId) return;
      if (!muted) playAskChime();
    });
    if (typeof unsubBell === 'function') unsubscribers.push(unsubBell);

    session.unsubscribers = unsubscribers;

    // Resize handling
    term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
      api.terminal.resize(sessionId, cols, rows);
    });

    return sessionId;
  }, [onSessionCreated, profiles]);

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
      // Focus the terminal so user can type immediately
      const term = session.term as { focus?: () => void };
      try { term.focus?.(); } catch { /* ignore */ }
    }
  }, []);

  // Close session
  const closeSession = useCallback((sessionId: string) => {
    const session = sessionMapRef.current.get(sessionId);
    if (!session) return;

    // 清理 IPC 监听器（防止内存泄漏和写入已销毁的 terminal）
    if (session.unsubscribers) {
      for (const unsub of session.unsubscribers) {
        try { unsub(); } catch { /* ignore */ }
      }
    }

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

  // Fit on resize + ResizeObserver for cold start / maximize / tab switch
  useEffect(() => {
    if (isCollapsed || !activeSessionId) return;

    const doFit = () => {
      const session = sessionMapRef.current.get(activeSessionId);
      if (session) {
        const fa = session.fitAddon as { fit: () => void };
        try { fa.fit(); } catch { /* ignore */ }
      }
    };
    doFit();

    // ResizeObserver: 确保 xterm cols/rows 与容器始终保持一致
    // 覆盖冷启动、展开、最大化、切 tab 后容器尺寸变化
    const containerEl = terminalRef.current;
    if (!containerEl) return;
    let rafId: number | null = null;
    const ro = new ResizeObserver(() => {
      // Throttle with requestAnimationFrame
      if (rafId !== null) return;
      rafId = requestAnimationFrame(() => {
        rafId = null;
        doFit();
      });
    });
    ro.observe(containerEl);

    return () => {
      ro.disconnect();
      if (rafId !== null) cancelAnimationFrame(rafId);
    };
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
      // Cmd+K: 清屏（发送 clear 或 form feed）
      if (e.key === 'k' && !e.shiftKey && !e.ctrlKey && activeSessionId) {
        const termEl = terminalRef.current;
        if (termEl && (termEl.contains(document.activeElement) || !isCollapsed)) {
          e.preventDefault();
          // 发送 Ctrl+L (form feed) 清屏
          window.nativesAPI?.terminal?.write?.(activeSessionId, '\x0c');
        }
      }
      // Cmd+V: paste as bracketed paste
      if (e.key === 'v' && e.shiftKey && activeSessionId) {
        // Cmd+Shift+V — bracketed paste
        e.preventDefault();
        navigator.clipboard.readText().then((text) => {
          if (text && activeSessionId) {
            // 用 bracketed paste 模式包裹粘贴内容，防止多行触发意外执行
            window.nativesAPI?.terminal?.write?.(activeSessionId, `\x1b[200~${text}\x1b[201~`);
          }
        }).catch(() => {});
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [sessions, activeSessionId, isCollapsed, createSession, closeSession, switchSession]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      sessionMapRef.current.forEach((session) => {
        // 清理 IPC 监听器
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

  // Listen for theme changes and update xterm ANSI colors dynamically
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

  // Agent launch: 若当前前台进程是 shell 则复用当前 tab，否则新建 tab
  const launchAgent = useCallback(async (cmd: string) => {
    if (activeSessionId) {
      // 检查当前 session 的前台进程是否是 shell
      try {
        const procResult = await window.nativesAPI?.terminal?.proc?.(activeSessionId) as
          { processName?: string; pid?: number } | undefined;
        const procName = procResult?.processName?.toLowerCase() || '';
        // shell 进程名: zsh, bash, fish, sh, powershell, cmd
        const isShell = ['zsh', 'bash', 'fish', 'sh', 'powershell', 'cmd', 'shell'].some(
          (s) => procName.includes(s)
        );
        if (!isShell) {
          // 前台跑着 TUI (vim/top/claude/codex)，新建 tab
          const newId = await createSession(undefined, selectedProfileId ?? undefined);
          if (newId) {
            window.nativesAPI?.terminal?.write?.(newId, cmd + '\r');
          }
          return;
        }
      } catch {
        // proc 不可用时 fallback 到当前 session
      }
      // 是 shell，发送命令到当前 session
      window.nativesAPI?.terminal?.write?.(activeSessionId, cmd + '\r');
      return;
    }
    // No session, create one then send command directly
    const newId = await createSession(undefined, selectedProfileId ?? undefined);
    if (newId) {
      window.nativesAPI?.terminal?.write?.(newId, cmd + '\r');
    }
  }, [activeSessionId, createSession, selectedProfileId]);

  // CWD detection — 使用 OSC 7 → lsof → HOME 三级 fallback
  const refreshCwd = useCallback(async (sessionId: string) => {
    try {
      const result = await window.nativesAPI?.terminal?.cwd?.(sessionId) as
        { cwd?: string; source?: string } | undefined;
      if (result?.cwd) {
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
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 pl-1">
            <TerminalIcon size={12} className="text-[var(--vibe-active-color)]" />
            <span style={{
              fontFamily: 'var(--font-display)',
              fontSize: '11px',
              fontWeight: 700,
              color: 'var(--vibe-brand-text)',
              letterSpacing: '0.05em',
              textTransform: 'uppercase'
            }}>
              {t(locale, 'terminal.title')}
            </span>
          </div>
          <div style={{ width: '1px', height: '12px', background: 'var(--vibe-btn-border, var(--border))', margin: '0 4px' }} />
          
          <div className="terminal-tabs">
            {sessions.map((session) => (
              <div
                key={session.id}
                className={`terminal-tab ${session.id === activeSessionId ? 'active' : ''}${isAgentBusy && session.id === activeSessionId ? ' anim-tabpulse' : ''}`}
                onClick={() => switchSession(session.id)}
              >
                <span>{session.label}</span>
                {sessions.length > 1 && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      closeSession(session.id);
                    }}
                    className="flex items-center justify-center p-0.5 rounded-full hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)] transition-colors opacity-60 hover:opacity-100"
                    style={{ background: 'none', border: 'none', color: 'inherit', cursor: 'pointer' }}
                    title={t(locale, 'terminal.closeTab')}
                    aria-label={t(locale, 'terminal.closeTab')}
                  >
                    <X size={10} />
                  </button>
                )}
              </div>
            ))}
            <button
              className="terminal-tab"
              onClick={() => createSession(undefined, selectedProfileId ?? undefined)}
              title={t(locale, 'terminal.newTab')}
              aria-label={t(locale, 'terminal.newTab')}
            >
              <Plus size={12} />
            </button>
          </div>
        </div>

        {/* Environment profile selector (US26) */}
        {profiles.length > 0 && (
          <div ref={profileRef} className="relative flex items-center">
            <button
              className="terminal-action-btn"
              onClick={() => setProfileMenuOpen((v) => !v)}
              title={t(locale, 'terminal.selectProfile')}
              aria-label={t(locale, 'terminal.selectProfile')}
            >
              <span>
                {profiles.find((p) => p.id === selectedProfileId)?.name || t(locale, 'terminal.selectProfile')}
              </span>
              <ChevronDown
                size={10}
                className={`transition-transform duration-200 ${profileMenuOpen ? 'rotate-180' : ''}`}
              />
            </button>
            {profileMenuOpen && (
              <div className="terminal-dropdown-menu">
                {profiles.map((p) => {
                  const isActive = p.id === selectedProfileId;
                  return (
                    <button
                      key={p.id}
                      className={`terminal-dropdown-item ${isActive ? 'active' : ''}`}
                      onClick={() => {
                        setSelectedProfileId(p.id);
                        setProfileMenuOpen(false);
                      }}
                    >
                      <span>{p.name}</span>
                      {isActive && <Check size={12} />}
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        )}

        <div className="terminal-actions">
          {/* Agent launch buttons */}
          <button
            className="terminal-action-btn"
            onClick={() => launchAgent('claude --dangerously-skip-permissions')}
            title={t(locale, 'terminal.launchClaude')}
            aria-label={t(locale, 'terminal.launchClaude')}
          >
            <Sparkles size={12} className="text-[var(--vibe-active-color)]" />
            <span>Claude</span>
          </button>
          <button
            className="terminal-action-btn"
            onClick={() => launchAgent('codex')}
            title={t(locale, 'terminal.launchCodex')}
            aria-label={t(locale, 'terminal.launchCodex')}
          >
            <Code2 size={12} className="text-[var(--vibe-active-color)]" />
            <span>Codex</span>
          </button>
          {/* Mute toggle — fanbox 风格 */}
          <button
            className="terminal-action-btn"
            onClick={() => setMuted((prev) => !prev)}
            title={muted ? t(locale, 'terminal.unmute') : t(locale, 'terminal.mute')}
            aria-label={muted ? t(locale, 'terminal.unmute') : t(locale, 'terminal.mute')}
          >
            {muted ? <VolumeX size={12} /> : <Volume2 size={12} />}
          </button>
          {/* Locate CWD — 把文件区跳到终端当前所在目录（fanbox 风格） */}
          <button
            className="terminal-action-btn"
            onClick={async () => {
              if (!activeSessionId) return;
              try {
                const result = await window.nativesAPI?.terminal?.cwd?.(activeSessionId) as
                  { cwd?: string; source?: string } | undefined;
                if (result?.cwd) {
                  window.dispatchEvent(new CustomEvent('navigate-files', { detail: result.cwd }));
                }
              } catch { /* ignore */ }
            }}
            title={t(locale, 'terminal.locateCwd')}
            aria-label={t(locale, 'terminal.locateCwd')}
          >
            <Crosshair size={12} />
          </button>
          {/* Follow mode toggle */}
          {onFollowModeToggle && (
            <button
              className={`terminal-action-btn ${followMode ? 'active' : ''}`}
              onClick={onFollowModeToggle}
              title={followMode ? t(locale, 'terminal.followModeOn') : t(locale, 'terminal.followModeOff')}
              aria-label={t(locale, 'terminal.ariaToggleFollowMode')}
            >
              <Link2 size={12} />
            </button>
          )}
          {/* Copy selection to clipboard（不调用 selectAll，保留用户选择） */}
          <button
            className="terminal-action-btn"
            onClick={async () => {
              const session = sessions.find(s => s.id === activeSessionId);
              if (!session) return;
              const term = session.term as { getSelection?: () => string; selectAll?: () => void; clearSelection?: () => void } | undefined;
              if (!term) return;
              try {
                let text = term.getSelection?.() || '';
                // 如果没有选择，才选择全部
                if (!text) {
                  term.selectAll?.();
                  text = term.getSelection?.() || '';
                  term.clearSelection?.();
                }
                if (text) {
                  const ok = await copyToClipboard(text);
                  if (ok && !muted) playDoneChime();
                }
              } catch { /* ignore */ }
            }}
            title={t(locale, 'terminal.copySelection')}
            aria-label={t(locale, 'terminal.copySelection')}
          >
            <Clipboard size={12} />
          </button>
          <button
            className="terminal-action-btn"
            onClick={onMaximizeToggle}
            title={isMaximized ? t(locale, 'terminal.restore') : t(locale, 'terminal.maximize')}
            aria-label={isMaximized ? t(locale, 'terminal.ariaRestore') : t(locale, 'terminal.ariaMaximize')}
          >
            {isMaximized ? <Minimize2 size={12} /> : <Maximize2 size={12} />}
          </button>
          <button
            className="terminal-action-btn"
            onClick={onToggle}
            title={isCollapsed ? t(locale, 'terminal.expand') : t(locale, 'terminal.collapse')}
            aria-label={isCollapsed ? t(locale, 'terminal.ariaOpen') : t(locale, 'terminal.ariaClose')}
          >
            {isCollapsed ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
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
            background: 'var(--accent-soft)',
            border: '2px dashed var(--accent)',
            borderRadius: BORDER_RADIUS.sm,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'var(--accent)', fontSize: FONT_SIZE.lg, fontWeight: 600,
            zIndex: 10, pointerEvents: 'none',
          }}>
            {t(locale, 'terminal.dropPrompt')}
          </div>
        )}
      </div>
    </div>
  );
}
