'use client';

import '@xterm/xterm/css/xterm.css';

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, useLocale, type Locale } from '@/i18n';
import { Terminal as TerminalIcon, Clipboard, X, Plus, Link2, VolumeX, Volume2, Crosshair, Maximize2, Minimize2, ChevronDown, ChevronUp, Check, Sparkles, Code2 } from 'lucide-react';
import { setFileFollow } from '@/lib/follow-mode';
import { playDoneChime } from '@/lib/chime';
import { copyToClipboard } from '@/lib/clipboard';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import { useTerminalSessions } from './useTerminalSessions';

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
  const locale = useLocale();

  // Profile state (US26)
  const [profiles, setProfiles] = useState<Array<{ id: number; name: string; is_default: number }>>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<number | null>(null);
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

  // Terminal session management (extracted hook)
  const {
    sessions,
    activeSessionId,
    terminalRef,
    createSession,
    closeSession,
    switchSession,
    refreshCwd,
    isAgentBusy,
  } = useTerminalSessions({ onSessionCreated, profiles, muted });

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

  // Session lifecycle managed by useTerminalSessions hook
  // Fit on resize + ResizeObserver for cold start / maximize / tab switch
  useEffect(() => {
    if (isCollapsed || !activeSessionId) return;

    const doFit = () => {
      const session = sessions.find(s => s.id === activeSessionId);
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
    const session = sessions.find(s => s.id === activeSessionId);
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
