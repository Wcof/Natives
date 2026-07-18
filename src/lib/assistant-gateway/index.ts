export type { AssistantGateway, GatewayFactory } from './gateway';
export { FixtureAssistantAdapter, type FixtureScenario } from './fixture-adapter';
export { DaemonAssistantAdapter, type DaemonAdapterOptions } from './daemon-adapter';

import { DaemonAssistantAdapter } from './daemon-adapter';
import { FixtureAssistantAdapter } from './fixture-adapter';
import type { AssistantGateway } from './gateway';

/** Prefer real daemon when assistantV2 is present; otherwise Fixture for browser/dev. */
export function createDefaultGateway(preferFixture = false): AssistantGateway {
  if (preferFixture) return new FixtureAssistantAdapter({ id: 'default' });
  if (typeof window !== 'undefined' && window.nativesAPI?.assistantV2?.request) {
    return new DaemonAssistantAdapter();
  }
  return new FixtureAssistantAdapter({ id: 'browser-fallback' });
}
