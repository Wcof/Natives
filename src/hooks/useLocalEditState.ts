'use client';

import { useCallback, useState } from 'react';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary, LaunchPlan } from '@/lib/tauri-adapter';

export interface LocalEditCallbacks {
  onToast: (message: string) => void;
  onSaved: () => void;
}

export interface LocalEditState {
  app: CreativeAppSummary | null;
  title: string;
  setTitle: (v: string) => void;
  autoOpen: boolean;
  setAutoOpen: (v: boolean) => void;
  saving: boolean;
  loading: boolean;
  plan: LaunchPlan | null;
  envKeys: string[];
  portMode: 'auto' | 'fixed';
  setPortMode: (v: 'auto' | 'fixed') => void;
  portValue: string;
  setPortValue: (v: string) => void;
  script: string;
  setScript: (v: string) => void;
  openPath: string;
  setOpenPath: (v: string) => void;
  cwd: string;
  setCwd: (v: string) => void;
  open: (app: CreativeAppSummary) => Promise<void>;
  close: () => void;
  save: () => Promise<void>;
}

/**
 * Local app settings editor (T10): loads the Host launch config on open,
 * edits the user-overridable fields, and persists via updateLocal.
 */
export function useLocalEditState({ onToast, onSaved }: LocalEditCallbacks): LocalEditState {
  const [app, setApp] = useState<CreativeAppSummary | null>(null);
  const [title, setTitle] = useState('');
  const [autoOpen, setAutoOpen] = useState(true);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(false);
  const [plan, setPlan] = useState<LaunchPlan | null>(null);
  const [envKeys, setEnvKeys] = useState<string[]>([]);
  const [portMode, setPortMode] = useState<'auto' | 'fixed'>('auto');
  const [portValue, setPortValue] = useState('');
  const [script, setScript] = useState('');
  const [openPath, setOpenPath] = useState('/');
  const [cwd, setCwd] = useState('.');

  const open = useCallback(async (target: CreativeAppSummary) => {
    setApp(target);
    setTitle(target.title);
    setAutoOpen(Boolean(target.localProject?.autoOpen ?? true));
    setLoading(true);
    try {
      const cfg = await window.nativesAPI?.creativeApp?.getLocalConfig?.(target.id);
      if (cfg?.launchPlan) {
        setPlan(cfg.launchPlan);
        setPortMode(cfg.launchPlan.port.mode);
        setPortValue(cfg.launchPlan.port.value != null ? String(cfg.launchPlan.port.value) : '');
        setScript(cfg.launchPlan.script || '');
        setOpenPath(cfg.launchPlan.openPath || '/');
        setCwd(cfg.launchPlan.cwdRelative || '.');
        setAutoOpen(cfg.launchPlan.autoOpen);
      }
      setEnvKeys(cfg?.envKeys || []);
    } catch (err) {
      onToast(classifyError(err).userMessage);
    } finally {
      setLoading(false);
    }
  }, [onToast]);

  const close = useCallback(() => setApp(null), []);

  const save = useCallback(async () => {
    if (!app) return;
    setSaving(true);
    try {
      let nextPlan = plan;
      if (nextPlan) {
        nextPlan = {
          ...nextPlan,
          source: 'user',
          cwdRelative: cwd.trim() || '.',
          script: script.trim() || nextPlan.script,
          openPath: openPath.trim() || '/',
          healthPath: openPath.trim() || '/',
          autoOpen,
          port: {
            mode: portMode,
            value: portMode === 'fixed' && portValue ? Number(portValue) : undefined,
          },
        };
      }
      await window.nativesAPI?.creativeApp?.updateLocal?.({
        id: app.id,
        title: title.trim() || app.title,
        autoOpen,
        launchPlan: nextPlan || undefined,
        launchMode: nextPlan ? 'custom' : undefined,
      });
      setApp(null);
      onSaved();
    } catch (err) {
      onToast(classifyError(err).userMessage);
    } finally {
      setSaving(false);
    }
  }, [app, autoOpen, cwd, onSaved, onToast, openPath, plan, portMode, portValue, script, title]);

  return {
    app,
    title,
    setTitle,
    autoOpen,
    setAutoOpen,
    saving,
    loading,
    plan,
    envKeys,
    portMode,
    setPortMode,
    portValue,
    setPortValue,
    script,
    setScript,
    openPath,
    setOpenPath,
    cwd,
    setCwd,
    open,
    close,
    save,
  };
}
