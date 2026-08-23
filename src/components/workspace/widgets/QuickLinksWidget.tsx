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

interface QuickLinksSettings extends Record<string, unknown> { links: QuickLinksData['links'] }

function QuickLinksView({ config }: WidgetProps<QuickLinksData, QuickLinksSettings>) {
  const locale = useLocale();
  const links = config.settings?.links ?? [];
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
    <div className="flex h-full w-full flex-col justify-between">
      <div className="space-y-1 overflow-y-auto min-h-0 flex-1">
        {links.length === 0 && <div className="ws-shell-state text-xs text-[var(--text-tertiary)]">{t(locale, 'workspace.quickLinksEmpty')}</div>}
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
  defaultConfig: { links: [] },
  configSchema: z.object({ links: z.array(z.object({ id: z.string(), label: z.string(), url: z.string().url(), icon: z.enum(['docs','git','web']) })).max(20) }),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  Component: QuickLinksView,
};

export default quickLinksWidgetDefinition;
