'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { followPriority, changedRange, getFollowState, recordTerminalActivity, getExt } from '@/lib/follow-mode';
import { parseAgentAction, composeNarration } from '@/lib/agent-narration';
import { highlightCode, extToLanguage } from '@/lib/shiki-utils';

interface FollowRendererProps {
  filePath: string | null;
  httpPort: number;
}

export default function FollowRenderer({ filePath, httpPort }: FollowRendererProps) {
  const [content, setContent] = useState<string | null>(null);
  const [lastContent, setLastContent] = useState<string | null>(null);
  const [narration, setNarration] = useState('');
  const [highlightedLines, setHighlightedLines] = useState<Set<number>>(new Set());
  const contentRef = useRef<string | null>(null);

  // Fetch file content when path changes
  useEffect(() => {
    if (!filePath) { setContent(null); return; }
    let cancelled = false;
    fetch(`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(filePath)}`)
      .then(r => r.text())
      .then(text => {
        if (cancelled) return;
        setLastContent(contentRef.current);
        contentRef.current = text;
        setContent(text);
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [filePath, httpPort]);

  // Compute changed lines
  useEffect(() => {
    if (!content || !lastContent) { setHighlightedLines(new Set()); return; }
    const range = changedRange(lastContent, content);
    const lines = new Set<number>();
    for (let i = range.start; i < range.end; i++) lines.add(i);
    setHighlightedLines(lines);
    // Clear highlight after 2s
    const timer = setTimeout(() => setHighlightedLines(new Set()), 2000);
    return () => clearTimeout(timer);
  }, [content, lastContent]);

  // Narration polling
  useEffect(() => {
    const interval = setInterval(() => {
      const state = getFollowState();
      if (!state.on || !state.currentPath) { setNarration(''); return; }

      // Get terminal lines for action parsing (placeholder — needs terminal buffer access)
      const action = ''; // Will be populated by terminal integration
      const prio = followPriority(state.currentPath);
      const isArtifact = prio === 0;
      setNarration(composeNarration(
        Date.now() - state.lastActivity < 8000,
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
        height: '100%', color: 'var(--text-faint)', fontSize: 12,
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
      <div style={{ padding: 20, textAlign: 'center' }}>
        <div style={{ fontSize: 32, marginBottom: 8 }}>📦</div>
        <div style={{ color: 'var(--text)', fontSize: 13, fontWeight: 600 }}>
          {filePath.split('/').pop()}
        </div>
        <div style={{ color: 'var(--text-faint)', fontSize: 11, marginTop: 4 }}>
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
        <LiveHtmlPreview path={filePath} httpPort={httpPort} />
        {narration && <NarrationBar text={narration} />}
      </div>
    );
  }

  // Markdown: rendered preview
  if (isMd && content) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
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

function LiveHtmlPreview({ path, httpPort }: { path: string; httpPort: number }) {
  const [currentSrc, setCurrentSrc] = useState<string>('');
  const [nextSrc, setNextSrc] = useState<string | null>(null);
  const swappingRef = useRef(false);
  const dirtyRef = useRef(false);
  const nextSrcRef = useRef<string | null>(null);

  useEffect(() => {
    const url = `http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(path)}&v=${Date.now()}`;
    if (!currentSrc) {
      setCurrentSrc(url);
      return;
    }
    if (swappingRef.current) {
      dirtyRef.current = true;
      return;
    }
    swappingRef.current = true;
    nextSrcRef.current = url;
    setNextSrc(url);
  }, [path, httpPort, currentSrc]);

  const handleNextLoad = useCallback(() => {
    if (!swappingRef.current) return;
    swappingRef.current = false;
    const loadedSrc = nextSrcRef.current;
    if (loadedSrc) setCurrentSrc(loadedSrc);
    setNextSrc(null);
    nextSrcRef.current = null;
    if (dirtyRef.current) {
      dirtyRef.current = false;
      setCurrentSrc(`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(path)}&v=${Date.now()}`);
    }
  }, [httpPort, path]);

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
          src={currentSrc}
          sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          style={{ width: '100%', height: '100%', border: 'none', background: '#fff' }}
        />
      )}
      {nextSrc && (
        <iframe
          src={nextSrc}
          sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          onLoad={handleNextLoad}
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
        fontSize: 13, lineHeight: 1.7, color: 'var(--text)',
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
          fontSize: 12, lineHeight: 1.6,
          fontFamily: 'var(--font-mono, monospace)',
          background: 'var(--bg-2, #131410)',
        }}
      />
    );
  }

  return (
    <pre ref={containerRef as any} style={{
      flex: 1, overflow: 'auto', margin: 0, padding: 12,
      fontSize: 12, lineHeight: 1.6,
      fontFamily: 'var(--font-mono, monospace)',
      color: 'var(--text)', whiteSpace: 'pre-wrap',
      background: 'var(--bg-2, #131410)',
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
      borderTop: '1px solid var(--border, #262920)',
      fontSize: 11,
      color: 'var(--text-dim, #9b9d8c)',
      background: 'var(--bg, #0b0c0a)',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
    }}>
      {text}
    </div>
  );
}
