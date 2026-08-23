'use client';

/**
 * Workspace Notes Widget (B-034).
 * Fast scratchpad for workspace context notes.
 */

import { useState } from 'react';
import { useLocale, t } from '@/i18n';
import { z } from 'zod';
import { StickyNote } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';

interface NotesData {
  content: string;
}

type NotesSettings = {
  placeholder?: string;
};

function NotesView(_props: WidgetProps<NotesData, NotesSettings>) {
  const locale = useLocale();
  // 初始文案走 i18n（仅在挂载时取一次默认值；用户输入后以本地 state 为准）。
  const [text, setText] = useState(() => t('zh', 'workspace.notesInitialContent'));

  return (
    <div className="flex h-full flex-col p-2.5">
      <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)] mb-1.5">
        <StickyNote size={14} className="text-[var(--primary)]" />
        <span className="font-medium">{t(locale, 'workspace.notesTitle')}</span>
      </div>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        className="flex-1 w-full resize-none rounded-md bg-[var(--surface-hover)] p-2 text-xs text-[var(--text)] border-none focus:outline-none focus:ring-1 focus:ring-[var(--primary)]"
        placeholder={t(locale, 'workspace.notesPlaceholder')}
      />
    </div>
  );
}

export const notesWidgetDefinition: WidgetDefinition<NotesData, NotesSettings> = {
  type: 'notes',
  titleKey: 'common.note',
  descriptionKey: 'common.note',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: () => 'workspace.notes:local',
  load: async () => ({ content: '' }),
  Component: NotesView,
};

export default notesWidgetDefinition;
