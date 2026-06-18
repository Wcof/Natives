'use client';

import React from 'react';
import { getBadgeExt, EXT_BADGES, KIND_COLORS } from '@/lib/file-badges';
import type { FileEntry, FileKind } from '@/types/file';

/* ── SVG 图标系统（移植自 fanbox，强辨识度专属图形）── */

interface IconProps {
  size?: number;
  color?: string;
  className?: string;
  style?: React.CSSProperties;
}

function SvgWrap({ children, size, style, viewBox = '0 0 24 24' }: IconProps & { children: React.ReactNode; viewBox?: string }) {
  return (
    <svg
      width={size ?? 24}
      height={size ?? 24}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ flexShrink: 0, ...style }}
    >
      {children}
    </svg>
  );
}

// 文件夹：开口文件夹轮廓
export function FbFolder({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.dir, ...style }}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </SvgWrap>
  );
}

// 通用文件：折角文档
export function FbFile({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.other, ...style }}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
    </SvgWrap>
  );
}

// 文本文件：带文字行
export function FbText({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.text, ...style }}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="8" y1="13" x2="16" y2="13" />
      <line x1="8" y1="17" x2="14" y2="17" />
    </SvgWrap>
  );
}

// 代码：尖括号
export function FbCode({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.text, ...style }}>
      <polyline points="16 18 22 12 16 6" />
      <polyline points="8 6 2 12 8 18" />
    </SvgWrap>
  );
}

// 图片：带山脉的风景画框
export function FbImage({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.image, ...style }}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="8.5" cy="8.5" r="1.5" />
      <polyline points="21 15 16 10 5 21" />
    </SvgWrap>
  );
}

// 视频：胶片 + 播放按钮
export function FbVideo({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.video, ...style }}>
      <polygon points="23 7 16 12 23 17 23 7" />
      <rect x="1" y="5" width="15" height="14" rx="2" />
    </SvgWrap>
  );
}

// 音频：音符 + 声波
export function FbAudio({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.audio, ...style }}>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </SvgWrap>
  );
}

// PDF：文件 + 专用标记
export function FbPdf({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.pdf, ...style }}>
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
      <line x1="9" y1="8" x2="15" y2="8" />
      <line x1="9" y1="12" x2="15" y2="12" />
    </SvgWrap>
  );
}

// 数据表：表格网格
export function FbData({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.text, ...style }}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <line x1="3" y1="9" x2="21" y2="9" />
      <line x1="3" y1="15" x2="21" y2="15" />
      <line x1="12" y1="3" x2="12" y2="21" />
    </SvgWrap>
  );
}

// JSON：花括号引号
export function FbJson({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? '#e8c95b', ...style }}>
      <path d="M8 3H7a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2 2 2 0 0 1 2 2v5a2 2 0 0 0 2 2h1" />
      <path d="M16 3h1a2 2 0 0 1 2 2v5a2 2 0 0 1 2 2 2 2 0 0 1-2 2v5a2 2 0 0 1-2 2h-1" />
    </SvgWrap>
  );
}

// 归档/压缩包：盒子 + 拉链
export function FbArchive({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.archive, ...style }}>
      <polyline points="21 8 21 21 3 21 3 8" />
      <rect x="1" y="3" width="22" height="5" />
      <line x1="10" y1="12" x2="14" y2="12" />
    </SvgWrap>
  );
}

// Markdown：带格式标记的文档
export function FbMarkdown({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? KIND_COLORS.text, ...style }}>
      <rect x="2.5" y="5" width="19" height="14" rx="2" />
      <path d="M6 15.5V9l3 3 3-3v6.5" />
      <path d="M17 9v4.5" />
      <path d="M14.8 12.5L17 15l2.2-2.5" />
    </SvgWrap>
  );
}

// HTML：带角括号标签的文档
export function FbHtml({ size = 24, color, style }: IconProps) {
  return (
    <SvgWrap size={size} style={{ color: color ?? '#e87b5b', ...style }}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <polyline points="9.3 12.5 7.5 14.5 9.3 16.5" />
      <polyline points="14.7 12.5 16.5 14.5 14.7 16.5" />
    </SvgWrap>
  );
}

/* ── 主调度函数 ── */

// 扩展名 → 图标映射（比 kind 更精准）
const ICON_BY_EXT: Record<string, React.ComponentType<IconProps>> = {
  md: FbMarkdown,
  markdown: FbMarkdown,
  html: FbHtml,
  htm: FbHtml,
};

// 扩展名 → 图标类别（code 用 <>
const EXT_ICON_KIND: Record<string, React.ComponentType<IconProps>> = {
  js: FbCode, mjs: FbCode, cjs: FbCode, jsx: FbCode,
  ts: FbCode, tsx: FbCode, py: FbCode, go: FbCode,
  rs: FbCode, swift: FbCode, java: FbCode, rb: FbCode,
  c: FbCode, cpp: FbCode, h: FbCode, php: FbCode,
  vue: FbCode, sh: FbCode, bash: FbCode, lua: FbCode,
  css: FbCode, scss: FbCode, sass: FbCode, less: FbCode,
  xml: FbCode,
  json: FbJson, json5: FbJson, yml: FbJson, yaml: FbJson,
  toml: FbJson, ini: FbJson, env: FbJson,
  csv: FbData, tsv: FbData, sql: FbData,
};

/**
 * 根据文件的 kind 和扩展名选取合适的 fanbox 风格图标。
 * 优先顺序：专属扩展名(md/html) > code 类 > kind 类
 */
export function getFileIcon(entry: Pick<FileEntry, 'name' | 'kind' | 'isDir'>): React.ComponentType<IconProps> {
  if (entry.isDir) return FbFolder;
  const ext = getBadgeExt(entry.name);
  const lowerExt = ext.toLowerCase();
  // 专属扩展名优先
  if (ICON_BY_EXT[lowerExt]) return ICON_BY_EXT[lowerExt];
  // 代码/数据类
  if (EXT_ICON_KIND[lowerExt]) return EXT_ICON_KIND[lowerExt];
  // kind 类
  switch (entry.kind) {
    case 'text': return FbText;
    case 'image': return FbImage;
    case 'video': return FbVideo;
    case 'audio': return FbAudio;
    case 'pdf': return FbPdf;
    case 'archive': return FbArchive;
    default: return FbFile;
  }
}

/**
 * 获取文件图标应使用的颜色（优先用扩展名徽章色）
 */
export function getIconColor(entry: Pick<FileEntry, 'name' | 'kind' | 'isDir'>): string {
  if (entry.isDir) return KIND_COLORS.dir;
  const ext = getBadgeExt(entry.name);
  const lowerExt = ext.toLowerCase();
  if (EXT_BADGES[lowerExt]) return EXT_BADGES[lowerExt].bg;
  if (ICON_BY_EXT[lowerExt]) {
    switch (lowerExt) {
      case 'md': case 'markdown': return '#3B82F6';
      case 'html': case 'htm': return '#E87B5B';
    }
  }
  return KIND_COLORS[entry.kind] || KIND_COLORS.other;
}
