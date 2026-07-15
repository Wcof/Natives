export interface AssistantRpcEnvelope<T> {
  success: boolean;
  data?: T;
  error?: { code: string; message: string };
}

export function unwrapAssistantRpc<T>(response: AssistantRpcEnvelope<T>): T {
  if (!response.success) {
    throw new Error(response.error?.message ?? 'Assistant request failed');
  }
  return response.data as T;
}
