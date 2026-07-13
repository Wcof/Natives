export interface AssistantRunEvent { runId: string; sequence: number; timestamp?: string; type: string; payload: Record<string, unknown>; }
export interface AssistantFileChange { path: string; change: string; }
