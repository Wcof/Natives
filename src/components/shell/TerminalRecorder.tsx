'use client';

/**
 * TerminalRecorder — asciinema v2 .cast 录像回放组件（Fanbox 风格）
 * 使用 xterm 只读实例渲染，保留 ANSI 色彩和光标控制序列
 */

import '@xterm/xterm/css/xterm.css';

import { startTransition, useCallback, useEffect, useRef, useState } from 'react';
import { Play, Square, List, X, Download, Trash2, RefreshCw } from 'lucide-react';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import { TERMINAL_THEMES } from '@/lib/theme-engine';

interface RecordingMeta {
  id: string;
  session_id: string;
  created_at: number;
  duration_secs: number;
  width: number;
  height: number;
  size_bytes: number;
}

interface Props {
  isCollapsed: boolean;
  onClose: () => void;
}

export default function TerminalRecorder({ isCollapsed, onClose }: Props) {
  const [recordings, setRecordings] = useState<RecordingMeta[]>([]);
  const [playing, setPlaying] = useState<string | null>(null);
  const [castData, setCastData] = useState<string[]>([]);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [isLoading, setIsLoading] = useState(false);
  const playRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const terminalContainerRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<{ write: (data: string) => void; dispose: () => void } | null>(null);
  const startTimeRef = useRef(0);

  const loadRecordings = useCallback(async () => {
    const api = window.nativesAPI;
    if (!api?.terminal?.recordList) return;
    try {
      const list = await api.terminal.recordList();
      // 解析 .cast 文件头部获取真实 width/height/duration
      const enriched = await Promise.all((list as RecordingMeta[]).map(async (rec) => {
        try {
          const raw = await api.terminal.recordPlay(rec.id);
          const lines = (raw as string).split('\n').filter(Boolean);
          if (lines.length > 0) {
            try {
              const header = JSON.parse(lines[0]!);
              if (header.width) rec.width = header.width;
              if (header.height) rec.height = header.height;
              // duration = 最后一个事件的时间戳
              if (lines.length > 1) {
                const lastEvent = JSON.parse(lines[lines.length - 1]!);
                rec.duration_secs = typeof lastEvent[0] === 'number' ? lastEvent[0] : 0;
              }
            } catch { /* skip */ }
          }
        } catch { /* skip */ }
        return rec;
      }));
      setRecordings(enriched);
    } catch { /* silent */ }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadRecordings();
  }, [loadRecordings]);

  // Initialize xterm for playback
  const initXterm = useCallback(async (width: number, height: number) => {
    const container = terminalContainerRef.current;
    if (!container) return null;

    // Clear container
    container.innerHTML = '';

    const [{ Terminal }] = await Promise.all([
      import('@xterm/xterm'),
    ]);

    const activeTheme = typeof document !== 'undefined'
      ? (document.documentElement.getAttribute('data-theme') || 'terminal-volt')
      : 'terminal-volt';
    const initialTerminalTheme = TERMINAL_THEMES[activeTheme] || TERMINAL_THEMES['terminal-volt']!;

    const term = new Terminal({
      cols: Math.max(width, 40),
      rows: Math.min(Math.max(height, 10), 40),
      fontSize: FONT_SIZE.sm,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: 'rgba(0, 0, 0, 0)',
        foreground: initialTerminalTheme.foreground,
        cursor: initialTerminalTheme.cursor,
        selectionBackground: initialTerminalTheme.selectionBackground || initialTerminalTheme.cursor + '33',
      },
      allowTransparency: true,
      cursorBlink: false,
      scrollback: 2000,
      disableStdin: true,     // 只读模式
      windowsMode: false,
    });

    term.open(container);

    const xtermInstance = {
      write: (data: string) => {
        try { term.write(data); } catch { /* ignore */ }
      },
      dispose: () => {
        try { term.dispose(); } catch { /* ignore */ }
      },
    };

    xtermRef.current = xtermInstance;
    return xtermInstance;
  }, []);

  // Play a recording
  const playRecording = useCallback(async (id: string) => {
    const api = window.nativesAPI;
    if (!api?.terminal?.recordPlay) return;
    setIsLoading(true);
    try {
      const raw = await api.terminal.recordPlay(id);
      const lines = (raw as string).split('\n').filter(Boolean);
      setCastData(lines);

      // Parse header for dimensions
      let width = 80, height = 24;
      if (lines.length > 0) {
        try {
          const header = JSON.parse(lines[0]!);
          if (header.width) width = header.width;
          if (header.height) height = header.height;
        } catch { /* skip */ }
      }

      // Init xterm
      await initXterm(width, height);

      setPlaying(id);
      setCurrentIdx(1); // Skip header (index 0)
      startTimeRef.current = Date.now();
    } catch { /* silent */ }
    setIsLoading(false);
  }, [initXterm]);

  // Step through events
  useEffect(() => {
    if (!playing || castData.length === 0) return;

    if (currentIdx >= castData.length) {
      startTransition(() => { setPlaying(null); });
    // eslint-disable-next-line react-hooks/set-state-in-effect
      setCurrentIdx(0);
      return;
    }

    const line = castData[currentIdx]!;
    let currentEventTuple: [number, string, string] | null = null;
    try {
      const parsed = JSON.parse(line) as [number, string, string];
      currentEventTuple = parsed;
      if (parsed[1] === 'o') {
        // Output event — write to xterm
        xtermRef.current?.write(parsed[2]);
      }
    } catch { /* skip malformed */ }

    const nextIdx = currentIdx + 1;
    if (nextIdx < castData.length) {
      const nextLine = castData[nextIdx]!;
      try {
        const nextEvent = JSON.parse(nextLine) as [number, string, string];
        if (currentEventTuple) {
          const rawDelay = (nextEvent[0] - currentEventTuple[0]) * 1000;
          // 压等待不压输出：静默时间封顶 500ms，连续输出保持原节奏
          const isSilent = currentEventTuple[1] === 'o' && currentEventTuple[2].length < 10;
          const delay = isSilent ? Math.min(rawDelay, 500) : Math.min(rawDelay, 200);
          playRef.current = setTimeout(() => setCurrentIdx(nextIdx), Math.max(delay, 10));
        } else {
          setCurrentIdx(nextIdx);
        }
      } catch {
        setCurrentIdx(nextIdx);
      }
    } else {
      // Playback complete
      setPlaying(null);
    }

    return () => {
      if (playRef.current) clearTimeout(playRef.current);
    };
  }, [playing, currentIdx, castData]);

  // Stop playback
  const stopPlayback = useCallback(() => {
    if (playRef.current) clearTimeout(playRef.current);
    setPlaying(null);
    setCurrentIdx(0);
    if (xtermRef.current) {
      xtermRef.current.dispose();
      xtermRef.current = null;
    }
    if (terminalContainerRef.current) {
      terminalContainerRef.current.innerHTML = '';
    }
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (playRef.current) clearTimeout(playRef.current);
      if (xtermRef.current) {
        xtermRef.current.dispose();
        xtermRef.current = null;
      }
    };
  }, []);

  if (isCollapsed) return null;

  return (
    <div style={{
      position: 'absolute', right: 0, top: 0, bottom: 0, width: 360,
      background: 'var(--vibe-sidebar-bg, var(--bg-2))',
      backdropFilter: 'blur(var(--vibe-right-panel-blur, 28px)) saturate(var(--vibe-right-panel-saturation, 145%))',
      WebkitBackdropFilter: 'blur(var(--vibe-right-panel-blur, 28px)) saturate(var(--vibe-right-panel-saturation, 145%))',
      borderLeft: '1px solid var(--vibe-sidebar-border, var(--border))',
      boxShadow: 'var(--vibe-sidebar-shadow)',
      display: 'flex', flexDirection: 'column', zIndex: 10,
    }}>
      {/* Header */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: `${SPACING.sm}px ${SPACING.md}px`,
        borderBottom: '1px solid var(--vibe-sidebar-border, var(--border))',
        fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text)',
      }}>
        <span>Recordings</span>
        <div style={{ display: 'flex', gap: 4 }}>
          <button onClick={loadRecordings} style={{ background: 'none', border: 'none', color: 'var(--text-faint)', cursor: 'pointer', padding: 2 }} title="Refresh" aria-label="Refresh">
            <RefreshCw size={12} />
          </button>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-faint)', cursor: 'pointer', padding: 2 }} title="Close" aria-label="Close">
            <X size={14} />
          </button>
        </div>
      </div>

      {/* Recording list */}
      <div style={{ flex: 1, overflow: 'auto', padding: SPACING.xs }}>
        {recordings.length === 0 && !isLoading && (
          <div style={{ textAlign: 'center', padding: SPACING.lg, color: 'var(--text-faint)', fontSize: FONT_SIZE.sm }}>
            No recordings yet
          </div>
        )}

        {isLoading && (
          <div style={{ textAlign: 'center', padding: SPACING.lg, color: 'var(--text-faint)', fontSize: FONT_SIZE.sm }}>
            Loading...
          </div>
        )}

        {recordings.map((rec) => (
          <div key={rec.id} style={{
            display: 'flex', alignItems: 'center', gap: SPACING.xs,
            padding: `${SPACING.xs}px ${SPACING.sm}px`,
            borderRadius: BORDER_RADIUS.sm,
            cursor: 'pointer', fontSize: FONT_SIZE.xs,
            color: 'var(--text-dim)',
            background: playing === rec.id ? 'var(--vibe-active-bg)' : 'transparent',
            transition: 'background 0.2s',
          }}>
            <button
              onClick={() => playing === rec.id ? stopPlayback() : playRecording(rec.id)}
              style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', padding: 2 }}
              title={playing === rec.id ? 'Stop' : 'Play'}
            >
              {playing === rec.id ? <Square size={12} /> : <Play size={12} />}
            </button>
            <div style={{ flex: 1, overflow: 'hidden' }}>
              <div style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {rec.session_id}
              </div>
              <div style={{ color: 'var(--text-faint)', fontSize: '10px' }}>
                {formatDuration(rec.duration_secs)} · {formatBytes(rec.size_bytes)} · {rec.width}×{rec.height}
              </div>
            </div>
            <span style={{ color: 'var(--text-faint)', fontSize: '10px' }}>
              {formatDate(rec.created_at)}
            </span>
          </div>
        ))}
      </div>

      {/* Playback terminal area — xterm read-only instance */}
      {playing && (
        <div style={{
          height: 200, overflow: 'hidden',
          borderTop: '1px solid var(--vibe-sidebar-border, var(--border))',
          background: 'var(--vibe-terminal-bg, var(--bg))',
          padding: '8px 12px 12px',
        }}>
          <div ref={terminalContainerRef} style={{ width: '100%', height: '100%' }} />
        </div>
      )}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

function formatDuration(secs: number): string {
  if (secs < 1) return '<1s';
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return `${m}m${s}s`;
}

function formatDate(ts: number): string {
  if (!ts) return '';
  const d = new Date(ts * 1000);
  const now = new Date();
  const diffMs = now.getTime() - d.getTime();
  if (diffMs < 3600000) return `${Math.round(diffMs / 60000)}m ago`;
  if (diffMs < 86400000) return `${Math.round(diffMs / 3600000)}h ago`;
  return d.toLocaleDateString();
}
