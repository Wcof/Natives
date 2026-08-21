'use client';

/**
 * Greeting Widget（B-021）—— 迁移到 V2 Definition。
 * surfacePolicy: plain 优先（bare policy 支持），低干扰默认。
 * load 复用 settings:username（greeting adapter）。
 */

import { z } from 'zod';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE } from '@/lib/design-tokens';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import { loadGreeting, greetingAdapterKey } from '@/lib/workspace/widgets/adapters/greeting';
import type { GreetingData } from '@/lib/workspace/widgets/adapters/greeting';

type GreetingSettings = Record<string, unknown>;

function GreetingView({ data }: WidgetProps<GreetingData, GreetingSettings>) {
  const locale = useLocale();
  const name = data?.username ?? null;
  return (
    <div
      className="ws-greeting"
      style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        gap: 4,
        height: '100%',
        padding: '0 12px',
        minWidth: 0,
      }}
    >
      <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', lineHeight: 1.3 }}>
        {t(locale, 'home.greeting', { name: name ?? t(locale, 'home.guest') })}
      </div>
      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', lineHeight: 1.4 }}>
        {t(locale, 'home.greetingSub')}
      </div>
    </div>
  );
}

export const greetingWidgetDefinition: WidgetDefinition<GreetingData, GreetingSettings> = {
  type: 'greeting',
  titleKey: 'home.widgetGreeting',
  descriptionKey: 'home.widgetGreeting',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['plain', 'material'], allowBlur: false, allowGlow: false },
  adapterKeyBuilder: greetingAdapterKey,
  load: loadGreeting,
  Component: GreetingView,
};

export default greetingWidgetDefinition;
