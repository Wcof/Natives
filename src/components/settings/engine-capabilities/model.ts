'use client';

import type { Dispatch, SetStateAction } from 'react';
import type { Locale } from '@/i18n';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import { classifyError } from '@/lib/error-classifier';

export const EXTENSION_DISCOVERY_STATUS = 'discovered_not_executable' as const;

export type EngineCapabilitySnapshot = {
  selectionActive?: boolean;
  agentProfileId?: string | null;
  teamId?: string | null;
  teamMembers?: string[] | null;
  skillIds?: string[];
  mcpServers?: string[];
};

export type EngineCapabilityRun = {
  id: string;
  status: string;
  provider_id: string;
  model_id: string;
  permission_profile: string;
  runtime_id?: string | null;
  agent_profile_id?: string | null;
  capability_snapshot?: EngineCapabilitySnapshot | null;
};

export type Loadable<T> =
  | { phase: 'idle' }
  | { phase: 'loading' }
  | { phase: 'success'; data: T }
  | { phase: 'error'; message: string }
  | { phase: 'unavailable' };

export type LoadSectionInput<T> = {
  advertised: boolean;
  locale: Locale;
  loader: () => Promise<T>;
  setState: Dispatch<SetStateAction<Loadable<T>>>;
  isCurrent: () => boolean;
};

export async function loadSection<T>({
  advertised,
  locale,
  loader,
  setState,
  isCurrent,
}: LoadSectionInput<T>): Promise<void> {
  if (!isCurrent()) return;
  if (!advertised) {
    setState({ phase: 'unavailable' });
    return;
  }
  setState({ phase: 'loading' });
  try {
    const data = await loader();
    if (isCurrent()) setState({ phase: 'success', data });
  } catch (error) {
    if (isCurrent()) {
      setState({
        phase: 'error',
        message: classifyError(error, { locale }).userMessage,
      });
    }
  }
}

export const initialLoadable = <T,>(): Loadable<T> => ({ phase: 'idle' });
