'use client';

import dynamic from 'next/dynamic';
import { useCallback, useRef, useState, useEffect } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

// Dynamic import — Monaco Editor is ~2MB, only load when actually editing code
const Editor = dynamic(() => import('@monaco-editor/react').then((m) => m.default), {
  ssr: false,
  loading: () => <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-dim)', fontSize: FONT_SIZE.md }}>Loading editor…</div>,
});

interface MonacoEditorProps {
  content: string;
  language: string;
  onSave?: (value: string) => void;
  readOnly?: boolean;
}

// Language mapping (from Natives2)
const EXT_TO_LANG: Record<string, string> = {
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript',
  ts: 'typescript', tsx: 'typescript', json: 'json', json5: 'json', jsonc: 'json',
  md: 'markdown', markdown: 'markdown', html: 'html', htm: 'html', vue: 'html',
  css: 'css', scss: 'scss', less: 'less', py: 'python', go: 'go', rs: 'rust',
  java: 'java', rb: 'ruby', php: 'php', c: 'c', cpp: 'cpp', cc: 'cpp', h: 'cpp',
  hpp: 'cpp', cs: 'csharp', sh: 'shell', bash: 'shell', zsh: 'shell',
  yml: 'yaml', yaml: 'yaml', toml: 'ini', ini: 'ini', conf: 'ini', xml: 'xml',
  sql: 'sql', swift: 'swift', lua: 'lua', kt: 'kotlin', dart: 'dart', r: 'r',
  graphql: 'graphql', svelte: 'html',
};

const WORD_WRAP_EXTS = new Set(['md', 'markdown', 'txt', 'log', 'srt', 'vtt', 'ass']);

export default function MonacoEditor({ content, language, onSave, readOnly }: MonacoEditorProps) {
  const editorRef = useRef<any>(null);

  const lang = EXT_TO_LANG[language] || 'plaintext';
  const wordWrap = WORD_WRAP_EXTS.has(language) ? 'on' : 'off';

  // Detect theme — light vs dark Monaco theme
  const getMonacoTheme = (): string => {
    if (typeof document !== 'undefined') {
      const htmlTheme = document.documentElement.getAttribute('data-theme');
      if (htmlTheme === 'frosted-jasmine') return 'vs-light';
    }
    return 'vs-dark';
  };

  const [monacoTheme, setMonacoTheme] = useState<string>(getMonacoTheme);

  useEffect(() => {
    // Listen for theme changes
    const handler = () => setMonacoTheme(getMonacoTheme());
    window.addEventListener('theme-changed', handler);
    // Also observe data-theme attribute changes
    const observer = new MutationObserver(handler);
    if (document.documentElement) {
      observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    }
    return () => {
      window.removeEventListener('theme-changed', handler);
      observer.disconnect();
    };
  }, []);

  const handleMount = useCallback((editor: any, monaco: any) => {
    editorRef.current = editor;
    // Cmd+S save
    if (onSave) {
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        onSave(editor.getValue());
      });
    }
  }, [onSave]);

  return (
    <Editor
      height="100%"
      defaultLanguage={lang}
      defaultValue={content}
      theme={monacoTheme}
      options={{
        readOnly: readOnly ?? false,
        minimap: { enabled: false },
        fontSize: FONT_SIZE.lg,
        lineHeight: 1.7,
        wordWrap,
        scrollBeyondLastLine: false,
        automaticLayout: true,
        tabSize: 2,
        renderWhitespace: 'none',
        smoothScrolling: true,
        padding: { top: 10, bottom: 10 },
        fontLigatures: true,
      }}
      onMount={handleMount}
    />
  );
}
