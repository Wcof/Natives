'use client';

import { useState, useCallback, useRef, useEffect } from 'react';

export type FollowMode = 'off' | 'terminal-follow' | 'file-follow';

// ── Follow Priority System ──
// HTML/Markdown = 3 (highest), Code/text = 2, Non-text = 1, Artifacts = 0

const ARTIFACT_EXTS = new Set([
  'dylib', 'so', 'o', 'a', 'node', 'wasm', 'bin', 'exe', 'dll', 'class', 'pyc',
  'dmg', 'pkg', 'deb', 'rpm', 'msi', 'jar', 'war', 'ipa', 'apk',
  'zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar', 'tgz',
  'woff', 'woff2', 'ttf', 'otf', 'eot',
]);

const NOEXT_TEXT = new Set([
  'Makefile', 'Dockerfile', 'LICENSE', 'README', 'CHANGELOG', 'Gemfile',
  'Rakefile', 'Vagrantfile', 'Procfile', 'Containerfile',
]);

export function getExt(path: string): string {
  const lastDot = path.lastIndexOf('.');
  if (lastDot <= 0) return ''; // no extension or dotfile like .gitignore
  return path.substring(lastDot + 1).toLowerCase();
}

export function isMarkdownFile(path: string): boolean {
  return ['md', 'markdown'].includes(getExt(path));
}

export function isCsvFile(path: string): boolean {
  return ['csv', 'tsv'].includes(getExt(path));
}

const ARCHIVE_EXTS = new Set(['zip', 'tar', 'tgz', 'gz', 'bz2', 'xz', '7z', 'rar']);
export function isArchiveFile(path: string): boolean {
  return ARCHIVE_EXTS.has(getExt(path)) || path.endsWith('.tar.gz');
}

// Known binary no-extension files
const NOEXT_BINARY = new Set(['node', 'a.out', 'core']);

function isArtifact(path: string): boolean {
  const ext = getExt(path);
  if (ARTIFACT_EXTS.has(ext)) return true;
  if (!ext) {
    const name = path.split('/').pop() || '';
    return NOEXT_BINARY.has(name);
  }
  return false;
}

function isHtmlOrMd(path: string): boolean {
  const ext = getExt(path);
  return ['html', 'htm', 'md', 'markdown'].includes(ext);
}

const TEXT_EXTS = new Set([
  'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'json', 'json5', 'jsonc',
  'css', 'scss', 'less', 'html', 'htm', 'vue', 'svelte',
  'py', 'rs', 'go', 'java', 'rb', 'php', 'c', 'cpp', 'cc', 'h', 'hpp', 'cs',
  'sh', 'bash', 'zsh', 'fish',
  'yaml', 'yml', 'toml', 'ini', 'conf', 'xml', 'sql', 'graphql',
  'md', 'markdown', 'txt', 'log', 'csv', 'tsv', 'env', 'gitignore',
  'swift', 'lua', 'kt', 'dart', 'r', 'rmd',
]);

function isTextFile(path: string): boolean {
  return TEXT_EXTS.has(getExt(path));
}

export function followPriority(path: string): number {
  if (isArtifact(path)) return 0;
  if (isHtmlOrMd(path)) return 3;
  if (isTextFile(path)) return 2;
  return 1;
}

// ── Follow State ──

interface FollowState {
  on: boolean;
  boundSessionId: string | null;
  currentPath: string | null;
  lastContent: string | null;
  pendingPath: string | null;
  throttleTimer: ReturnType<typeof setTimeout> | null;
  lastActivity: number; // timestamp of last terminal output
}

const followState: FollowState = {
  on: false,
  boundSessionId: null,
  currentPath: null,
  lastContent: null,
  pendingPath: null,
  throttleTimer: null,
  lastActivity: 0,
};

// Listeners
type FollowChangeListener = (path: string | null, content: string | null) => void;
const listeners = new Set<FollowChangeListener>();

export function onFollowChange(listener: FollowChangeListener): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function notifyListeners(path: string | null, content: string | null) {
  listeners.forEach(l => l(path, content));
}

// ── Scope Check ──

function inFollowScope(filePath: string, scopeRoot: string | null): boolean {
  if (!scopeRoot) return false;
  return filePath.startsWith(scopeRoot);
}

function boundAgentActive(): boolean {
  if (!followState.boundSessionId) return false;
  // Check if terminal had recent output (within 8s)
  if (Date.now() - followState.lastActivity < 8000) return true;
  // Check via agent status
  return false; // Will be updated by terminal output hook
}

// ── Core Follow Logic ──

/**
 * Entry point: called when a file change event arrives.
 * @param dir Directory of the changed file
 * @param filename Name of the changed file
 * @param scopeRoot The bound terminal's CWD (scope root)
 */
export function followChange(dir: string, filename: string, scopeRoot: string | null) {
  if (!followState.on || !followState.boundSessionId) return;

  // Scope check
  const fullPath = dir.endsWith('/') ? dir + filename : dir + '/' + filename;
  if (!inFollowScope(fullPath, scopeRoot)) return;

  // Activity check
  if (!boundAgentActive()) return;

  // Same file — just re-render
  if (fullPath === followState.currentPath) {
    scheduleFollowRender();
    return;
  }

  // Different file — priority check
  const newPrio = followPriority(fullPath);
  const pendingPrio = followState.pendingPath ? followPriority(followState.pendingPath) : -1;

  // Higher or equal priority displaces pending
  if (newPrio >= pendingPrio) {
    followState.pendingPath = fullPath;
  }

  // Throttle: 900ms if already following, 120ms if first file
  const delay = followState.currentPath ? 900 : 120;
  if (followState.throttleTimer) clearTimeout(followState.throttleTimer);
  followState.throttleTimer = setTimeout(() => {
    const target = followState.pendingPath;
    followState.pendingPath = null;
    followState.throttleTimer = null;
    if (target) followSwitch(target);
  }, delay);
}

function followSwitch(fullPath: string) {
  followState.currentPath = fullPath;
  // Navigate file browser to the file's directory
  const dir = fullPath.substring(0, fullPath.lastIndexOf('/')) || '/';
  window.dispatchEvent(new CustomEvent('navigate-files', { detail: { path: dir, selectFile: fullPath } }));
  notifyListeners(fullPath, null);
}

let renderTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleFollowRender() {
  if (renderTimer) clearTimeout(renderTimer);
  renderTimer = setTimeout(() => {
    renderTimer = null;
    notifyListeners(followState.currentPath, null);
  }, 300);
}

// ── Terminal Activity Hook ──

export function recordTerminalActivity(sessionId: string) {
  if (sessionId === followState.boundSessionId) {
    followState.lastActivity = Date.now();
  }
}

// ── Public API ──

export function setFileFollow(on: boolean, sessionId?: string) {
  if (on) {
    followState.on = true;
    followState.boundSessionId = sessionId || null;
    followState.lastActivity = Date.now();
  } else {
    followState.on = false;
    followState.boundSessionId = null;
    followState.currentPath = null;
    followState.lastContent = null;
    followState.pendingPath = null;
    if (followState.throttleTimer) {
      clearTimeout(followState.throttleTimer);
      followState.throttleTimer = null;
    }
    if (renderTimer) {
      clearTimeout(renderTimer);
      renderTimer = null;
    }
  }
  notifyListeners(followState.currentPath, null);
}

export function getFollowState() {
  return { ...followState };
}

export function isFollowing(): boolean {
  return followState.on;
}

export function getFollowPath(): string | null {
  return followState.currentPath;
}

// ── React Hook ──

export function useFollowMode(
  currentDir?: string,
  onFollowAction?: (mode: FollowMode, path: string) => void,
  writeToTerminal?: (cmd: string) => void,
) {
  const [mode, setMode] = useState<FollowMode>('off');
  const prevModeRef = useRef<FollowMode>('off');

  const cycleMode = useCallback(() => {
    setMode((prev) => {
      if (prev === 'off') return 'terminal-follow';
      if (prev === 'terminal-follow') return 'file-follow';
      return 'off';
    });
  }, []);

  useEffect(() => {
    const prevMode = prevModeRef.current;
    prevModeRef.current = mode;
    if (prevMode === mode) return;
    if (!currentDir) return;

    if (mode === 'terminal-follow') {
      onFollowAction?.('terminal-follow', currentDir);
      writeToTerminal?.(`cd ${JSON.stringify(currentDir)}`);
    } else if (mode === 'file-follow') {
      onFollowAction?.('file-follow', currentDir);
      window.dispatchEvent(new CustomEvent('navigate-files', { detail: { path: currentDir } }));
      setFileFollow(true);
    } else {
      setFileFollow(false);
    }
  }, [mode, currentDir, onFollowAction, writeToTerminal]);

  const terminalFollows = mode === 'terminal-follow';
  const fileBrowserFollows = mode === 'file-follow';

  return { mode, cycleMode, terminalFollows, fileBrowserFollows, setMode };
}

// ── Changed Range Detection ──
// O(n) common prefix/suffix diff for locating changed lines

export function changedRange(oldStr: string, newStr: string): { start: number; end: number } {
  const oldLines = oldStr.split('\n');
  const newLines = newStr.split('\n');
  let prefix = 0;
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) prefix++;
  let suffix = 0;
  while (
    suffix < oldLines.length - prefix &&
    suffix < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]
  ) suffix++;
  return { start: prefix, end: newLines.length - suffix };
}
