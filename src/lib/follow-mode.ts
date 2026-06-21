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
  'pyo', 'nib', 'car',
]);

// Artifact path patterns (Natives2: .app/.framework bundles count as artifacts)
const ARTIFACT_PATTERNS = [
  /\.app\//, /\.framework\//, /\.xcassets\//, /\.xcodeproj\//,
  /\/DerivedData\//, /\/build\/Products\//,
];

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
  // Check path patterns (.app bundles, frameworks, etc.)
  if (ARTIFACT_PATTERNS.some(p => p.test(path))) return true;
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
  /** Agent status from agent-status-changed events */
  agentStatus: 'idle' | 'busy' | null;
  /** true = user manually browsed/edited, follow paused until next agent activity */
  paused: boolean;
  /** HTML double buffer: pre-render target for zero-flash HTML/MD switching */
  htmlBufferPath: string | null;
  swapping: boolean;
}

const followState: FollowState = {
  on: false,
  boundSessionId: null,
  currentPath: null,
  lastContent: null,
  pendingPath: null,
  throttleTimer: null,
  lastActivity: 0,
  agentStatus: null,
  paused: false,
  htmlBufferPath: null,
  swapping: false,
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
  // Explicit agent status takes priority
  if (followState.agentStatus === 'busy') return true;
  if (followState.agentStatus === 'idle') return false;
  // Fallback: check if terminal had recent output (within 8s)
  return Date.now() - followState.lastActivity < 8000;
}

// ── Core Follow Logic ──

/**
 * Entry point: called when a file change event arrives.
 * @param dir Directory of the changed file
 * @param filename Name of the changed file
 * @param scopeRoot The bound terminal's CWD (scope root)
 */
// Noise patterns: skip system/generated files in follow mode
const FOLLOW_NOISE = [
  /\.git\//, /node_modules\//, /\.next\//, /dist\//, /build\//,
  /__pycache__\//, /\.DS_Store$/, /\.db-wal$/, /\.db-shm$/, /\.lock$/,
];

/**
 * Manual takeover: any manual file browse/edit pauses follow mode.
 * Follow resumes automatically when the agent becomes active again.
 */
export function followManualTakeover() {
  if (!followState.on) return;
  followState.paused = true;
}

/**
 * Update agent status from agent-status-changed events.
 * When agent becomes active after being paused, auto-resume follow.
 */
export function followAgentStatus(status: 'idle' | 'busy' | 'working') {
  const prev = followState.agentStatus;
  followState.agentStatus = status === 'working' ? 'busy' : status;
  // Auto-resume on agent activity after manual takeover
  if (followState.paused && status !== 'idle' && prev !== 'busy') {
    followState.paused = false;
  }
}

export function followChange(dir: string, filename: string, scopeRoot: string | null) {
  if (!followState.on || !followState.boundSessionId) return;
  // Paused by manual takeover — skip until agent resumes
  if (followState.paused) return;

  // Scope check
  const fullPath = dir.endsWith('/') ? dir + filename : dir + '/' + filename;
  if (!inFollowScope(fullPath, scopeRoot)) return;

  // Noise filter
  if (FOLLOW_NOISE.some(p => p.test(fullPath))) return;

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
  const wasHtml = followState.currentPath && isHtmlOrMd(followState.currentPath);
  const isHtml = isHtmlOrMd(fullPath);

  // HTML double buffer: if switching between HTML/MD files, use buffer swap
  if (isHtml && wasHtml) {
    followState.swapping = true;
    followState.htmlBufferPath = fullPath;
    // Pre-render into buffer — the renderer will call followCommitSwap()
    // after the new content is loaded into the hidden buffer
    const dir = fullPath.substring(0, fullPath.lastIndexOf('/')) || '/';
    window.dispatchEvent(new CustomEvent('navigate-files', { detail: { path: dir, selectFile: fullPath } }));
    notifyListeners(fullPath, null);
    // Auto-commit swap after a short delay (fallback if renderer doesn't call it)
    setTimeout(() => {
      if (followState.swapping) followCommitSwap();
    }, 200);
    return;
  }

  followState.currentPath = fullPath;
  followState.htmlBufferPath = null;
  followState.swapping = false;
  // Navigate file browser to the file's directory
  const dir = fullPath.substring(0, fullPath.lastIndexOf('/')) || '/';
  window.dispatchEvent(new CustomEvent('navigate-files', { detail: { path: dir, selectFile: fullPath } }));
  notifyListeners(fullPath, null);
}

/**
 * Commit the HTML double buffer swap. Called by the renderer after
 * the new HTML/MD content is fully loaded into the hidden buffer.
 * This eliminates white flash during HTML-to-HTML transitions.
 */
export function followCommitSwap() {
  if (!followState.swapping) return;
  followState.currentPath = followState.htmlBufferPath;
  followState.htmlBufferPath = null;
  followState.swapping = false;
  notifyListeners(followState.currentPath, null);
}

export function isSwapping(): boolean {
  return followState.swapping;
}

export function getHtmlBufferPath(): string | null {
  return followState.htmlBufferPath;
}

let renderTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleFollowRender() {
  if (renderTimer) clearTimeout(renderTimer);
  renderTimer = setTimeout(() => {
    renderTimer = null;
    notifyListeners(followState.currentPath, null);
  }, 300);
}

// ── Terminal Output Buffer for Action Parsing ──

// ── Terminal Activity Hook ──

export function recordTerminalActivity(sessionId: string) {
  if (sessionId === followState.boundSessionId) {
    followState.lastActivity = Date.now();
  }
}

// ── Public API ──

// Manual takeover listener — any navigate-files event NOT from followSwitch pauses follow
let _manualNavHandler: ((e: Event) => void) | null = null;

export function setFileFollow(on: boolean, sessionId?: string) {
  if (on) {
    followState.on = true;
    followState.boundSessionId = sessionId || null;
    followState.lastActivity = Date.now();
    followState.paused = false;
    followState.agentStatus = null;
    // Install manual takeover listener
    if (!_manualNavHandler) {
      _manualNavHandler = (e: Event) => {
        // If we're not in a follow-initiated switch, treat as manual takeover
        if (!followState.swapping && followState.on) {
          followManualTakeover();
        }
      };
      window.addEventListener('navigate-files', _manualNavHandler);
    }
  } else {
    followState.on = false;
    followState.boundSessionId = null;
    followState.currentPath = null;
    followState.lastContent = null;
    followState.pendingPath = null;
    followState.paused = false;
    followState.agentStatus = null;
    followState.htmlBufferPath = null;
    followState.swapping = false;
    if (followState.throttleTimer) {
      clearTimeout(followState.throttleTimer);
      followState.throttleTimer = null;
    }
    if (renderTimer) {
      clearTimeout(renderTimer);
      renderTimer = null;
    }
    // Remove manual takeover listener
    if (_manualNavHandler) {
      window.removeEventListener('navigate-files', _manualNavHandler);
      _manualNavHandler = null;
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
