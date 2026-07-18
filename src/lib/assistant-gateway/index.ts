export type { AssistantGateway, GatewayFactory } from './gateway';
export { FixtureAssistantAdapter, type FixtureScenario } from './fixture-adapter';
export { DaemonAssistantAdapter, type DaemonAdapterOptions } from './daemon-adapter';

import { DaemonAssistantAdapter } from './daemon-adapter';
import { FixtureAssistantAdapter } from './fixture-adapter';
import type { AssistantGateway } from './gateway';

/** Prefer real daemon. Fixture is allowed only when explicitly requested. */
export function createDefaultGateway(preferFixture = false): AssistantGateway {
  if (preferFixture) return new FixtureAssistantAdapter({ id: 'default' });
  return new DaemonAssistantAdapter();
}
