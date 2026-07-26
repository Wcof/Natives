'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { RefreshCw, Upload, Search } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { listCapabilitySkills, rescanCapabilitySkills } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import CategoryFilterBar, { matchesCategory } from '../shared/CategoryFilterBar';
import type { CapabilitySkill, CategoryFilterValue } from '../shared/capability-types';
import SkillList from './SkillList';
import SkillDetail from './SkillDetail';
import SkillImportDialog from './SkillImportDialog';

interface SkillsTabProps {
  locale: Locale;
  gateway: AssistantGateway;
}

export default function SkillsTab({ locale, gateway }: SkillsTabProps) {
  const { toast } = useToast();
  const [skills, setSkills] = useState<CapabilitySkill[]>([]);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const [category, setCategory] = useState<CategoryFilterValue>('all');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [rescanning, setRescanning] = useState(false);
  // 竞态保护：慢响应不得覆盖新请求的结果（对齐同页 McpHubBrowser 模式）
  const requestSeq = useRef(0);

  // 300ms 防抖：打字不整表重载，也不闪 loading
  useEffect(() => {
    const timer = setTimeout(() => {
      setSearching(true);
      setDebouncedQuery(query);
    }, 300);
    return () => clearTimeout(timer);
  }, [query]);

  // 重载期间保留旧列表：仅首屏（phase 初值 loading）显示全屏 LoadingState
  const load = useCallback(async () => {
    const seq = ++requestSeq.current;
    try {
      const q = debouncedQuery.trim();
      const list = await listCapabilitySkills(gateway, q ? { query: q } : {});
      if (seq !== requestSeq.current) return;
      setSkills(list);
      setError(null);
      setPhase('ready');
    } catch (e) {
      if (seq !== requestSeq.current) return;
      setError(classifyError(e).userMessage);
      setPhase('error');
    } finally {
      if (seq === requestSeq.current) setSearching(false);
    }
  }, [gateway, debouncedQuery]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleRescan = useCallback(async () => {
    setRescanning(true);
    try {
      const result = await rescanCapabilitySkills(gateway);
      toast(
        t(locale, 'capabilities.skills.rescanResult', {
          scanned: result.scanned,
          new: result.new,
          changed: result.changed,
          missing: result.missing.length,
        }),
        'success',
      );
      await load();
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setRescanning(false);
    }
  }, [gateway, load, locale, toast]);

  const visible = skills.filter((skill) => matchesCategory(skill.category, category));
  const selected = selectedId ? skills.find((skill) => skill.id === selectedId) ?? null : null;

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-2">
        <div
          className="flex min-w-48 flex-1 items-center gap-1.5 rounded-lg border px-2.5 py-1.5"
          style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
        >
          <Search size={14} style={{ color: 'var(--text-disabled)' }} aria-hidden />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t(locale, 'capabilities.skills.searchPlaceholder')}
            aria-label={t(locale, 'capabilities.skills.searchPlaceholder')}
            className="w-full bg-transparent text-sm"
            style={{ color: 'var(--text)', outline: 'none', border: 'none' }}
          />
          {searching && phase === 'ready' ? (
            <RefreshCw size={13} className="animate-spin" style={{ color: 'var(--text-disabled)', flexShrink: 0 }} aria-hidden />
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => void handleRescan()}
          disabled={rescanning}
          className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm disabled:opacity-50"
          style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
        >
          <RefreshCw size={14} className={rescanning ? 'animate-spin' : undefined} aria-hidden />
          {t(locale, 'capabilities.skills.rescan')}
        </button>
        <button
          type="button"
          onClick={() => setImportOpen(true)}
          className="btn btn-primary flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm"
        >
          <Upload size={14} aria-hidden />
          {t(locale, 'capabilities.skills.import')}
        </button>
      </div>
      <CategoryFilterBar locale={locale} value={category} onChange={setCategory} />

      {/* Three honest states (R-E10) + list */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {phase === 'loading' ? (
          <LoadingState message={t(locale, 'capabilities.common.loading')} />
        ) : phase === 'error' ? (
          <ErrorState message={error ?? t(locale, 'capabilities.skills.loadFailed')} onRetry={() => void load()} />
        ) : visible.length === 0 ? (
          <EmptyState
            title={t(locale, 'capabilities.skills.empty')}
            description={t(locale, 'capabilities.skills.emptyDesc')}
            action={{ label: t(locale, 'capabilities.skills.import'), onClick: () => setImportOpen(true) }}
          />
        ) : (
          <SkillList locale={locale} skills={visible} selectedId={selectedId} onSelect={setSelectedId} />
        )}
      </div>

      {selected && (
        <SkillDetail
          locale={locale}
          gateway={gateway}
          skill={selected}
          onClose={() => setSelectedId(null)}
          onChanged={() => void load()}
          onDeleted={() => {
            setSelectedId(null);
            void load();
          }}
        />
      )}
      <SkillImportDialog
        locale={locale}
        gateway={gateway}
        open={importOpen}
        onClose={() => setImportOpen(false)}
        onImported={() => {
          setImportOpen(false);
          void load();
        }}
      />
    </div>
  );
}
