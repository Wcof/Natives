'use client';

import { startTransition, useState, useEffect, useRef, useCallback } from 'react';
import { Package } from 'lucide-react';
import { followPriority, changedRange, getFollowState, recordTerminalActivity, getExt } from '@/lib/follow-mode';
import { getScrollbackLines } from '@/lib/path-detector';
import { parseAgentAction, composeNarration } from '@/lib/agent-narration';
import { highlightCode, extToLanguage } from '@/lib/shiki-utils';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import { IFRAME_SANDBOX } from '@/lib/iframe-manager';
import { useTheme } from '@/context/ThemeContext';

interface FollowRendererProps {
  filePath: string | null;
}

export default function FollowRenderer({ filePath }: FollowRendererProps) {
  const [content, setContent] = useState<string | null>(null);
  const [lastContent, setLastContent] = useState<string | null>(null);
  const [narration, setNarration] = useState('');
  const [highlightedLines, setHighlightedLines] = useState<Set<number>>(new Set());
  const contentRef = useRef<string | null>(null);

  // Fetch file content when path changes — use Tauri IPC fs.readFile
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (!filePath) { setContent(null); return; }
    let cancelled = false;
    (async () => {
      try {
        const text = await window.nativesAPI?.fs?.readFile?.(filePath) as string | undefined;
        if (cancelled || text === undefined) return;
        setLastContent(contentRef.current);
        contentRef.current = text;
        setContent(text);
      } catch {
        // fallback: do nothing
      }
    })();
    return () => { cancelled = true; };
  }, [filePath]);

  // Compute changed lines
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (!content || !lastContent) { setHighlightedLines(new Set()); return; }
    const range = changedRange(lastContent, content);
    const lines = new Set<number>();
    for (let i = range.start; i < range.end; i++) lines.add(i);
    setHighlightedLines(lines);
    // Clear highlight after 2s
    const timer = setTimeout(() => setHighlightedLines(new Set()), 2000);
    return () => clearTimeout(timer);
  }, [content, lastContent]);

  // Narration polling — connect to terminal output buffer
  useEffect(() => {
    const interval = setInterval(() => {
      const state = getFollowState();
      if (!state.on || !state.currentPath) { setNarration(''); return; }

      // Get terminal output lines for action parsing
      const terminalLines = getScrollbackLines();
      const action = terminalLines.length > 0 ? parseAgentAction(terminalLines) : '';
      const prio = followPriority(state.currentPath);
      const isArtifact = prio === 0;

      // Determine if agent is active (has recent terminal output within 8s)
      const isActive = terminalLines.length > 0 && Date.now() - state.lastActivity < 8000;

      setNarration(composeNarration(
        isActive,
        action,
        state.currentPath,
        isArtifact,
      ));
    }, 1200);
    return () => clearInterval(interval);
  }, []);

  if (!filePath) {
    return (
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)',
      }}>
        Follow mode active — waiting for agent edits...
      </div>
    );
  }

  const ext = getExt(filePath);
  const isHtml = ['html', 'htm'].includes(ext);
  const isMd = ['md', 'markdown'].includes(ext);
  const prio = followPriority(filePath);

  // Artifacts: show card
  if (prio === 0) {
    return (
      <div style={{ padding: SPACING.xl, textAlign: 'center' }}>
        <Package size={32} style={{ color: 'var(--accent)', marginBottom: SPACING.sm }} />
        <div style={{ color: 'var(--text)', fontSize: 'var(--fs-md)', fontWeight: 600 }}>
          {filePath.split('/').pop()}
        </div>
        <div style={{ color: 'var(--text-faint)', fontSize: FONT_SIZE.sm, marginTop: SPACING.xs }}>
          Build artifact generated
        </div>
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  // HTML: double-buffered iframe
  if (isHtml) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <LiveHtmlPreview path={filePath} />
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  // Markdown: rendered preview
  if (isMd && content) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-md)' }}>
          <LiveMarkdownPreview content={content} />
        </div>
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  // Code: syntax highlighted with change lines
  if (content) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <LiveCodePreview content={content} highlightedLines={highlightedLines} ext={ext} />
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  return null;
}

// ── Live HTML Preview (double-buffered iframe) ──

function LiveHtmlPreview({ path }: { path: string }) {
  const [currentSrc, setCurrentSrc] = useState<string>('');
  const [nextSrc, setNextSrc] = useState<string | null>(null);
  const swappingRef = useRef(false);
  const dirtyRef = useRef(false);
  const nextSrcRef = useRef<string | null>(null);
  const currentIframeRef = useRef<HTMLIFrameElement>(null);
  const nextIframeRef = useRef<HTMLIFrameElement>(null);
  const lastUrlRef = useRef('');
  const { themeId } = useTheme();

  // Get theme CSS variables to pass into sandboxed iframes
  const getThemeCSS = useCallback((): string => {
    const root = document.documentElement;
    const computed = getComputedStyle(root);
    const vars: string[] = [];
    // Extract key CSS custom properties for theme coherence
    const keys = [
      '--bg', '--text', '--text-dim', '--text-faint', '--accent',
      '--vibe-toolbar-bg', '--vibe-content-bg', '--vibe-btn-border', '--vibe-btn-text',
      '--mac-red', '--mac-yellow', '--mac-green',
      '--font-sans', '--font-mono', '--fs-sm', '--fs-md',
    ];
    for (const key of keys) {
      const val = computed.getPropertyValue(key).trim();
      if (val) vars.push(`${key}: ${val};`);
    }
    return vars.join('\n');
  }, []);

  // Send theme CSS to iframe via postMessage
  const sendThemeToIframe = useCallback((iframe: HTMLIFrameElement | null) => {
    if (!iframe?.contentWindow) return;
    try {
      iframe.contentWindow.postMessage(
        { type: 'natives-theme-update', css: getThemeCSS(), themeId },
        '*', // targetOrigin '*' is safe here because the iframe is sandboxed (no allow-same-origin)
      );
    } catch { /* cross-origin errors are expected if iframe navigated */ }
  }, [getThemeCSS, themeId]);

  // Re-send theme on every themeId change
  useEffect(() => {
    sendThemeToIframe(currentIframeRef.current);
    // Also re-render with cache-busting URL so the iframe reloads with new theme
    if (currentSrc) {
      startTransition(() => { setCurrentSrc(`${currentSrc.split('&v=')[0]}&v=${Date.now()}`); });
    }
  }, [themeId, sendThemeToIframe, currentSrc]);

  useEffect(() => {
    // Read file content via Tauri IPC and create a blob URL for the iframe
    (async () => {
      try {
        const api = window.nativesAPI;
        if (!api?.fs?.readFile) {
          console.warn('[FollowRenderer] fs.readFile not available');
          return;
        }

        const result = await api.fs.readFile(path);
        if (!result) return;
        const content = typeof result === 'string' ? result : (result as any).content;
        const mimeType = path.endsWith('.html') || path.endsWith('.htm') ? 'text/html' : 'text/plain';
        const blob = new Blob([content || ''], { type: mimeType });
        const url = URL.createObjectURL(blob);

        if (!currentSrc) {
          setCurrentSrc(url);
          lastUrlRef.current = url;
          return;
        }
        if (swappingRef.current) {
          dirtyRef.current = true;
          return;
        }
        swappingRef.current = true;
        nextSrcRef.current = url;
        setNextSrc(url);
      } catch (err) {
        console.error('[FollowRenderer] Failed to load file:', err);
      }
    })();
  }, [path, currentSrc]);

  const handleNextLoad = useCallback(async () => {
    if (!swappingRef.current) return;
    swappingRef.current = false;
    const loadedSrc = nextSrcRef.current;
    if (loadedSrc) setCurrentSrc(loadedSrc);
    setNextSrc(null);
    nextSrcRef.current = null;

    // Send theme to newly loaded iframe
    sendThemeToIframe(currentIframeRef.current);

    if (dirtyRef.current) {
      dirtyRef.current = false;
      try {
        const api = window.nativesAPI;
        if (api?.fs?.readFile) {
          const result = await api.fs.readFile(path);
          if (result) {
            const content = typeof result === 'string' ? result : (result as any).content;
            const mimeType = path.endsWith('.html') || path.endsWith('.htm') ? 'text/html' : 'text/plain';
            const blob = new Blob([content || ''], { type: mimeType });
            setCurrentSrc(URL.createObjectURL(blob));
          }
        }
      } catch { /* ignore fallback */ }
    }
  }, [path, sendThemeToIframe]);

  // Force swap after 2.5s
  useEffect(() => {
    if (!nextSrc) return;
    const timer = setTimeout(handleNextLoad, 2500);
    return () => clearTimeout(timer);
  }, [nextSrc, handleNextLoad]);

  return (
    <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
      {currentSrc && (
        <iframe
          ref={currentIframeRef}
          src={currentSrc}
          sandbox={IFRAME_SANDBOX}
          onLoad={() => sendThemeToIframe(currentIframeRef.current)}
          style={{ width: '100%', height: '100%', border: 'none', background: '#fff' }}
        />
      )}
      {nextSrc && (
        <iframe
          ref={nextIframeRef}
          src={nextSrc}
          sandbox={IFRAME_SANDBOX}
          onLoad={() => {
            handleNextLoad();
            sendThemeToIframe(nextIframeRef.current);
          }}
          style={{
            position: 'absolute', inset: 0,
            width: '100%', height: '100%',
            border: 'none', background: '#fff',
            opacity: 0, // Hidden until swap
          }}
        />
      )}
    </div>
  );
}

// ── HTML Sanitization ──
const ESC_MAP: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#x27;' };
function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, c => ESC_MAP[c] || c);
}

function sanitizeHtml(html: string): string {
  // Strip script/event-handler tags, keep safe HTML
  return html
    .replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '')
    .replace(/\son\w+="[^"]*"/gi, '')
    .replace(/\son\w+='[^']*'/gi, '');
}

// ── Live Markdown Preview ──

function LiveMarkdownPreview({ content }: { content: string }) {
  const [renderedHtml, setRenderedHtml] = useState('');

  useEffect(() => {
    (async () => {
      try {
        const { marked } = await import('marked');
        const html = await marked(content);
        setRenderedHtml(sanitizeHtml(html as string));
      } catch {
        setRenderedHtml(`<pre>${escapeHtml(content)}</pre>`);
      }
    })();
  }, [content]);

  return (
    <div
      className="markdown-body"
      dangerouslySetInnerHTML={{ __html: renderedHtml }}
      style={{
        fontSize: 'var(--fs-md)', lineHeight: 1.7, color: 'var(--text)',
        fontFamily: 'var(--font-sans, system-ui)',
      }}
    />
  );
}

// ── Live Code Preview ──

function LiveCodePreview({ content, highlightedLines, ext }: {
  content: string;
  highlightedLines: Set<number>;
  ext: string;
}) {
  const [highlightedHtml, setHighlightedHtml] = useState<string>('');
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    highlightCode(content, extToLanguage(ext)).then(html => {
      if (!cancelled) setHighlightedHtml(html);
    });
    return () => { cancelled = true; };
  }, [content, ext]);

  // Scroll to first changed line
  useEffect(() => {
    if (highlightedLines.size === 0 || !containerRef.current) return;
    const firstLine = Math.min(...highlightedLines);
    const lineEl = containerRef.current.querySelector(`[data-line="${firstLine}"]`);
    lineEl?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }, [highlightedLines]);

  if (highlightedHtml) {
    return (
      <div
        ref={containerRef}
        dangerouslySetInnerHTML={{ __html: highlightedHtml }}
        style={{
          flex: 1, overflow: 'auto', padding: 12,
          fontSize: 'var(--fs-sm)', lineHeight: 1.6,
          fontFamily: 'var(--font-mono, monospace)',
          background: 'var(--vibe-toolbar-bg)',
        }}
      />
    );
  }

  return (
    <pre ref={containerRef as any} style={{
      flex: 1, overflow: 'auto', margin: 0, padding: 12,
      fontSize: 'var(--fs-sm)', lineHeight: 1.6,
      fontFamily: 'var(--font-mono, monospace)',
      color: 'var(--text)', whiteSpace: 'pre-wrap',
      background: 'var(--vibe-toolbar-bg)',
    }}>
      {content}
    </pre>
  );
}

// ── Narration Bar ──

function NarrationBar({ text }: { text: string }) {
  if (!text) return null;
  return (
    <div style={{
      padding: '4px 12px',
      borderTop: '1px solid var(--vibe-btn-border)',
      fontSize: FONT_SIZE.sm,
      color: 'var(--vibe-btn-text)',
      background: 'var(--vibe-content-bg)',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
    }}>
      {text}
    </div>
  );
}
