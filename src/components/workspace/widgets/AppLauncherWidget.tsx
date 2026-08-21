'use client';

/**
 * App Launcher Widget（B-023）—— 迁移到 V2 Definition。
 * load 复用 creativeApp.list()（现有 facade），不复制 Apps domain。
 */

import { z } from 'zod';
import { Box } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadAppLauncher,
  appLauncherAdapterKey,
} from '@/lib/workspace/widgets/adapters/apps';
import type { AppLauncherData } from '@/lib/workspace/widgets/adapters/apps';

type AppLauncherSettings = Record<string, unknown>;

function AppLauncherView({ data, loading }: WidgetProps<AppLauncherData, AppLauncherSettings>) {
  const locale = useLocale();
  const apps = data?.apps ?? [];

  if (loading && apps.length === 0) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (data?.unavailable) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'home.appsUnavailable')}
      </div>
    );
  }

  if (apps.length === 0) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'home.noApps')}
      </div>
    );
  }

  return (
    <ul
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
        height: '100%',
        overflowY: 'auto',
        padding: '4px 8px',
        margin: 0,
        listStyle: 'none',
      }}
    >
      {apps.map((app) => (
        <li key={app.id}>
          <div
            title={app.title}
            style={{
              display: 'flex',
              width: '100%',
              alignItems: 'center',
              gap: SPACING.sm,
              padding: '4px 8px',
              borderRadius: 8,
              fontSize: FONT_SIZE.xs,
              color: 'var(--text-secondary)',
            }}
          >
            <Box size={13} style={{ flexShrink: 0, color: 'var(--text-tertiary)' }} />
            <span
              style={{
                minWidth: 0,
                flex: 1,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {app.title}
            </span>
            {app.state === 'running' ? (
              <span
                style={{
                  flexShrink: 0,
                  borderRadius: 4,
                  padding: '2px 6px',
                  background: 'var(--primary-soft)',
                  color: 'var(--primary)',
                  fontSize: 10,
                }}
              >
                {t(locale, 'home.running')}
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  );
}

export const appLauncherWidgetDefinition: WidgetDefinition<AppLauncherData, AppLauncherSettings> = {
  type: 'app_launcher',
  titleKey: 'home.widgetAppLauncher',
  descriptionKey: 'home.widgetAppLauncher',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'medium',
  surfacePolicy: { surfaces: ['plain', 'material'], allowBlur: false, allowGlow: false },
  adapterKeyBuilder: appLauncherAdapterKey,
  load: loadAppLauncher,
  Component: AppLauncherView,
};

export default appLauncherWidgetDefinition;
