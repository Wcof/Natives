'use client';

import React, { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { fmtCount, fmtSize } from '@/lib/format';
import type { SkillInfo, ClaudeUsage, ModelTokenUsage, CodexUsage, RtkUsage } from '@/types/agent';
import { TokenHero } from '@/components/dashboard/TokenHero';
import { SkillsPanel } from '@/components/dashboard/SkillsPanel';
import { ModelStatsTable, type ModelStat } from '@/components/dashboard/ModelStatsTable';
import { TokenTrendChart, type TokenHistoryPoint } from '@/components/dashboard/TokenTrendChart';
import KanbanCard from '@/components/dashboard/KanbanCard';
import CodeGraphPanel from '@/components/dashboard/CodeGraphPanel';
import { useLocale, t } from '@/i18n';
import '@/types';
import {
  Grid3x3, HardDrive, Activity, RefreshCw,
  Sparkles, Wrench, Code2, GitBranch,
  Coins, BarChart3, Database, Layers, Zap,
  Inbox, CircleAlert,
} from 'lucide-react';

// ── Session cache (cold-data strategy) ──

interface CacheEntry<T> { data: T; fetchedAt: number; }
const CACHE_TTL = 3_600_000; // 1 hour — only refresh after stale or manual trigger
const sessionCache = new Map<string, CacheEntry<any>>();

function getCached<T>(key: string): T | null {
  const entry = sessionCache.get(key);
  if (!entry) return null;
  return entry.data as T;
}
function setCache<T>(key: string, data: T): void {
  sessionCache.set(key, { data, fetchedAt: Date.now() });
}
function isCacheValid(key: string): boolean {
  const entry = sessionCache.get(key);
  if (!entry) return false;
  return Date.now() - entry.fetchedAt < CACHE_TTL;
}
function invalidateCache(): void { sessionCache.clear(); }

// ── Batched async loader ──
// Runs tasks in batches with delays to avoid blocking the UI
function delay(ms: number): Promise<void> { return new Promise(r => setTimeout(r, ms)); }
async function runBatched(batches: Array<() => Promise<void>>, gapMs = 1500): Promise<void> {
  for (let i = 0; i < batches.length; i++) {
    if (i > 0) await delay(gapMs);
    const batch = batches[i];
    if (batch) { try { await batch(); } catch { /* batch failed, continue */ } }
  }
}

// ── Types ──

interface UsageResult {
  claude: ClaudeUsage | null;
  codex: CodexUsage | null;
  rtk: RtkUsage | null;
  modelStats: ModelStat[];
  history: TokenHistoryPoint[];
  sourceConfigured?: boolean;
  sourceBreadcrumbs?: string[];
}

interface RtkGainResult {
  totalSaved: number;
  totalCommands: number;
  commands: { command: string; count: number; tokensSaved: number }[];
}

// ── Component ──

export default function DashboardPage() {
  const locale = useLocale();
  const [loc, setLoc] = useState(locale);

  const [skills, setSkills] = useState<SkillInfo[]>(() => getCached<SkillInfo[]>('skills') ?? []);
  const [usageResult, setUsageResult] = useState<UsageResult | null>(() => getCached<UsageResult>('usage'));
  const [rtkData, setRtkData] = useState<RtkGainResult | null>(null);
  const [codexUsage, setCodexUsage] = useState<CodexUsage | null>(null);
  const [moduleCount, setModuleCount] = useState(() => getCached<number>('moduleCount') ?? 0);

  const [isSkillsLoading, setIsSkillsLoading] = useState(() => !isCacheValid('skills'));
  const [isUsageLoading, setIsUsageLoading] = useState(() => !isCacheValid('usage'));
  const [isModulesLoading, setIsModulesLoading] = useState(() => !isCacheValid('moduleCount'));
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [codeGraphExpanded, setCodeGraphExpanded] = useState(false);

  // Accordion mode states
  const [expandedCards, setExpandedCards] = useState<Record<string, boolean>>({
    tokens: true, // expand tokens by default
    tools: false,
    savings: false,
    trend: false,
    models: false,
  });
  const [accordionMode, setAccordionMode] = useState(true);

  // Storage calculation states
  const [storageSize, setStorageSize] = useState<string>('…');
  const [storageDetail, setStorageDetail] = useState<{ used: string; total: string; percent: number } | null>(null);
  const [isStorageLoading, setIsStorageLoading] = useState(true);

  const handleToggleCard = useCallback((cardId: string) => {
    setExpandedCards(prev => {
      const isCurrentlyExpanded = prev[cardId];
      if (accordionMode) {
        if (!isCurrentlyExpanded) {
          return {
            tokens: cardId === 'tokens',
            tools: cardId === 'tools',
            savings: cardId === 'savings',
            trend: cardId === 'trend',
            models: cardId === 'models',
          };
        } else {
          return {
            tokens: false,
            tools: false,
            savings: false,
            trend: false,
            models: false,
          };
        }
      } else {
        return {
          ...prev,
          [cardId]: !isCurrentlyExpanded,
        };
      }
    });
  }, [accordionMode]);

  const mountRef = useRef(false);
  const loadGenRef = useRef(0);

  // ── Derived summaries ──

  const totalTokens = usageResult?.claude?.localTokens?.total ?? 0;
  const totalRequests = usageResult?.claude?.totalRequests ?? 0;
  const totalCost = usageResult?.claude?.totalCost;
  const modelCount = usageResult?.claude?.models ? Object.keys(usageResult.claude.models).length : 0;
  const skillCount = skills?.length ?? 0;
  const enabledSkillCount = skills.filter(s => s.enabled).length;

  const rtkSaved = rtkData?.totalSaved ?? 0;
  const rtkCmds = rtkData?.totalCommands ?? 0;

  // Top model by requests
  const topModel = useMemo(() => {
    if (!usageResult?.modelStats || usageResult.modelStats.length === 0) return null;
    return [...usageResult.modelStats].sort((a, b) => b.requestCount - a.requestCount)[0];
  }, [usageResult?.modelStats]);

  // ── Locale ──
  useEffect(() => {
    async function loadLocale() {
      try { const s = await window.nativesAPI?.getLocale?.(); if (s) setLoc(s === 'en' ? 'en' : 'zh'); } catch { /* no-op */ }
    }
    loadLocale();
  }, []);

  useEffect(() => {
    const h = (e: Event) => {
      const d = (e as CustomEvent).detail;
      if (d === 'en') setLoc('en');
      else if (d && d.startsWith('zh')) setLoc('zh');
    };
    window.addEventListener('locale-changed', h);
    return () => window.removeEventListener('locale-changed', h);
  }, []);

  // ── Data loading (cold-data + batched) ──

  const loadData = useCallback((forceRefresh = false): Promise<void> | void => {
    const gen = ++loadGenRef.current;
    const stale = (k: string) => forceRefresh || !isCacheValid(k);

    // If nothing is stale and not forced, skip entirely
    if (!forceRefresh && isCacheValid('moduleCount') && isCacheValid('skills') && isCacheValid('usage') && isCacheValid('rtkGain')) {
      return;
    }

    // Batch 1: lightweight data (module count + skills)
    const batch1 = async () => {
      if (!stale('moduleCount') && !stale('skills')) return;
      if (stale('moduleCount')) setIsModulesLoading(true);
      if (stale('skills')) setIsSkillsLoading(true);

      const tasks: Promise<void>[] = [];
      if (stale('moduleCount')) {
        const p = window.nativesAPI?.module.list();
        if (p) {
          tasks.push(
            p.then((mods: any) => { if (gen !== loadGenRef.current) return; const c = Array.isArray(mods) ? mods.length : 0; setModuleCount(c); setCache('moduleCount', c); })
              .catch(() => { if (gen !== loadGenRef.current) return; setModuleCount(0); })
              .finally(() => setIsModulesLoading(false))
          );
        } else { setIsModulesLoading(false); }
      }
      if (stale('skills')) {
        const p = window.nativesAPI?.agent.scanSkills();
        if (p) {
          tasks.push(
            p.then((s: any) => { if (gen !== loadGenRef.current) return; const list = (Array.isArray(s) ? s : s?.items ?? []) as SkillInfo[]; setSkills(list); setCache('skills', list); })
              .catch(() => { if (gen !== loadGenRef.current) return; setSkills([]); })
              .finally(() => setIsSkillsLoading(false))
          );
        } else { setIsSkillsLoading(false); }
      }
      await Promise.allSettled(tasks);
    };

    // Batch 2: usage data (heavier)
    const batch2 = async () => {
      if (!stale('usage')) return;
      setIsUsageLoading(true);
      try {
        const r = await window.nativesAPI?.usage.refresh();
        if (gen !== loadGenRef.current) return;
        const result: UsageResult = {
          claude: r?.claude ?? null,
          codex: r?.codex ?? null,
          rtk: r?.rtk ?? null,
          modelStats: Array.isArray(r?.modelStats) ? r.modelStats.map((m: any) => ({
            model: m.model ?? 'unknown', requestCount: m.requestCount ?? 0,
            totalTokens: m.totalTokens ?? 0, totalCost: m.totalCost ?? 0,
            avgCostPerRequest: m.avgCostPerRequest ?? 0,
          })) : [],
          history: Array.isArray(r?.history) ? r.history.map((h: any) => ({
            date: (() => { try { return new Date(h.timestamp).toISOString(); } catch { return new Date().toISOString(); } })(),
            input: h.inputTokens ?? 0, output: h.outputTokens ?? 0,
            cacheWrite: h.cacheCreationTokens ?? 0, cacheRead: h.cacheReadTokens ?? 0,
            skills: h.skills ?? 0,
          })) : [],
          sourceConfigured: r?.sourceConfigured === true,
          sourceBreadcrumbs: Array.isArray(r?.sourceBreadcrumbs) ? r.sourceBreadcrumbs : [],
        };
        setUsageResult(result);
        setCodexUsage(result.codex);
        setCache('usage', result);
      } catch { if (gen !== loadGenRef.current) return; setUsageResult(null); }
      finally { setIsUsageLoading(false); }
    };

    // Batch 3: RTK gain (heaviest, external process)
    const batch3 = async () => {
      if (!stale('rtkGain')) return;
      try {
        const r = await (window as any).nativesAPI?.codegraph?.rtkGain?.();
        if (gen !== loadGenRef.current) return;
        if (r) { setRtkData(r as RtkGainResult); setCache('rtkGain', r); }
      } catch { /* rtk not installed */ }
    };

    // Run batches with 1.5s gaps
    return runBatched([batch1, batch2, batch3], 1500);
  }, []);

  // Load system disk info for storage StatCard (cold cache)
  useEffect(() => {
    if (isCacheValid('storage')) {
      const cached = getCached<{ size: string; detail: typeof storageDetail }>('storage');
    // eslint-disable-next-line react-hooks/set-state-in-effect
      if (cached) { setStorageSize(cached.size); setStorageDetail(cached.detail); setIsStorageLoading(false); return; }
    }
    async function getStorage() {
      setIsStorageLoading(true);
      try {
        const api = window.nativesAPI;
        if (api?.disk?.systemInfo) {
          const info = await api.disk.systemInfo() as { total_bytes: number; used_bytes: number; available_bytes: number };
          if (info && info.total_bytes > 0) {
            const usedGB = (info.used_bytes / (1024 ** 3)).toFixed(1);
            const totalGB = (info.total_bytes / (1024 ** 3)).toFixed(0);
            const percent = Math.round((info.used_bytes / info.total_bytes) * 100);
            const size = `${usedGB} / ${totalGB} GB`;
            const detail = { used: `${usedGB} GB`, total: `${totalGB} GB`, percent };
            setStorageSize(size);
            setStorageDetail(detail);
            setCache('storage', { size, detail });
            setIsStorageLoading(false);
            return;
          }
        }
      } catch (err) {
        console.error('Failed to get storage info:', err);
      }
      setStorageSize('—');
      setStorageDetail(null);
      setIsStorageLoading(false);
    }
    getStorage();
  }, []);

  useEffect(() => {
    if (mountRef.current) return;
    mountRef.current = true;
    loadData(false);
  }, [loadData]);

  const handleRefresh = useCallback(async () => {
    invalidateCache();
    setIsRefreshing(true);
    setIsSkillsLoading(true); setIsUsageLoading(true); setIsModulesLoading(true);
    try { await loadData(true); } catch { /* ignore */ }
    setIsRefreshing(false);
  }, [loadData]);

  // ── Model summary lines for usage card ──
  const modelSummaryLines = useMemo(() => {
    const models = usageResult?.claude?.models;
    if (!models) return [];
    return Object.entries(models)
      .sort(([, a], [, b]) => (b.inputTokens + b.outputTokens) - (a.inputTokens + a.outputTokens))
      .slice(0, 5)
      .map(([name, usage]) => ({
        id: name,
        name: name.split('/').pop() ?? name,
        tokens: usage.inputTokens + usage.outputTokens + (usage.cacheReadInputTokens ?? 0) + (usage.cacheCreationInputTokens ?? 0),
        cost: usage.costUSD,
      }));
  }, [usageResult?.claude?.models]);

    // eslint-disable-next-line react-hooks/preserve-manual-memoization
  const maxRtkSaved = useMemo(() => {
    if (!rtkData?.commands || rtkData.commands.length === 0) return 1;
    return Math.max(...rtkData.commands.map(c => c.tokensSaved), 1);
  }, [rtkData?.commands]);

    // eslint-disable-next-line react-hooks/preserve-manual-memoization
  const usageHistory = useMemo(() => {
    if (!usageResult?.history) return [];
    return usageResult.history;
  }, [usageResult?.history]);

  const skillsOverview = useMemo(() => {
    const list = skills || [];
    const nonResidue = list.filter(i => !i.residue);
    const enabled = nonResidue.filter(i => i.enabled);
    const uniqueNames = new Set(nonResidue.map(i => i.name));

    let budgetChars = 0;
    for (const item of list) {
      if (item.enabled && !item.residue && (item.source === '~/.claude/skills' || item.source === 'project/.claude/skills' || item.source === 'claude-plugins')) {
        budgetChars += item.description?.length ?? 0;
      }
    }

    const totalHits = list.reduce((acc, s) => acc + (s.triggerCount ?? s.hitCount ?? 0), 0);
    const activeCount = enabled.filter(i => (i.triggerCount ?? i.hitCount ?? 0) > 0).length;
    const dustCount = enabled.filter(i => (i.triggerCount ?? i.hitCount ?? 0) === 0).length;
    const issueCount = list.filter(i => i.health && !i.health.ok).length;

    return {
      total: nonResidue.length,
      unique: uniqueNames.size,
      active: activeCount,
      dust: dustCount,
      issues: issueCount,
      totalHits,
      budgetChars,
      budgetLimit: 15000,
    };
  }, [skills]);

  return (
    <div className="w-full h-full flex flex-col overflow-y-auto">
      {/* ── Greeting ── */}
      <div style={{ padding: `${SPACING.lg}px ${SPACING.xl}px ${SPACING.sm}px`, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <div>
          <h1 style={{ fontSize: '22px', fontWeight: 700, color: 'var(--vibe-brand-text)', letterSpacing: '-0.02em' }}>
            {t(loc, 'dashboard.greeting')}
          </h1>
          <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)', marginTop: 1 }}>
            {t(loc, 'dashboard.subtitle')}
          </p>
        </div>
        
        <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, flexShrink: 0 }}>
          {/* Accordion Mode Switch */}
          <button
            onClick={() => setAccordionMode(!accordionMode)}
            title={accordionMode ? t(loc, 'dashboard.accordionOn') : t(loc, 'dashboard.accordionOff')}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '5px 10px',
              borderRadius: BORDER_RADIUS.md,
              background: accordionMode ? 'var(--vibe-active-bg)' : 'var(--vibe-btn-bg)',
              border: `0.0625rem solid ${accordionMode ? 'var(--vibe-active-color)' : 'var(--vibe-btn-border)'}`,
              color: accordionMode ? 'var(--vibe-active-color)' : 'var(--vibe-brand-text)',
              fontSize: FONT_SIZE.xs,
              cursor: 'pointer',
              transition: 'all 0.2s ease',
            }}
          >
            <Activity size={12} />
            {accordionMode ? t(loc, 'dashboard.accordionOn') || '聚焦手风琴：开' : t(loc, 'dashboard.accordionOff') || '聚焦手风琴：关'}
          </button>

          {/* Refresh button */}
          <button onClick={handleRefresh} disabled={isRefreshing} title={t(loc, 'dashboard.refresh')}
            style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '5px 10px', borderRadius: BORDER_RADIUS.md, background: 'var(--vibe-btn-bg)', border: '0.0625rem solid var(--vibe-btn-border)', color: 'var(--vibe-brand-text)', fontSize: FONT_SIZE.xs, cursor: isRefreshing ? 'default' : 'pointer', opacity: isRefreshing ? 0.6 : 1, transition: 'opacity 0.15s' }}>
            <RefreshCw size={12} style={{ animation: isRefreshing ? 'spin 0.8s linear infinite' : undefined }} />
            {isRefreshing ? t(loc, 'dashboard.refreshing') : t(loc, 'dashboard.refresh')}
          </button>
        </div>
      </div>

      {/* ── Top stat cards ── */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.sm}px`, flexShrink: 0 }}>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr) 1.6fr', gap: SPACING.sm }}>
          <StatCard
            icon={<Grid3x3 size={16} />}
            label="全部 Skills"
            value={isSkillsLoading ? '…' : `${skillsOverview.unique}/${skillsOverview.total}`}
            subtext={`${moduleCount} 模块 · ${isStorageLoading ? '…' : storageSize}`}
            hoverColor="#3b82f6"
            title="全部技能及模块分布"
            onClick={() => window.dispatchEvent(new CustomEvent('navigate', { detail: 'modules' }))}
          />
          <StatCard
            icon={<Zap size={16} />}
            label="活跃技能"
            value={isSkillsLoading ? '…' : String(skillsOverview.active)}
            subtext={`共 ${skillsOverview.totalHits} 次触发`}
            hoverColor="#10b981"
            title="45天内被触发的技能数量"
          />
          <StatCard
            icon={<Inbox size={16} />}
            label="吃灰中"
            value={isSkillsLoading ? '…' : String(skillsOverview.dust)}
            subtext="45天零触发"
            hoverColor="var(--text-faint)"
            title="已启用但近期未触发的技能"
          />
          <StatCard
            icon={<CircleAlert size={16} />}
            label="异常 / 问题"
            value={isSkillsLoading ? '…' : String(skillsOverview.issues)}
            subtext="健康度异常"
            hoverColor={skillsOverview.issues > 0 ? 'var(--danger)' : 'var(--text-faint)'}
            valueColor={skillsOverview.issues > 0 ? 'var(--danger)' : undefined}
            title="frontmatter 缺失/描述截断/残留文件夹等异常"
          />
          <StatCard
            icon={<Layers size={16} />}
            label="Claude 常驻预算"
            value={`${(skillsOverview.budgetChars / 1000).toFixed(1)}k / ${(skillsOverview.budgetLimit / 1000).toFixed(0)}k`}
            hoverColor={skillsOverview.budgetChars > skillsOverview.budgetLimit ? 'var(--danger)' : '#a855f7'}
            title="Claude 常驻描述字符量占用情况"
          >
            <div style={{ marginTop: 6 }}>
              <div style={{
                height: 6,
                borderRadius: 3,
                background: 'var(--vibe-btn-bg)',
                overflow: 'hidden',
                position: 'relative'
              }}>
                <div style={{
                  height: '100%',
                  width: `${Math.min(100, (skillsOverview.budgetChars / skillsOverview.budgetLimit) * 100)}%`,
                  background: skillsOverview.budgetChars > skillsOverview.budgetLimit 
                    ? 'var(--danger)' 
                    : 'linear-gradient(90deg, #a855f7, var(--accent))',
                  borderRadius: 3,
                  transition: 'width 0.3s ease'
                }} />
              </div>
            </div>
          </StatCard>
        </div>
      </div>

      {/* ── Two-Column Kanban Grid (Independent Columns to prevent stretching) ── */}
      <div style={{ padding: `0 ${SPACING.xl}px`, flexShrink: 0 }}>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: SPACING.sm, alignItems: 'start', marginBottom: SPACING.sm }}>
          
          {/* Left Column */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
            
            {/* Card 1: Token Usage */}
            <KanbanCard
              icon={<Sparkles size={16} />}
              title={t(loc, 'dashboard.tokenUsage')}
              accentColor="#3b82f6"
              summary={totalTokens > 0 ? fmtCount(totalTokens) : '—'}
              badge={modelCount > 0 ? `${modelCount} models` : undefined}
              isLoading={isUsageLoading}
              expanded={expandedCards.tokens}
              onToggle={() => handleToggleCard('tokens')}
            >
              <TokenHero
                usage={usageResult?.claude ?? null}
                isLoading={false}
                sourceConfigured={usageResult?.sourceConfigured}
              />
              {modelSummaryLines.length > 0 && (
                <div style={{ marginTop: SPACING.sm }}>
                  <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 6 }}>
                    {t(loc, 'dashboard.modelStats')}
                  </div>
                  {modelSummaryLines.map(m => (
                    <div key={m.id} style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: '3px 0', fontSize: FONT_SIZE.xs, borderTop: '0.0625rem solid var(--vibe-btn-border)' }}>
                      <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--vibe-brand-text)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.name}</span>
                      <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-dim)' }}>{fmtCount(m.tokens)}</span>
                      <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-dim)', width: 60, textAlign: 'right' }}>{m.cost > 0 ? `$${m.cost.toFixed(2)}` : '—'}</span>
                    </div>
                  ))}
                </div>
              )}
            </KanbanCard>

            {/* Card 2: Model Stats Table */}
            <KanbanCard
              icon={<BarChart3 size={16} />}
              title={t(loc, 'dashboard.modelStats') || '模型统计'}
              accentColor="#22c55e"
              summary={modelCount > 0 ? `${modelCount} models` : '—'}
              badge="Table"
              isLoading={isUsageLoading}
              expanded={expandedCards.models}
              onToggle={() => handleToggleCard('models')}
            >
              <ModelStatsTable
                modelStats={usageResult?.modelStats}
                isLoading={false}
                minimal
              />
            </KanbanCard>

            {/* Card 3: Token Savings */}
            <KanbanCard
              icon={<Coins size={16} />}
              title="Token 节约量"
              accentColor="#f59e0b"
              summary={rtkSaved > 0 ? fmtCount(rtkSaved) : '—'}
              badge={rtkCmds > 0 ? `${rtkCmds} commands` : undefined}
              isLoading={false}
              expanded={expandedCards.savings}
              onToggle={() => handleToggleCard('savings')}
            >
              {rtkData && rtkData.commands.length > 0 ? (
                <div>
                  <div style={{
                    display: 'flex', alignItems: 'center', gap: SPACING.sm,
                    padding: `${SPACING.sm}px ${SPACING.md}px`,
                    borderRadius: BORDER_RADIUS.md, marginBottom: SPACING.sm,
                    background: 'var(--vibe-btn-bg)', border: '0.0625rem solid var(--vibe-btn-border)',
                  }}>
                    <Coins size={16} style={{ color: '#f59e0b' }} />
                    <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-brand-text)' }}>
                      <strong>{fmtCount(rtkSaved)}</strong> tokens saved
                    </span>
                    <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)', flex: 1, textAlign: 'right' }}>
                      {rtkCmds} commands · {((rtkSaved as number) / Math.max(totalTokens, 1) * 100).toFixed(1)}% of total
                    </span>
                  </div>

                  <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 4, padding: `0 ${SPACING.xs}px` }}>
                    By Command
                  </div>
                  {rtkData.commands.slice(0, 15).map((cmd, i) => {
                    const percentage = (cmd.tokensSaved / maxRtkSaved) * 100;
                    return (
                      <div key={`${cmd.command}-${i}`} style={{
                        position: 'relative',
                        display: 'flex',
                        alignItems: 'center',
                        gap: SPACING.sm,
                        padding: '6px 8px',
                        fontSize: FONT_SIZE.xs,
                        borderTop: '0.0625rem solid var(--vibe-btn-border)',
                        borderRadius: BORDER_RADIUS.sm,
                        overflow: 'hidden',
                        transition: 'background 0.1s',
                      }}
                        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; }}
                        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
                      >
                        {/* Visual progress bar behind row */}
                        <div style={{
                          position: 'absolute',
                          left: 0,
                          top: 0,
                          bottom: 0,
                          width: `${percentage}%`,
                          background: '#f59e0b0d', // very subtle amber tint
                          zIndex: 0,
                        }} />
                        
                        <div style={{ display: 'flex', alignItems: 'center', width: '100%', zIndex: 1 }}>
                          {/* Rank */}
                          <span style={{ width: 18, textAlign: 'center', color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', fontSize: 10 }}>
                            #{i + 1}
                          </span>
                          {/* Command name */}
                          <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
                            {cmd.command}
                          </span>
                          {/* Count */}
                          <span style={{ color: 'var(--text-dim)', textAlign: 'right', minWidth: 40, marginRight: 8 }}>
                            {cmd.count}x
                          </span>
                          {/* Tokens saved */}
                          <span style={{ color: '#f59e0b', fontFamily: 'var(--font-mono)', fontWeight: 700, textAlign: 'right', minWidth: 60 }}>
                            {fmtCount(cmd.tokensSaved)}
                          </span>
                        </div>
                      </div>
                    );
                  })}
                  {rtkData.commands.length > 15 && (
                    <div style={{ textAlign: 'center', padding: `${SPACING.xs}px 0`, fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>
                      +{rtkData.commands.length - 15} more commands
                    </div>
                  )}
                </div>
              ) : (
                <div style={{ padding: `${SPACING.sm}px 0`, textAlign: 'center', color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>
                  Run <code style={{ background: 'var(--vibe-btn-bg)', padding: '1px 4px', borderRadius: 3 }}>rtk init -g</code> to start tracking token savings
                </div>
              )}
            </KanbanCard>

          </div>

          {/* Right Column */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>

            {/* Card 4: Tools Usage (Consolidated with Skills) */}
            <KanbanCard
              icon={<Wrench size={16} />}
              title={t(loc, 'dashboard.toolsUsage')}
              accentColor="#f97316"
              summary={String(totalRequests)}
              badge={`${totalRequests} reqs · ${skillCount} skills`}
              isLoading={isUsageLoading}
              expanded={expandedCards.tools}
              onToggle={() => handleToggleCard('tools')}
            >
              {usageResult?.claude ? (
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
                    <Code2 size={14} style={{ color: '#3b82f6' }} />
                    <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>Code</span>
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: SPACING.sm, marginBottom: SPACING.md }}>
                    <MiniMetric label={t(loc, 'dashboard.totalRequests')} value={fmtCount(usageResult.claude.totalRequests ?? 0)} accent="#3b82f6" />
                    <MiniMetric label={t(loc, 'dashboard.totalCost')} value={usageResult.claude.totalCost != null ? `$${usageResult.claude.totalCost.toFixed(2)}` : '—'} accent="#22c55e" />
                    <MiniMetric label={t(loc, 'dashboard.cacheHitRate')} value={`${usageResult.claude.activity?.totalSessions ?? 0}`} accent="#8b5cf6" />
                  </div>

                  <div style={{ borderTop: '0.0625rem solid var(--vibe-btn-border)', paddingTop: SPACING.sm }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
                      <Database size={14} style={{ color: '#8b5cf6' }} />
                      <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>Codex</span>
                    </div>
                    {codexUsage ? (
                      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: SPACING.sm }}>
                        <MiniMetric label="Total" value={fmtCount(codexUsage.totalTokens)} accent="#8b5cf6" />
                        <MiniMetric label="Sessions" value={fmtCount(codexUsage.totalSessions)} accent="#f97316" />
                        <MiniMetric label="Today" value={fmtCount(codexUsage.todayTokens)} accent="#22c55e" />
                        {codexUsage.totalCost > 0 && (
                          <MiniMetric label="Cost" value={`$${codexUsage.totalCost.toFixed(2)}`} accent="#eab308" />
                        )}
                      </div>
                    ) : (
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', textAlign: 'center', padding: `${SPACING.sm}px 0` }}>
                        No Codex usage data available
                      </div>
                    )}
                  </div>

                  {/* Skills ranking panel merged directly inside */}
                  <div style={{ borderTop: '0.0625rem solid var(--vibe-btn-border)', paddingTop: SPACING.sm, marginTop: SPACING.md }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
                      <Zap size={14} style={{ color: '#f43f5e' }} />
                      <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>{t(loc, 'dashboard.skillsTitle') || 'Skills'}</span>
                    </div>
                    <SkillsPanel skills={skills ?? undefined} isLoading={false} minimal />
                  </div>
                </div>
              ) : (
                <div style={{ padding: `${SPACING.sm}px 0`, textAlign: 'center', color: 'var(--text-faint)', fontSize: FONT_SIZE.xs }}>
                  {t(loc, 'dashboard.noData')}
                </div>
              )}
            </KanbanCard>

            {/* Card 6: Usage Trend Chart */}
            <KanbanCard
              icon={<Activity size={16} />}
              title={t(loc, 'dashboard.trendTitle') || '使用趋势'}
              accentColor="#8b5cf6"
              summary={usageHistory.length > 0 ? `${usageHistory.length} days` : '—'}
              badge="Chart"
              isLoading={isUsageLoading}
              expanded={expandedCards.trend}
              onToggle={() => handleToggleCard('trend')}
            >
              <TokenTrendChart
                usageHistory={usageHistory}
                isLoading={false}
                minimal
              />
            </KanbanCard>

          </div>
        </div>

        {/* ── Code Graph (collapsible, outside kanban grid) ── */}
        <div style={{ marginBottom: SPACING.sm }}>
          <div
            onClick={() => setCodeGraphExpanded(!codeGraphExpanded)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { setCodeGraphExpanded(!codeGraphExpanded); e.preventDefault(); } }}
            style={{
              display: 'flex', alignItems: 'center', gap: SPACING.sm,
              padding: `${SPACING.sm}px ${SPACING.md}px`,
              borderRadius: BORDER_RADIUS.lg,
              border: '0.0625rem solid var(--vibe-content-border)',
              background: 'var(--vibe-content-bg)',
              backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
              cursor: 'pointer', userSelect: 'none', transition: 'background 0.12s',
            }}
            onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)'; }}
            onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-content-bg)'; }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 32, height: 32, borderRadius: BORDER_RADIUS.md, background: '#8b5cf618', color: '#8b5cf6', flexShrink: 0 }}>
              <Layers size={16} />
            </div>
            <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--vibe-brand-text)', flex: 1 }}>
              Code Graph
            </span>
            <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-dim)' }}>
              {codeGraphExpanded ? '收起' : '展开'}
            </span>
            <div style={{ color: 'var(--text-faint)', flexShrink: 0, transition: 'transform 0.2s' }}>
              {codeGraphExpanded ? (
                <ChevronDown size={14} />
              ) : (
                <ChevronRight size={14} />
              )}
            </div>
          </div>
          {codeGraphExpanded && (
            <div style={{ marginTop: SPACING.xs }}>
              <CodeGraphPanel />
            </div>
          )}
        </div>
      </div>

      <div style={{ height: SPACING.xl, flexShrink: 0 }} />
    </div>
  );
}

// ── Chevron icons (inline to avoid extra import) ──
function ChevronDown({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}
function ChevronRight({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
}

// ── MiniMetric — tiny stat display ──
function MiniMetric({ label, value, accent }: { label: string; value: string; accent: string }) {
  return (
    <div style={{
      borderRadius: BORDER_RADIUS.sm, background: 'var(--vibe-btn-bg)',
      border: '0.0625rem solid var(--vibe-btn-border)', padding: `${SPACING.xs}px ${SPACING.sm}px`,
    }}>
      <div style={{ fontSize: '9px', color: 'var(--text-faint)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 2 }}>{label}</div>
      <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 700, color: accent, fontFamily: 'var(--font-mono)' }}>{value}</div>
    </div>
  );
}

// ── StatCard ──
interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  onClick?: () => void;
  title?: string;
  hoverColor?: string;
  badge?: string;
  subtext?: string;
  valueColor?: string;
  children?: React.ReactNode;
}

const StatCard = React.memo(function StatCard({
  icon,
  label,
  value,
  onClick,
  title,
  hoverColor = 'var(--vibe-accent-color)',
  badge,
  subtext,
  valueColor,
  children
}: StatCardProps) {
  const [hovered, setHovered] = useState(false);
  const [active, setActive] = useState(false);
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => { setHovered(false); setActive(false); }}
      onMouseDown={() => setActive(true)}
      onMouseUp={() => setActive(false)}
      title={title}
      style={{
        borderRadius: BORDER_RADIUS.md,
        border: `0.0625rem solid ${hovered ? hoverColor : 'var(--vibe-content-border)'}`,
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px)) saturate(var(--vibe-content-saturation, 145%))',
        padding: `${SPACING.sm}px ${SPACING.md}px`,
        display: 'flex',
        alignItems: 'center',
        gap: SPACING.sm,
        cursor: onClick ? 'pointer' : 'default',
        transform: active ? 'scale(0.98)' : hovered ? 'translateY(-2px)' : 'none',
        boxShadow: hovered ? `0 6px 20px ${hoverColor}15` : 'none',
        transition: 'border-color 0.2s, box-shadow 0.2s, transform 0.2s cubic-bezier(0.16, 1, 0.3, 1)',
        minWidth: 0,
        flex: 1,
        position: 'relative',
      }}
    >
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        width: 32, height: 32, borderRadius: BORDER_RADIUS.md,
        background: `${hoverColor}15`, color: hoverColor, flexShrink: 0,
        transform: hovered ? 'scale(1.05)' : 'none',
        transition: 'transform 0.2s',
      }}>
        {icon}
      </div>
      <div style={{ minWidth: 0, flex: 1 }}>
        <p style={{ fontSize: '10px', color: 'var(--text-faint)', lineHeight: 1.2 }}>{label}</p>
        <p style={{
          fontSize: '18px',
          fontWeight: 700,
          color: valueColor || 'var(--vibe-brand-text)',
          fontFamily: 'var(--font-mono)',
          lineHeight: 1.3,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}>{value}</p>
        {subtext && <p style={{ fontSize: '10px', color: 'var(--text-dim)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{subtext}</p>}
        {children}
      </div>
      {badge && (
        <span style={{
          position: 'absolute',
          top: 6,
          right: 8,
          fontSize: '9px',
          fontWeight: 600,
          color: hoverColor,
          background: `${hoverColor}15`,
          padding: '1px 5px',
          borderRadius: 10,
          fontFamily: 'var(--font-mono)',
        }}>
          {badge}
        </span>
      )}
    </div>
  );
});
