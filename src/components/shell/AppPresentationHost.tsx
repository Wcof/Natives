'use client';

/**
 * AppPresentationHost — 当前应用呈现的内容区容器（APPV2-T02 / T03 挂载点）。
 *
 * 职责（方案 4.2）：
 * - Web：最小工具条（重载/后退/前进/关闭）+ 状态层（switching / error / unsupported），
 *   child WebView 覆盖内容矩形；bounds 由本组件测量（T03 经 debounce 后发给 Host）。
 * - macOS：Natives 只提供承载背景、状态与修复动作；真正页面来自归位后的外部窗口
 *   （窗口控制由 T05–T07 的 AXWindowDriver 提供，本组件不实现任何窗口强控）。
 * - 内容矩形：ResizeObserver + getBoundingClientRect()；resize/侧栏动画期间只更新
 *   内存，短 debounce 后才触发 onBoundsChange。
 *
 * 本组件不承载 Registry CRUD —— 管理仍在 AppsPage。
 */

import React, { useEffect, useRef } from 'react';
import { AlertCircle, ArrowLeft, ArrowRight, RotateCw, X } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { UNLOCAL_PROJECT_ERROR, type ActiveAppTarget } from '@/lib/app-target';
import type { DockResult, SystemRunningState } from '@/lib/tauri/apps';

export interface AppPresentationBounds {
  /** viewport 坐标（逻辑像素）；T03 由 Host 换算为目标屏幕坐标。 */
  x: number;
  y: number;
  width: number;
  height: number;
}

export type AppWebCommand = 'reload' | 'back' | 'forward' | 'close';
export type AppSystemCommand = 'activate' | 'hide' | 'terminate' | 'redock' | 'settings' | 'recheck';

export interface AppPresentationHostProps {
  target: ActiveAppTarget | null;
  locale: Locale;
  onRetry: (appId: string) => void;
  onBackToApps: () => void;
  onWebCommand: (appId: string, command: AppWebCommand) => void;
  onSystemCommand: (appId: string, command: AppSystemCommand) => void;
  systemState?: SystemRunningState | null;
  dockResult?: DockResult | null;
  needsRedock?: boolean;
  /** T03 接线：内容矩形稳定后通知（debounce 后）；null = 卸载。 */
  onBoundsChange?: (bounds: AppPresentationBounds | null) => void;
}

const TOOL_BTN =
  'flex h-6 w-6 items-center justify-center rounded-md text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-primary)] disabled:opacity-40 disabled:hover:bg-transparent';

export default function AppPresentationHost({
  target,
  locale,
  onRetry,
  onBackToApps,
  onWebCommand,
  onSystemCommand,
  systemState,
  dockResult,
  needsRedock = false,
  onBoundsChange,
}: AppPresentationHostProps) {
  const contentRef = useRef<HTMLDivElement | null>(null);
  const boundsTimerRef = useRef<number | null>(null);

  // 内容矩形测量：ResizeObserver + 150ms debounce（T03 接到 apps_web_set_bounds）。
  useEffect(() => {
    const el = contentRef.current;
    if (!el || !onBoundsChange) return;
    const report = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        onBoundsChange({ x: rect.x, y: rect.y, width: rect.width, height: rect.height });
      }
    };
    const debounced = () => {
      if (boundsTimerRef.current !== null) window.clearTimeout(boundsTimerRef.current);
      boundsTimerRef.current = window.setTimeout(report, 150);
    };
    const ro = new ResizeObserver(debounced);
    ro.observe(el);
    // Initial immediate report if element is already measured with positive dimensions
    const initialRect = el.getBoundingClientRect();
    if (initialRect.width > 0 && initialRect.height > 0) {
      onBoundsChange({ x: initialRect.x, y: initialRect.y, width: initialRect.width, height: initialRect.height });
    } else {
      debounced();
    }
    return () => {
      ro.disconnect();
      if (boundsTimerRef.current !== null) window.clearTimeout(boundsTimerRef.current);
      onBoundsChange(null);
    };
  }, [onBoundsChange]);

  if (!target) return null;

  const view = target.view;
  const isWeb = target.kind === 'web_application';
  const title = view?.title ?? target.appId;
  const unsupportedLocal = target.phase === 'error' && target.error === UNLOCAL_PROJECT_ERROR;
  const canUseToolbar = target.phase === 'presented';

  return (
    <div className="flex h-full w-full flex-col" data-app-presentation={target.appId}>
      {/* Web 最小工具条（仅 web 目标；关闭 = 隐藏 Surface，永不终止应用） */}
      {isWeb && view && (
        <div
          className="flex h-9 shrink-0 items-center gap-1 border-b border-[var(--border-default)] px-2"
          style={{ background: 'var(--surface-overlay)' }}
        >
          <button
            type="button"
            className={TOOL_BTN}
            disabled={!canUseToolbar}
            title={t(locale, 'appsPage.presentWebBack')}
            onClick={() => onWebCommand(target.appId, 'back')}
          >
            <ArrowLeft size={14} />
          </button>
          <button
            type="button"
            className={TOOL_BTN}
            disabled={!canUseToolbar}
            title={t(locale, 'appsPage.presentWebForward')}
            onClick={() => onWebCommand(target.appId, 'forward')}
          >
            <ArrowRight size={14} />
          </button>
          <button
            type="button"
            className={TOOL_BTN}
            disabled={!canUseToolbar}
            title={t(locale, 'appsPage.presentWebReload')}
            onClick={() => onWebCommand(target.appId, 'reload')}
          >
            <RotateCw size={14} />
          </button>
          <span className="mx-2 min-w-0 flex-1 truncate text-xs font-medium text-[var(--text-primary)]">
            {title}
          </span>
          <button
            type="button"
            className={TOOL_BTN}
            title={t(locale, 'appsPage.presentWebCloseHint')}
            onClick={() => onWebCommand(target.appId, 'close')}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* 内容矩形（T03：child WebView bounds 锚点，覆盖于此矩形内） */}
      <div ref={contentRef} className="relative min-h-0 flex-1">
        {target.phase === 'switching' && (
          <div className="flex h-full flex-col items-center justify-center gap-3">
            <MathCurveLoader size={40} />
            <span className="text-xs text-[var(--text-secondary)]">
              {t(locale, 'appsPage.presentSwitching')}
            </span>
          </div>
        )}

        {target.phase === 'error' && (
          <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
            <AlertCircle className="h-6 w-6 text-[var(--danger)]" />
            <span className="text-sm font-medium text-[var(--text-primary)]">
              {unsupportedLocal
                ? t(locale, 'appsPage.presentUnsupportedLocal')
                : t(locale, 'appsPage.presentError')}
            </span>
            {!unsupportedLocal && target.error && (
              <span className="max-w-md break-words text-[11px] text-[var(--text-tertiary)]">
                {target.error}
              </span>
            )}
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => onRetry(target.appId)}
                className="rounded-xl bg-[var(--interactive-accent)] px-3 py-1.5 text-xs font-medium text-[var(--text-on-accent)] hover:opacity-90"
              >
                {t(locale, 'appsPage.presentRetry')}
              </button>
              <button
                type="button"
                onClick={onBackToApps}
                className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-primary)]"
              >
                {t(locale, 'appsPage.presentBackToApps')}
              </button>
            </div>
          </div>
        )}

        {/* macOS 状态层：外部独立系统窗口，Natives 只显示承载状态（T05–T07 提供窗口控制） */}
        {target.kind === 'system_application' && target.phase === 'presented' && (
          <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
            <span className="text-sm font-medium text-[var(--text-primary)]">{title}</span>
            <span className="text-xs text-[var(--text-secondary)]">
              {!systemState?.installed
                ? t(locale, 'appsPage.presentMacNotInstalled')
                : systemState.unobservable
                  ? t(locale, 'appsPage.presentMacUnobservable')
                  : systemState.active
                    ? t(locale, 'appsPage.presentMacActive')
                    : systemState.hidden
                      ? t(locale, 'appsPage.presentMacHidden')
                      : systemState.running
                        ? t(locale, 'appsPage.presentMacRunning')
                        : t(locale, 'appsPage.presentMacStopped')}
            </span>
            <span className="text-[10px] text-[var(--text-tertiary)]">
              {needsRedock
                ? t(locale, 'appsPage.presentMacNeedsRedock')
                : dockResult?.status === 'docked'
                  ? t(locale, 'appsPage.presentMacDocked')
                  : dockResult?.status === 'permission_required'
                    ? t(locale, 'appsPage.presentMacPermission')
                    : dockResult?.status === 'unsupported'
                      ? t(locale, 'appsPage.presentMacUnsupported')
                      : dockResult?.message ?? t(locale, 'appsPage.presentMacDockPending')}
            </span>
            <div className="flex flex-wrap justify-center gap-2">
              <button type="button" onClick={() => onSystemCommand(target.appId, 'activate')} className="rounded-xl bg-[var(--interactive-accent)] px-3 py-1.5 text-xs font-medium text-[var(--text-on-accent)]">
                {t(locale, 'appsPage.presentMacActivate')}
              </button>
              {systemState?.running && !systemState.hidden && (
                <button type="button" onClick={() => onSystemCommand(target.appId, 'hide')} className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)]">
                  {t(locale, 'appsPage.presentMacHide')}
                </button>
              )}
              {systemState?.running && (
                <button type="button" onClick={() => onSystemCommand(target.appId, 'terminate')} className="rounded-xl border border-[var(--danger)]/30 px-3 py-1.5 text-xs text-[var(--danger)]">
                  {t(locale, 'appsPage.presentMacTerminate')}
                </button>
              )}
              <button type="button" onClick={() => onSystemCommand(target.appId, 'redock')} className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)]">
                {t(locale, 'appsPage.presentMacRedock')}
              </button>
              {dockResult?.status === 'permission_required' && (
                <button type="button" onClick={() => onSystemCommand(target.appId, 'settings')} className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)]">
                  {t(locale, 'appsPage.presentMacOpenSettings')}
                </button>
              )}
              <button type="button" onClick={() => onSystemCommand(target.appId, 'recheck')} className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)]">
                {t(locale, 'appsPage.presentMacRecheck')}
              </button>
            </div>
            <button
              type="button"
              onClick={onBackToApps}
              className="rounded-xl border border-[var(--border-default)] px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-primary)]"
            >
              {t(locale, 'appsPage.presentBackToApps')}
            </button>
          </div>
        )}

        {/* web presented：child WebView 覆盖内容矩形，状态层无遮挡 */}
      </div>
    </div>
  );
}
