'use client';

import type { ReactNode } from 'react';
import {
  AlertTriangle,
  Code2,
  ExternalLink,
  Folder,
  Github,
  PackagePlus,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Settings2,
  Sparkles,
  Trash2,
  Wand2,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { EmptyState } from '@/components/ui/EmptyState';
import { isActionBusy, mergeActionsWithBusy, sortCreativeApps, sourceBadge } from '@/lib/creative-app';
import type { CreativeAppState, CreativeAppSummary } from '@/lib/tauri-adapter';

/**
 * ADR-0014 决策 4：作品目录不再把三种来源平铺为同种卡片。
 * 内部 Workshop 应用是「可以继续创作的作品」，导入的应用是「需要运行管理的外物」——
 * 两者的主操作根本不同，所以分组发生在数据层而不是靠卡片内部 if 分叉。
 */
export interface CreativeCatalogGroups {
  /** source === 'internal'：AI 生成 / Workshop 静态 SPA，可继续创作。 */
  creations: CreativeAppSummary[];
  /** external_github + local_project：只导入与运行，不进创作主流程。 */
  imported: CreativeAppSummary[];
}

export function groupCreativeApps(apps: CreativeAppSummary[]): CreativeCatalogGroups {
  const creations: CreativeAppSummary[] = [];
  const imported: CreativeAppSummary[] = [];
  for (const app of apps) {
    if (app.source === 'internal') creations.push(app);
    else imported.push(app);
  }
  // 组内仍沿用统一排序，避免两组出现两套「谁在前」的规则。
  return { creations: sortCreativeApps(creations), imported: sortCreativeApps(imported) };
}

/** 状态文案必须来自 app.state；缺失映射时回落原始枚举值，绝不猜成「运行中」。 */
const STATE_KEYS: Record<CreativeAppState, string> = {
  available: 'workshop.stateAvailable',
  disabled: 'workshop.stateDisabled',
  installing: 'workshop.stateInstalling',
  installed_stopped: 'workshop.stateInstalledStopped',
  starting: 'workshop.stateStarting',
  running: 'workshop.stateRunning',
  stopping: 'workshop.stateStopping',
  runtime_unavailable: 'workshop.stateRuntimeUnavailable',
  install_failed: 'workshop.stateInstallFailed',
  start_failed: 'workshop.stateStartFailed',
  deleting: 'workshop.stateDeleting',
  delete_failed: 'workshop.stateDeleteFailed',
  cleanup_failed: 'workshop.stateCleanupFailed',
  orphaned: 'workshop.stateOrphaned',
};

function stateLabel(locale: Locale, state: CreativeAppState): string {
  const key = STATE_KEYS[state];
  return key ? t(locale, key) : state;
}

const FAILED_STATES: CreativeAppState[] = [
  'install_failed',
  'start_failed',
  'delete_failed',
  'cleanup_failed',
  'orphaned',
  'runtime_unavailable',
];

function StatusDot({ state }: { state: CreativeAppState }) {
  if (state === 'running') {
    return <span className="h-2 w-2 rounded-full bg-emerald-500 animate-pulse shrink-0" />;
  }
  // 过渡态用转圈而不是色点：用户要看出「正在变」，静止的点会被读成终态。
  if (isActionBusy(state)) {
    return <RefreshCw size={12} className="animate-spin text-[var(--primary)] shrink-0" />;
  }
  const color = FAILED_STATES.includes(state) ? 'bg-rose-500' : 'bg-zinc-400 dark:bg-zinc-500';
  return <span className={`h-2 w-2 rounded-full shrink-0 ${color}`} />;
}

/** 来源徽章的三种画法收在一处，卡片里不再出现三层三元表达式。 */
const BADGE_STYLE = {
  github: { cls: 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20', Icon: Github, labelKey: 'workshop.sourceGithub' },
  local: { cls: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 border-emerald-500/20', Icon: Folder, labelKey: 'workshop.sourceLocal' },
  internal: { cls: 'bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20', Icon: Code2, labelKey: 'workshop.sourceInternal' },
} as const;

const BTN_PRIMARY =
  'h-8 px-3 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 active:scale-95 transition-all flex items-center justify-center gap-1.5 shrink-0 disabled:opacity-40 disabled:pointer-events-none';
const BTN_SECONDARY =
  'h-8 px-3 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0';
const BTN_ICON =
  'h-8 px-2.5 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0';
const BTN_DANGER =
  'h-8 px-2.5 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-rose-500 hover:bg-rose-500/10 hover:border-rose-500/20 transition-all flex items-center justify-center gap-1.5 shrink-0 ml-auto';

export interface CreativeCatalogProps {
  apps: CreativeAppSummary[];
  locale: Locale;
  /** 乐观并发中的 id（命令已发出、事件未回）；与 state 中的过渡态一起决定 busy。 */
  busyIds?: ReadonlySet<string>;
  onOpen: (app: CreativeAppSummary) => void;
  /** 内部作品专属：以已发布模块为种子开新草稿（ADR-0014 第 9 节）。 */
  onContinueCreating: (app: CreativeAppSummary) => void;
  onStart: (app: CreativeAppSummary) => void;
  onStop: (app: CreativeAppSummary) => void;
  onDelete: (app: CreativeAppSummary) => void;
  onRestart?: (app: CreativeAppSummary) => void;
  onLogs?: (app: CreativeAppSummary) => void;
  /** 已导入应用的运行设置（端口 / 启动方式 / 自动打开）。 */
  onRunSettings?: (app: CreativeAppSummary) => void;
  /** 孤儿进程恢复：应用重启后发现残留进程时的唯一出路（statusDetail.code=orphaned_process）。 */
  onResolveOrphan?: (app: CreativeAppSummary, restart: boolean) => void;
  /** 本地项目的依赖安装（npm/pnpm install）；后端两条命令此前无入口。 */
  onInstallDeps?: (app: CreativeAppSummary) => void;
  /** 空态 CTA：回到创作入口 / 导入入口，避免空目录成为死路。 */
  onCreateNew?: () => void;
  onImport?: () => void;
}

interface CardShellProps {
  app: CreativeAppSummary;
  locale: Locale;
  onResolveOrphan?: (app: CreativeAppSummary, restart: boolean) => void;
  children: ReactNode;
}

/** 卡片的信息区两组通用；差异全部落在 children（动作区），这样差异一眼可见。 */
function CardShell({ app, locale, onResolveOrphan, children }: CardShellProps) {
  const badge = BADGE_STYLE[sourceBadge(app.source)];
  return (
    <div className="bg-[var(--surface)] border border-[var(--border)] rounded-xl p-4 flex flex-col justify-between transition-all hover:border-[var(--border-hover)] hover:shadow-sm">
      <div>
        <div className="flex items-center justify-between gap-2 border-b border-[var(--border-subtle)] pb-2.5 mb-2.5">
          <div className="font-semibold text-sm text-[var(--text)] truncate" title={app.title}>
            {app.title}
          </div>
          <span
            className={`text-[10px] font-medium px-2 py-0.5 rounded-full flex items-center gap-1 border shrink-0 ${badge.cls}`}
          >
            <badge.Icon size={10} />
            <span>{t(locale, badge.labelKey)}</span>
          </span>
        </div>

        <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)] mb-2">
          <div className="flex items-center gap-1.5 font-medium">
            <StatusDot state={app.state} />
            <span className="text-[var(--text)]">{stateLabel(locale, app.state)}</span>
          </div>
          {app.version && (
            <>
              <span className="text-[var(--border)]">•</span>
              <span className="font-mono text-[11px]">{app.version}</span>
            </>
          )}
        </div>

        {app.description && (
          <div className="text-[11px] text-[var(--text-secondary)] line-clamp-2 mb-2">
            {app.description}
          </div>
        )}

        {app.lastError && (
          <div className="text-[11px] text-rose-500 bg-rose-500/10 border border-rose-500/20 p-2 rounded-lg mb-2 flex items-start gap-1.5">
            <AlertTriangle size={12} className="shrink-0 mt-0.5" />
            <span className="line-clamp-2">{app.statusDetail?.message || app.lastError}</span>
          </div>
        )}

        {/* 孤儿进程：后端 resolve_orphan 早已就绪但前端无任何入口，
            用户只能眼看着应用卡在「残留进程」状态 */}
        {app.statusDetail?.code === 'orphaned_process' && onResolveOrphan && (
          <div className="mb-2 flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => onResolveOrphan(app, false)}
              className="rounded-md border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-secondary)] hover:text-[var(--text)]"
            >
              {t(locale, 'workshop.orphanStop')}
            </button>
            <button
              type="button"
              onClick={() => onResolveOrphan(app, true)}
              className="rounded-md border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-secondary)] hover:text-[var(--text)]"
            >
              {t(locale, 'workshop.orphanRestart')}
            </button>
          </div>
        )}
      </div>

      <div className="flex items-center gap-1.5 mt-3 pt-3 border-t border-[var(--border-subtle)] flex-wrap">
        {children}
      </div>
    </div>
  );
}

interface CardProps extends CardShellProps {
  busy: boolean;
  handlers: CreativeCatalogProps;
}

/**
 * 动作按钮只有「样式 + 图标 + 文案 + 回调」四个变量。收成一处后，两组卡片的差异
 * 就只剩下各自的动作清单本身 —— 这正是本次改版要让人一眼看见的东西。
 */
function ActionBtn(p: {
  cls: string;
  icon: ReactNode;
  labelKey: string;
  locale: Locale;
  onClick: () => void;
  /** 次要动作只留图标，文案降级为 title，避免动作区把卡片撑爆。 */
  iconOnly?: boolean;
  disabled?: boolean;
}) {
  const label = t(p.locale, p.labelKey);
  return (
    <button type="button" className={p.cls} title={label} disabled={p.disabled} onClick={p.onClick}>
      {p.icon}
      {!p.iconOnly && <span>{label}</span>}
    </button>
  );
}

/** 「继续创作」组：主操作是回到创作台，运行控制被刻意省略（Workshop 静态 SPA 无进程可启停）。 */
function CreationCard({ app, locale, busy, handlers }: Omit<CardProps, 'children'>) {
  const blocked = busy || isActionBusy(app.state);
  const actions = mergeActionsWithBusy(app.actions, blocked);
  return (
    <CardShell app={app} locale={locale}>
      <ActionBtn cls={BTN_PRIMARY} icon={<Wand2 size={13} />} locale={locale} disabled={blocked}
        labelKey="creative.catalog.actionContinueCreating"
        onClick={() => handlers.onContinueCreating(app)} />
      {actions.canOpen && (
        <ActionBtn cls={BTN_SECONDARY} icon={<ExternalLink size={13} />} locale={locale}
          labelKey="workshop.actionOpen" onClick={() => handlers.onOpen(app)} />
      )}
      {actions.canDelete && (
        <ActionBtn cls={BTN_DANGER} icon={<Trash2 size={13} />} locale={locale} iconOnly
          labelKey="workshop.actionDelete" onClick={() => handlers.onDelete(app)} />
      )}
    </CardShell>
  );
}

/** 「已导入应用」组：主操作是启动 / 停止 + 日志 + 运行设置，没有继续创作入口。 */
function ImportedCard({ app, locale, busy, handlers }: Omit<CardProps, 'children'>) {
  const actions = mergeActionsWithBusy(app.actions, busy || isActionBusy(app.state));
  // 重启只在进程可能存在时才有意义；其它状态下点它只会产生误导。
  const restartable = app.state === 'running' || app.state === 'start_failed';
  return (
    <CardShell app={app} locale={locale} onResolveOrphan={handlers.onResolveOrphan}>
      {actions.canStart && (
        <ActionBtn cls={BTN_PRIMARY} icon={<Play size={13} />} locale={locale}
          labelKey="workshop.actionStart" onClick={() => handlers.onStart(app)} />
      )}
      {actions.canStop && (
        <ActionBtn cls={BTN_SECONDARY} icon={<Pause size={13} />} locale={locale}
          labelKey="workshop.actionStop" onClick={() => handlers.onStop(app)} />
      )}
      {actions.canOpen && (
        <ActionBtn cls={BTN_SECONDARY} icon={<ExternalLink size={13} />} locale={locale}
          labelKey="workshop.actionOpen" onClick={() => handlers.onOpen(app)} />
      )}
      {actions.canRetry && (
        <ActionBtn cls={BTN_SECONDARY} icon={<RotateCcw size={13} />} locale={locale}
          labelKey="workshop.actionRetry" onClick={() => handlers.onStart(app)} />
      )}
      {handlers.onRestart && restartable && (
        <ActionBtn cls={BTN_ICON} icon={<RotateCcw size={13} />} locale={locale} iconOnly
          labelKey="workshop.actionRestart" onClick={() => handlers.onRestart?.(app)} />
      )}
      {handlers.onLogs && (
        <ActionBtn cls={BTN_ICON} icon={<ScrollText size={13} />} locale={locale} iconOnly
          labelKey="workshop.actionLogs" onClick={() => handlers.onLogs?.(app)} />
      )}
      {handlers.onRunSettings && (
        <ActionBtn cls={BTN_ICON} icon={<Settings2 size={13} />} locale={locale} iconOnly
          labelKey="creative.catalog.actionRunSettings" onClick={() => handlers.onRunSettings?.(app)} />
      )}
      {/* 依赖安装只对本地项目有意义（容器应用在镜像里已装好） */}
      {handlers.onInstallDeps && app.source === 'local_project' && (
        <ActionBtn cls={BTN_ICON} icon={<PackagePlus size={13} />} locale={locale} iconOnly
          labelKey="workshop.installDeps" onClick={() => handlers.onInstallDeps?.(app)} />
      )}
      {actions.canDelete && (
        <ActionBtn cls={BTN_DANGER} icon={<Trash2 size={13} />} locale={locale} iconOnly
          labelKey="workshop.actionDelete" onClick={() => handlers.onDelete(app)} />
      )}
    </CardShell>
  );
}

const GRID = 'grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4';

/** key 全量写死而非拼接，i18n 同步检查才能 grep 到它们。 */
const GROUP_KEYS = {
  creations: { title: 'creative.catalog.creationsTitle', hint: 'creative.catalog.creationsHint', emptyTitle: 'creative.catalog.creationsEmptyTitle', emptyDesc: 'creative.catalog.creationsEmptyDesc', emptyAction: 'creative.catalog.creationsEmptyAction' },
  imported: { title: 'creative.catalog.importedTitle', hint: 'creative.catalog.importedHint', emptyTitle: 'creative.catalog.importedEmptyTitle', emptyDesc: 'creative.catalog.importedEmptyDesc', emptyAction: 'creative.catalog.importedEmptyAction' },
} as const;

/**
 * 两组共享外壳但各带自己的空态：文案分开，是因为「没创作过」和「没导入过」
 * 要把用户推向两个不同的入口。
 */
function Group(p: {
  locale: Locale;
  keys: (typeof GROUP_KEYS)[keyof typeof GROUP_KEYS];
  icon: ReactNode;
  items: CreativeAppSummary[];
  onEmptyAction?: () => void;
  renderCard: (app: CreativeAppSummary) => ReactNode;
}) {
  return (
    <section>
      <div className="flex items-baseline gap-2 mb-3">
        <h3 className="text-sm font-semibold text-[var(--text)]">{t(p.locale, p.keys.title)}</h3>
        <span className="text-[11px] font-mono text-[var(--text-secondary)]">{p.items.length}</span>
        <span className="text-[11px] text-[var(--text-secondary)] truncate">{t(p.locale, p.keys.hint)}</span>
      </div>
      {p.items.length === 0 ? (
        <EmptyState
          icon={p.icon}
          title={t(p.locale, p.keys.emptyTitle)}
          description={t(p.locale, p.keys.emptyDesc)}
          action={
            p.onEmptyAction
              ? { label: t(p.locale, p.keys.emptyAction), onClick: p.onEmptyAction }
              : undefined
          }
        />
      ) : (
        <div className={GRID}>{p.items.map((app) => p.renderCard(app))}</div>
      )}
    </section>
  );
}

/** 作品目录：对 apps 的只读投影，本身不持有状态也不发命令。 */
export default function CreativeCatalog(props: CreativeCatalogProps) {
  const { apps, locale, busyIds, onCreateNew, onImport } = props;
  const { creations, imported } = groupCreativeApps(apps);
  const isBusy = (app: CreativeAppSummary) => Boolean(busyIds?.has(app.id));

  return (
    <div className="flex flex-col gap-8">
      <Group
        locale={locale}
        keys={GROUP_KEYS.creations}
        icon={<Sparkles size={32} />}
        items={creations}
        onEmptyAction={onCreateNew}
        renderCard={(app) => (
          <CreationCard key={app.id} app={app} locale={locale} busy={isBusy(app)} handlers={props} />
        )}
      />
      <Group
        locale={locale}
        keys={GROUP_KEYS.imported}
        icon={<Folder size={32} />}
        items={imported}
        onEmptyAction={onImport}
        renderCard={(app) => (
          <ImportedCard key={app.id} app={app} locale={locale} busy={isBusy(app)} handlers={props} />
        )}
      />
    </div>
  );
}
