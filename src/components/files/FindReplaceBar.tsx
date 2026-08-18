'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { CaseSensitive, ChevronDown, ChevronUp, CornerDownLeft, Replace, X } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { SPACING } from '@/lib/design-tokens';

export interface FindReplaceBarProps {
  locale: Locale;
  query: string;
  onQueryChange: (value: string) => void;
  matchCase: boolean;
  onToggleMatchCase: () => void;
  /** 当前匹配索引（0-based），-1 表示无匹配。 */
  index: number;
  count: number;
  onNavigate: (direction: 'prev' | 'next') => void;
  onClose: () => void;
  /** 只读 Preview 无替换；Monaco/Milkdown 编辑路径才提供。 */
  canReplace?: boolean;
  replacement?: string;
  onReplacementChange?: (value: string) => void;
  onReplaceOne?: () => void;
  onReplaceAll?: () => void;
}

/**
 * 文件面板内聚焦时的查找/替换 bar（问题12）。
 * - 只在面板聚焦时由 FilePreview 拦截 Cmd/Ctrl+F 打开，不抢全局快捷键
 * - 只读 Preview：仅查找、上/下一个、大小写、结果数和有界匹配，无替换
 * - Escape 先关 find bar，再关预览（层级由 FilePreview 统一处理）
 */
export default function FindReplaceBar({
  locale,
  query,
  onQueryChange,
  matchCase,
  onToggleMatchCase,
  index,
  count,
  onNavigate,
  onClose,
  canReplace = false,
  replacement = '',
  onReplacementChange,
  onReplaceOne,
  onReplaceAll,
}: FindReplaceBarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [focused, setFocused] = useState<'find' | 'replace'>('find');

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        // 已打开时重复 Cmd/Ctrl+F 回到 find 输入框
        e.preventDefault();
        e.stopPropagation();
        setFocused('find');
        inputRef.current?.focus();
        inputRef.current?.select();
      } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'g') {
        // Cmd/Ctrl+Shift+G 上一个匹配（浏览器习惯）
        e.preventDefault();
        e.stopPropagation();
        onNavigate('prev');
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'g') {
        // Cmd/Ctrl+G 下一个匹配（浏览器习惯）
        e.preventDefault();
        e.stopPropagation();
        onNavigate('next');
      } else if (e.key === 'Enter' && !e.shiftKey && focused === 'replace' && onReplaceOne) {
        e.preventDefault();
        onReplaceOne();
      }
    },
    [focused, onClose, onNavigate, onReplaceOne],
  );

  const inputStyle: React.CSSProperties = {
    height: 28,
    minWidth: 0,
    flex: 1,
    padding: `0 ${SPACING.sm}px`,
    border: '1px solid var(--border)',
    borderRadius: 6,
    background: 'var(--surface)',
    color: 'var(--text)',
    fontSize: 12,
    outline: 'none',
  };
  const iconBtnStyle: React.CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 26,
    height: 26,
    border: 0,
    borderRadius: 6,
    background: 'transparent',
    color: 'var(--text-secondary)',
    cursor: 'pointer',
  };

  return (
    <div
      className="flex items-center gap-1.5 border-b border-[var(--border)] bg-[var(--surface-hover)] px-2 py-1.5"
      role="search"
      aria-label={t(locale, 'fileBrowser.findInFile')}
      onKeyDown={handleKeyDown}
    >
      <div className="flex min-w-0 flex-1 items-center gap-1.5">
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          onFocus={() => setFocused('find')}
          placeholder={t(locale, 'fileBrowser.findPlaceholder')}
          aria-label={t(locale, 'fileBrowser.findPlaceholder')}
          style={inputStyle}
        />
        {canReplace && (
          <input
            value={replacement}
            onChange={(e) => onReplacementChange?.(e.target.value)}
            onFocus={() => setFocused('replace')}
            placeholder={t(locale, 'fileBrowser.replacePlaceholder')}
            aria-label={t(locale, 'fileBrowser.replacePlaceholder')}
            style={inputStyle}
          />
        )}
      </div>

      {/* 结果数（aria-live 播报） */}
      <span
        aria-live="polite"
        className="whitespace-nowrap text-[11px] tabular-nums text-[var(--text-disabled)]"
        data-testid="find-result-count"
      >
        {count > 0 ? `${index + 1}/${count}` : count === 0 && query ? '0' : ''}
      </span>

      <button
        type="button"
        onClick={onToggleMatchCase}
        title={t(locale, 'fileBrowser.findMatchCase')}
        aria-label={t(locale, 'fileBrowser.findMatchCase')}
        aria-pressed={matchCase}
        style={{ ...iconBtnStyle, color: matchCase ? 'var(--primary)' : undefined }}
      >
        <CaseSensitive size={14} />
      </button>
      <button
        type="button"
        onClick={() => onNavigate('prev')}
        title={t(locale, 'fileBrowser.findPrev')}
        aria-label={t(locale, 'fileBrowser.findPrev')}
        style={iconBtnStyle}
      >
        <ChevronUp size={14} />
      </button>
      <button
        type="button"
        onClick={() => onNavigate('next')}
        title={t(locale, 'fileBrowser.findNext')}
        aria-label={t(locale, 'fileBrowser.findNext')}
        style={iconBtnStyle}
      >
        <ChevronDown size={14} />
      </button>
      {canReplace && onReplaceOne && (
        <button
          type="button"
          onClick={onReplaceOne}
          title={t(locale, 'fileBrowser.replaceOne')}
          aria-label={t(locale, 'fileBrowser.replaceOne')}
          style={iconBtnStyle}
        >
          <Replace size={14} />
        </button>
      )}
      {canReplace && onReplaceAll && (
        <button
          type="button"
          onClick={onReplaceAll}
          title={t(locale, 'fileBrowser.replaceAll')}
          aria-label={t(locale, 'fileBrowser.replaceAll')}
          style={{ ...iconBtnStyle, color: 'var(--text-secondary)' }}
        >
          <CornerDownLeft size={14} />
        </button>
      )}
      <button
        type="button"
        onClick={onClose}
        title={t(locale, 'common.close')}
        aria-label={t(locale, 'common.close')}
        style={iconBtnStyle}
      >
        <X size={14} />
      </button>
    </div>
  );
}
