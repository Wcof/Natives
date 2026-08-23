'use client';

/**
 * Pinned Quick Links Widget (B-034).
 * Fast navigation to pinned project docs and web portals.
 */

import { z } from 'zod';
import { useLocale, t } from '@/i18n';
import { ExternalLink, Globe, BookOpen, GitBranch } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';

interface QuickLinksData {
  links: Array<{ id: string; label: string; url: string; icon: 'docs' | 'git' | 'web' }>;
}

type QuickLinksSettings = Record<string, unknown>;

/** 默认链接（label 文案走 i18n：workspace.link* 键；url 不变）。 */
const DEFAULT_LINKS: Array<{ id: string; labelKey: string; url: string; icon: QuickLinksData['links'][number]['icon'] }> = [
  { id: '1', labelKey: 'workspace.linkLocalDocs', url: 'http://localhost:3000', icon: 'docs' },
  { id: '2', labelKey: 'workspace.linkRepoTree', url: 'https://github.com', icon: 'git' },
  { id: '3', labelKey: 'workspace.linkReferenceMatrix', url: 'docs/README.md', icon: 'web' },
];

function buildDefaultLinks(locale: string): QuickLinksData['links'] {
  return DEFAULT_LINKS.map((link) => ({ id: link.id, label: t(locale, link.labelKey), url: link.url, icon: link.icon }));
}

function QuickLinksView(_props: WidgetProps<QuickLinksData, QuickLinksSettings>) {
  const locale = useLocale();
  // 默认链接列表随 locale 渲染（组件级默认值，不写回持久化结构）。
  const links = buildDefaultLinks(locale);
  const getIcon = (type: string) => {
    switch (type) {
      case 'docs':
        return <BookOpen size={13} className="text-[var(--primary)] shrink-0" />;
      case 'git':
        return <GitBranch size={13} className="text-[var(--primary)] shrink-0" />;
      default:
        return <Globe size={13} className="text-[var(--primary)] shrink-0" />;
    }
  };

  return (
    <div className="flex h-full flex-col justify-between p-2.5">
      <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)] mb-1">
        <ExternalLink size={14} className="text-[var(--primary)]" />
        <span className="font-medium">{t(locale, 'workspace.linksTitle')}</span>
      </div>
      <div className="space-y-1 overflow-y-auto min-h-0 flex-1">
        {links.map((link) => (
          <a
            key={link.id}
            href={link.url}
            target="_blank"
            rel="noreferrer"
            className="flex items-center justify-between rounded-md bg-[var(--surface-hover)] p-1.5 text-xs hover:bg-[var(--primary-soft)] transition-colors text-[var(--text)]"
          >
            <div className="flex items-center gap-2 truncate">
              {getIcon(link.icon)}
              <span className="truncate">{link.label}</span>
            </div>
            <ExternalLink size={11} className="text-[var(--text-disabled)] shrink-0" />
          </a>
        ))}
      </div>
    </div>
  );
}

export const quickLinksWidgetDefinition: WidgetDefinition<QuickLinksData, QuickLinksSettings> = {
  type: 'quick_links',
  titleKey: 'common.links',
  descriptionKey: 'common.links',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: () => 'workspace.links:local',
  // 本地数据源：按当前持久化 locale 提供默认链接列表（默认 zh，R-I5）。
  load: async () => {
    const saved = typeof window !== 'undefined' ? await window.nativesAPI?.getLocale?.().catch(() => null) : null;
    return { links: buildDefaultLinks(saved === 'en' ? 'en' : 'zh') };
  },
  Component: QuickLinksView,
};

export default quickLinksWidgetDefinition;
