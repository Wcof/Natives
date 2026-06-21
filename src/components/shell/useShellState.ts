'use client';

import { startTransition, useState, useEffect, useRef, useCallback } from 'react';
import type { Locale } from '@/i18n';
import type { RightPanelMode } from './RightPanel';
import type { PreviewSubMode } from '@/components/files/FilePreview';
import type { FileEntry } from '@/types/file';
import { useFollowMode } from '@/lib/follow-mode';

export interface ShellState {
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  rightPanelMode: RightPanelMode;
  rightPanelWidth: number;
  previewSubMode: PreviewSubMode;
  terminalCollapsed: boolean;
  terminalHeight: number;
  terminalMaximized: boolean;
  cmdkOpen: boolean;
}

export interface VisualConfig {
  blurAmount: number;
  displacementScale: number;
  saturation: number;
  aberrationIntensity: number;
  elasticity: number;
  cornerRadius: number;
  showWallpaper: boolean;
  showBlobs: boolean;
}

export interface ShellStateReturn {
  // Layout
  state: ShellState;
  setState: React.Dispatch<React.SetStateAction<ShellState>>;
  stateRef: React.RefObject<ShellState>;
  // View
  activeView: string;
  setActiveView: React.Dispatch<React.SetStateAction<string>>;
  themeReady: boolean;
  setThemeReady: React.Dispatch<React.SetStateAction<boolean>>;
  locale: Locale;
  setLocale: React.Dispatch<React.SetStateAction<Locale>>;
  httpPort: number | null;
  setHttpPort: React.Dispatch<React.SetStateAction<number | null>>;
  // Refs
  terminalSessionIdRef: React.RefObject<string | null>;
  iframeContainerRef: React.RefObject<HTMLDivElement | null>;
  activeModuleRef: React.RefObject<string | null>;
  contentRef: React.RefObject<HTMLDivElement | null>;
  // Preview
  selectedFile: FileEntry | null;
  setSelectedFile: React.Dispatch<React.SetStateAction<FileEntry | null>>;
  editMode: boolean;
  setEditMode: React.Dispatch<React.SetStateAction<boolean>>;
  // Visual
  visualConfig: VisualConfig;
  setVisualConfig: React.Dispatch<React.SetStateAction<VisualConfig>>;
  // Follow
  followMode: string;
  cycleFollowMode: () => void;
  // Crash
  crashedModules: Set<string>;
  setCrashedModules: React.Dispatch<React.SetStateAction<Set<string>>>;
  iframeReloadKey: number;
  setIframeReloadKey: React.Dispatch<React.SetStateAction<number>>;
  // Onboarding
  needsOnboarding: boolean | null;
  setNeedsOnboarding: React.Dispatch<React.SetStateAction<boolean | null>>;
  // Screenshot/Release
  annotatingFile: string | null;
  setAnnotatingFile: React.Dispatch<React.SetStateAction<string | null>>;
  annotationImageUrl: string | null;
  setAnnotationImageUrl: React.Dispatch<React.SetStateAction<string | null>>;
  releaseWizardOpen: boolean;
  setReleaseWizardOpen: React.Dispatch<React.SetStateAction<boolean>>;
  // Handlers
  toggleSidebar: () => void;
  toggleTerminal: () => void;
  toggleMaximized: () => void;
  toggleRightPanel: (mode: RightPanelMode) => void;
  handleModuleSelect: (moduleId: string) => void;
  handleFileSelect: (file: FileEntry) => void;
  handleInstallModule: (moduleId: string) => void;
  setRightPanelMode: (mode: RightPanelMode) => void;
}

export function useShellState(): ShellStateReturn {
  // ── Layout state ──
  const [state, setState] = useState<ShellState>({
    sidebarCollapsed: false,
    sidebarWidth: 248,
    rightPanelMode: 'closed',
    rightPanelWidth: 320,
    previewSubMode: 'preview',
    terminalCollapsed: true,
    terminalHeight: 280,
    terminalMaximized: false,
    cmdkOpen: false,
  });
  const stateRef = useRef(state);
  useEffect(() => { stateRef.current = state; }, [state]);

  // ── View state ──
  const [activeView, setActiveView] = useState<string>(() => {
    if (typeof window === 'undefined') return 'dashboard';
    const path = window.location.pathname.replace(/\/+$/, '') || '/';
    const routingTable: Record<string, string> = {
      '/': 'dashboard', '/files': 'files', '/tools': 'tools',
      '/ai': 'ai', '/modules': 'modules',
    };
    return routingTable[path] || 'dashboard';
  });
  const [themeReady, setThemeReady] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');
  const [httpPort, setHttpPort] = useState<number | null>(null);

  // ── Refs ──
  const terminalSessionIdRef = useRef<string | null>(null);
  const iframeContainerRef = useRef<HTMLDivElement>(null);
  const activeModuleRef = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // ── File preview state ──
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);
  const [editMode, setEditMode] = useState(false);

  // ── Visual Config ──
  const [visualConfig, setVisualConfig] = useState<VisualConfig>({
    blurAmount: 0.40, displacementScale: 64, saturation: 135,
    aberrationIntensity: 2, elasticity: 0, cornerRadius: 28,
    showWallpaper: true, showBlobs: true,
  });

  // ── Follow mode ──
  const { mode: followMode, cycleMode: cycleFollowMode } = useFollowMode();

  // ── Crash state ──
  const [crashedModules, setCrashedModules] = useState<Set<string>>(new Set());
  const [iframeReloadKey, setIframeReloadKey] = useState(0);

  // ── Onboarding ──
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);

  // Check if username is set (first-run detection)
  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.db?.get) {
      startTransition(() => { setNeedsOnboarding(false); });
      return;
    }
    api.db.get('settings:username').then((value: unknown) => {
      setNeedsOnboarding(!value);
    }).catch(() => setNeedsOnboarding(false));
  }, []);

  // ── Screenshot/Release ──
  const [annotatingFile, setAnnotatingFile] = useState<string | null>(null);
  const [annotationImageUrl, setAnnotationImageUrl] = useState<string | null>(null);
  const [releaseWizardOpen, setReleaseWizardOpen] = useState(false);

  // ── Handlers ──
  const toggleSidebar = useCallback(() => {
    setState((prev) => ({ ...prev, sidebarCollapsed: !prev.sidebarCollapsed }));
  }, []);

  const toggleTerminal = useCallback(() => {
    setState((prev) => ({ ...prev, terminalCollapsed: !prev.terminalCollapsed }));
  }, []);

  const toggleMaximized = useCallback(() => {
    setState((prev) => ({ ...prev, terminalMaximized: !prev.terminalMaximized }));
  }, []);

  const toggleRightPanel = useCallback((mode: RightPanelMode) => {
    setState((prev) => ({
      ...prev,
      rightPanelMode: prev.rightPanelMode === mode ? 'closed' : mode,
    }));
  }, []);

  const setRightPanelMode = useCallback((mode: RightPanelMode) => {
    setState((prev) => ({ ...prev, rightPanelMode: mode }));
  }, []);

  const handleModuleSelect = useCallback((moduleId: string) => {
    setActiveView(moduleId.startsWith('module:') ? moduleId : `module:${moduleId}`);
    setState((prev) => ({ ...prev, rightPanelMode: 'closed' }));
  }, []);

  const handleFileSelect = useCallback((file: FileEntry) => {
    setSelectedFile(file);
    setState((prev) => ({
      ...prev,
      rightPanelMode: 'file-preview',
      previewSubMode: 'preview',
    }));
  }, []);

  const handleInstallModule = useCallback((moduleId: string) => {
    setActiveView(moduleId.startsWith('module:') ? moduleId : `module:${moduleId}`);
  }, []);

  return {
    state, setState, stateRef,
    activeView, setActiveView,
    themeReady, setThemeReady,
    locale, setLocale,
    httpPort, setHttpPort,
    terminalSessionIdRef,
    iframeContainerRef,
    activeModuleRef,
    contentRef,
    selectedFile, setSelectedFile,
    editMode, setEditMode,
    visualConfig, setVisualConfig,
    followMode, cycleFollowMode,
    crashedModules, setCrashedModules,
    iframeReloadKey, setIframeReloadKey,
    needsOnboarding, setNeedsOnboarding,
    annotatingFile, setAnnotatingFile,
    annotationImageUrl, setAnnotationImageUrl,
    releaseWizardOpen, setReleaseWizardOpen,
    toggleSidebar, toggleTerminal, toggleMaximized,
    toggleRightPanel, handleModuleSelect, handleFileSelect,
    handleInstallModule, setRightPanelMode,
  };
}
