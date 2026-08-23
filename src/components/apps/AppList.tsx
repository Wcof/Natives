'use client';

import React, { useState } from 'react';
import {
  Folder,
  Laptop,
  Globe,
  Search,
  BookmarkCheck,
  Plus,
  RefreshCw,
  Layers,
} from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { AppView, AppKind, AppRuntimeState } from '@/lib/tauri/apps';

interface AppListProps {
  apps: AppView[];
  selectedId: string | null;
  loading: boolean;
  onSelect: (app: AppView) => void;
  onAdd: () => void;
  onRefresh: () => void;
}

export function AppList({
  apps,
  selectedId,
  loading,
  onSelect,
  onAdd,
  onRefresh,
}: AppListProps) {
  const locale = useLocale();
  const [filterKind, setFilterKind] = useState<AppKind | 'all'>('all');
  const [searchQuery, setSearchQuery] = useState('');

  const filteredApps = apps.filter((app) => {
    if (filterKind !== 'all' && app.kind !== filterKind) return false;
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
      case 'local_project':
        return <Folder className="h-4 w-4 text-[var(--success)]" />;
      case 'system_application':
        return <Laptop className="h-4 w-4 text-[var(--interactive-accent)]" />;
      case 'web_application':
        return <Globe className="h-4 w-4 text-[var(--primary)]" />;
    }
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
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--primary-soft)] text-[var(--interactive-accent)] border border-[var(--interactive-accent)]/20">
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
          <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-[var(--surface-muted)] text-[var(--text-tertiary)] border border-[var(--border-subtle)]">
            {t(locale, 'appsPage.stateStopped')}
          </span>
        );
    }
  };

  return (
    <div className="flex flex-col h-full border-r border-[var(--border-default)] bg-[var(--surface-base)] w-80 shrink-0 select-none">
      {/* Header */}
      <div className="p-4 border-b border-[var(--border-subtle)] space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Layers className="h-5 w-5 text-[var(--interactive-accent)]" />
            <h1 className="text-sm font-bold text-[var(--text-primary)]">
              {t(locale, 'appsPage.title')}
            </h1>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              title={t(locale, 'appsPage.refresh')}
              className="p-1.5 rounded-lg text-[var(--text-secondary)] hover:bg-[var(--surface-overlay)] hover:text-[var(--text-primary)] transition-colors"
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button
              type="button"
              onClick={onAdd}
              className="flex items-center gap-1 px-2.5 py-1.5 rounded-xl bg-[var(--interactive-accent)] text-[var(--text-on-accent)] text-xs font-medium hover:opacity-90 transition-opacity shadow-sm"
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
            placeholder="Search registered apps..."
            className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] pl-8 pr-3 py-1.5 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          />
        </div>

        {/* Filter Pills */}
        <div className="flex rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-muted)] p-0.5 text-[11px]">
          <button
            type="button"
            onClick={() => setFilterKind('all')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'all'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            All
          </button>
          <button
            type="button"
            onClick={() => setFilterKind('local_project')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'local_project'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            Local
          </button>
          <button
            type="button"
            onClick={() => setFilterKind('system_application')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'system_application'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            System
          </button>
          <button
            type="button"
            onClick={() => setFilterKind('web_application')}
            className={`flex-1 py-1 rounded-md font-medium transition-all ${
              filterKind === 'web_application'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            Web
          </button>
        </div>
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
                  ? 'bg-[var(--surface-overlay)] border border-[var(--interactive-accent)]/40 shadow-sm'
                  : 'hover:bg-[var(--surface-overlay)]/60 border border-transparent'
              }`}
            >
              <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--surface-muted)] border border-[var(--border-subtle)]">
                {getKindIcon(app.kind)}
              </div>
              <div className="flex-1 min-w-0 space-y-1">
                <div className="flex items-center justify-between gap-1">
                  <span className="text-xs font-semibold text-[var(--text-primary)] truncate">
                    {app.title}
                  </span>
                  {app.showInSidebar && (
                    <BookmarkCheck className="h-3 w-3 shrink-0 text-[var(--interactive-accent)]" />
                  )}
                </div>
                {app.description && (
                  <p className="text-[11px] text-[var(--text-tertiary)] truncate">
                    {app.description}
                  </p>
                )}
                <div className="pt-0.5">{getStatusBadge(app.runtimeState)}</div>
              </div>
            </button>
          ))
        )}
      </div>
    </div>
  );
}
