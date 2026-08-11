'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { findMatches, navigateMatch, FIND_MATCH_LIMIT } from '@/lib/find-replace';

/**
 * 文件面板内的查找状态机（问题12）。
 *
 * - 只在面板聚焦时由 FilePreview 打开（不抢全局快捷键）
 * - 只读 Preview 查找：用 TreeWalker 遍历文本节点定位匹配并滚动到当前项，
 *   不改写 DOM；PDF/sandbox iframe（PreviewSurface 内部）天然不进 TreeWalker，
 *   查找结果为 0，诚实显示「无匹配」而非伪造
 * - Escape 层级由 FilePreview 处理：先关 bar，再关预览
 */
export function useFindReplace(containerRef: React.RefObject<HTMLElement | null>) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [matchCase, setMatchCase] = useState(false);
  const [index, setIndex] = useState(-1);
  const [count, setCount] = useState(0);
  const rangesRef = useRef<
    Array<{ node: Text; start: number; end: number; range: Range }>
  >([]);
  const currentRangeRef = useRef<Range | null>(null);

  /** 移除上一次高亮，避免叠加。 */
  const clearHighlight = useCallback(() => {
    if (currentRangeRef.current) {
      try {
        currentRangeRef.current.detach();
      } catch { /* no-op */ }
      currentRangeRef.current = null;
    }
    rangesRef.current = [];
  }, []);

  const close = useCallback(() => {
    clearHighlight();
    setOpen(false);
    setQuery('');
    setIndex(-1);
    setCount(0);
  }, [clearHighlight]);

  const runFind = useCallback(
    (value: string) => {
      const root = containerRef.current;
      clearHighlight();
      if (!root || !value.trim()) {
        setCount(0);
        setIndex(-1);
        return;
      }
      // 面板内全部文本（TreeWalker 覆盖 text node，不进入 iframe）
      let fullText = '';
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      const segments: Array<{ node: Text; start: number }> = [];
      let node: Node | null = walker.nextNode();
      while (node) {
        const text = (node as Text).data;
        segments.push({ node: node as Text, start: fullText.length });
        fullText += text;
        node = walker.nextNode();
      }
      const matches = findMatches(fullText, value, {
        caseSensitive: matchCase,
        limit: FIND_MATCH_LIMIT,
      });
      const ranges = matches.map((match) => {
        // 定位起始匹配落在哪个文本节点
        let seg = segments.find((s) => match.start < s.start + s.node.data.length) ?? segments.at(-1);
        const nodeStart = seg?.start ?? 0;
        const nodeOffset = match.start - nodeStart;
        const range = document.createRange();
        range.setStart(seg!.node, nodeOffset);
        range.setEnd(seg!.node, nodeOffset + Math.min(match.end - match.start, seg!.node.data.length - nodeOffset));
        return { node: seg!.node, start: match.start, end: match.end, range };
      });
      rangesRef.current = ranges;
      setCount(ranges.length);
      setIndex(ranges.length > 0 ? 0 : -1);
      if (ranges[0]) scrollToRange(ranges[0]!.range);
    },
    [containerRef, matchCase, clearHighlight],
  );

  const scrollToRange = useCallback((range: Range) => {
    clearHighlight();
    try {
      // 只选择/滚动，不改写 DOM（问题12：TreeWalker+Range 定位，selection 高亮）。
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      range.startContainer.parentElement?.scrollIntoView({ block: 'center' });
      currentRangeRef.current = range;
    } catch {
      // 跨节点/只读区 fallback：仅滚动不选中
      try { range.startContainer.parentElement?.scrollIntoView({ block: 'center' }); } catch { /* no-op */ }
    }
  }, [clearHighlight]);

  const navigate = useCallback(
    (direction: 'prev' | 'next') => {
      const next = navigateMatch(index, count, direction);
      setIndex(next);
      const target = rangesRef.current[next];
      if (target) scrollToRange(target.range);
    },
    [index, count, scrollToRange],
  );

  const openFind = useCallback(() => {
    setOpen(true);
    setIndex(-1);
    setCount(0);
    if (query) runFind(query);
  }, [query, runFind]);

  useEffect(() => {
    if (!open) return;
    if (query) runFind(query);
    else { setCount(0); setIndex(-1); }
  }, [query, matchCase, open, runFind]);

  // 卸载清理
  useEffect(() => () => clearHighlight(), [clearHighlight]);

  return {
    open,
    openFind,
    close,
    query,
    setQuery,
    matchCase,
    setMatchCase,
    index,
    count,
    navigate,
  };
}
