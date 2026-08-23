'use client';

import { createContext, useContext } from 'react';
import type { TimeRange } from './types';

export const TimeRangeContext = createContext<TimeRange>('7d');

export function useWorkspaceTimeRange(): TimeRange {
  return useContext(TimeRangeContext);
}
