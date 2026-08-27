'use client';

import React, { useState } from 'react';
import {
  Laptop,
  Globe,
  Search,
  BookmarkCheck,
  Plus,
  RefreshCw,
  Layers,
} from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { AppView, AppKind, AppRuntimeState, SystemRunningState } from '@/lib/tauri/apps';

interface AppListProps {
  apps: AppView[];
  selectedId: string | null;
  loading: boolean;
  systemStates: Record<string, SystemRunningState>;
  onSelect: (app: AppView) => void;
  onAdd: () => void;
  onRefresh: () => void;
}

export function AppList({
  apps,
  selectedId,
  loading,
  systemStates,
  onSelect,
  onAdd,
  onRefresh,
}: AppListProps) {
  const locale = useLocale();
  const [filterKind, setFilterKind] = useState<Exclude<AppKind, 'local_project'> | 'all'>('all');
  const [filterStatus, setFilterStatus] = useState<'all' | 'running' | 'stopped' | 'attention'>('all');
  const [searchQuery, setSearchQuery] = useState('');

  const filteredApps = apps.filter((app) => {
    if (filterKind !== 'all' && app.kind !== filterKind) return false;
    if (app.kind === 'web_application') {
      if (filterStatus !== 'all') return false;
      if (!searchQuery.trim()) return true;
      const q = searchQuery.toLowerCase();
      return app.title.toLowerCase().includes(q) || Boolean(app.description?.toLowerCase().includes(q));
    }
    const systemState = systemStates[app.appId];
    const status = systemState
      ? !systemState.installed || systemState.unobservable
        ? 'attention'
        : systemState.running
          ? 'running'
          : 'stopped'
      : app.runtimeState === 'failed' || app.runtimeState === 'orphaned'
        ? 'attention'
        : app.runtimeState === 'running'
          ? 'running'
          : 'stopped';
    if (filterStatus !== 'all' && status !== filterStatus) return false;
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      const matchTitle = app.title.toLowerCase().includes(q);
      const matchDesc = app.description?.toLowerCase().includes(q);
      return matchTitle || matchDesc;
    }
    return true;
  });

  const getKindIcon = (kind: AppKind) => {
    switch (kind) {
      case 'system_application':
        return <Laptop className="h-4 w-4 text-[var(--accent)]" />;
      case 'web_application':
        return <Globe className="h-4 w-4 text-[var(--primary)]" />;
    }
  };

  const getSystemStatusBadge = (state: SystemRunningState) => {
    if (!state.installed) return getStatusBadge('failed');
    if (state.unobservable) return <span className="text-[10px] text-[var(--warning)]">{t(locale, 'appsPage.stateUnobservable')}</span>;
    if (state.active) return <span className="text-[10px] text-[var(--success)]">{t(locale, 'appsPage.stateActive')}</span>;
    if (state.hidden) return <span className="text-[10px] text-[var(--text-tertiary)]">{t(locale, 'appsPage.stateHidden')}</span>;
    return getStatusBadge(state.running ? 'running' : 'stopped');
  };

  const getStatusBadge = (state: AppRuntimeState) => {
    switch (state) {
      case 'running':
        return (
          <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--success-soft)] text-[var(--success)] border border-[var(--success)]/20">
            <span className="h-1.5 w-1.5 rounded-full bg-[var(--success)] animate-pulse" />
            {t(locale, 'appsPage.stateRunning')}
          </span>
        );
      case 'starting':
        return (
          <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--warning-soft)] text-[var(--warning)] border border-[var(--warning)]/20">
            <span className="h-1.5 w-1.5 rounded-full bg-[var(--warning)] animate-spin" />
            {t(locale, 'appsPage.stateStarting')}
          </span>
        );
      case 'stopping':
        return (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--danger-soft)] text-[var(--danger)] border border-[var(--danger)]/20">
            {t(locale, 'appsPage.stateStopping')}
          </span>
        );
      case 'hibernated':
        return (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--primary-soft)] text-[var(--text-secondary)] border border-[var(--border-subtle)]">
            {t(locale, 'appsPage.stateHibernated')}
          </span>
        );
      case 'orphaned':
        return (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--warning-soft)] text-[var(--warning)] border border-[var(--warning)]/20">
            {t(locale, 'appsPage.stateOrphaned')}
          </span>
        );
      case 'failed':
        return (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--danger-soft)] text-[var(--danger)] border border-[var(--danger)]/20">
            {t(locale, 'appsPage.stateFailed')}
          </span>
        );
      default:
        return (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--surface-hover)] text-[var(--text-tertiary)] border border-[var(--border-subtle)]">
            {t(locale, 'appsPage.stateStopped')}
          </span>
        );
    }
  };

  return (
    <div className="flex flex-col h-full border-r border-[var(--border-subtle)] bg-[var(--surface)] w-80 shrink-0 select-none">
      {/* Header */}
      <div className="p-4 border-b border-[var(--border-subtle)] space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Layers className="h-5 w-5 text-[var(--primary)]" />
            <h1 className="text-sm font-bold text-[var(--text)]">
              {t(locale, 'appsPage.title')}
            </h1>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              title={t(locale, 'appsPage.refresh')}
              className="p-1.5 rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-colors"
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button
              type="button"
              onClick={onAdd}
              className="flex items-center gap-1 px-2.5 py-1.5 rounded-xl bg-[var(--primary)] text-[var(--primary-foreground)] text-xs font-medium hover:bg-[var(--primary-hover)] transition-colors shadow-sm"
            >
              <Plus className="h-3.5 w-3.5" />
              {t(locale, 'appsPage.addApp')}
            </button>
          </div>
        </div>

        {/* Search */}
        <div className="relative">
          <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-[var(--text-tertiary)]" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t(locale, 'appsPage.searchPlaceholder')}
            className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] pl-8 pr-3 py-1.5 text-xs text-[var(--text)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--primary)] focus:outline-none"
          />
        </div>

        {/* Filter Pills */}
        <div className="flex rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)] p-0.5 text-[11px]">
          <button
            type="button"
            onClick={() => setFilterKind('all')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'all'
                ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
            }`}
          >
            {t(locale, 'appsPage.filterAll')}
          </button>
          <button
            type="button"
            onClick={() => setFilterKind('system_application')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'system_application'
                ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
            }`}
          >
            {t(locale, 'appsPage.filterMac')}
          </button>
          <button
            type="button"
            onClick={() => setFilterKind('web_application')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'web_application'
                ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
            }`}
          >
            {t(locale, 'appsPage.filterWeb')}
          </button>
        </div>
        <select value={filterStatus} onChange={(event) => setFilterStatus(event.target.value as typeof filterStatus)} className="w-full rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)] px-2 py-1 text-[11px] text-[var(--text-secondary)]">
          <option value="all">{t(locale, 'appsPage.filterStatusAll')}</option>
          <option value="running">{t(locale, 'appsPage.filterStatusRunning')}</option>
          <option value="stopped">{t(locale, 'appsPage.filterStatusStopped')}</option>
          <option value="attention">{t(locale, 'appsPage.filterStatusAttention')}</option>
        </select>
      </div>

      {/* List Content */}
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {filteredApps.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-8 text-center space-y-2">
            <Layers className="h-8 w-8 text-[var(--text-tertiary)] stroke-1" />
            <p className="text-xs font-medium text-[var(--text-secondary)]">
              {t(locale, 'appsPage.noApps')}
            </p>
            <p className="text-[11px] text-[var(--text-tertiary)] max-w-[200px]">
              {t(locale, 'appsPage.noAppsHint')}
            </p>
          </div>
        ) : (
          filteredApps.map((app) => (
            <button
              key={app.appId}
              type="button"
              onClick={() => onSelect(app)}
              className={`w-full flex items-start gap-3 p-3 rounded-xl text-left transition-all ${
                selectedId === app.appId
                  ? 'bg-[var(--surface-hover)] border border-[var(--primary)]/40 shadow-sm'
                  : 'hover:bg-[var(--surface-hover)]/60 border border-transparent'
              }`}
            >
              <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--surface-hover)] border border-[var(--border-subtle)]">
                {getKindIcon(app.kind)}
              </div>
              <div className="flex-1 min-w-0 space-y-1">
                <div className="flex items-center justify-between gap-1">
                  <span className="text-xs font-semibold text-[var(--text)] truncate">
                    {app.title}
                  </span>
                  {app.showInSidebar && (
                    <BookmarkCheck className="h-3 w-3 shrink-0 text-[var(--primary)]" />
                  )}
                </div>
                {app.description && (
                  <p className="text-[11px] text-[var(--text-tertiary)] truncate">
                    {app.description}
                  </p>
                )}
                <div className="flex items-center justify-between gap-2 pt-0.5">
                  {app.kind === 'web_application'
                    ? <span className="text-[10px] text-[var(--text-tertiary)]">{t(locale, 'appsPage.webSaved')}</span>
                    : app.kind === 'system_application' && systemStates[app.appId]
                    ? getSystemStatusBadge(systemStates[app.appId]!)
                    : getStatusBadge(app.runtimeState)}
                  {app.updatedAt && <time className="text-[9px] text-[var(--text-disabled)]">{new Date(app.updatedAt).toLocaleDateString(locale)}</time>}
                </div>
              </div>
            </button>
          ))
        )}
      </div>
    </div>
  );
}
