export type ProviderRouteCredential =
  | { kind: 'api_key'; keyId: string }
  | { kind: 'sub2api_pool' };

export interface ProviderRouteBinding {
  id: string;
  providerId: string;
  modelId: string;
  credential: ProviderRouteCredential;
  priority: number;
  enabled: boolean;
}

export interface ProviderRoutingSettings {
  enabled: boolean;
  loopbackEnabled: boolean;
  loopbackPort: number;
  rectifierEnabled: boolean;
  outboundProxyEnabled: boolean;
  outboundProxyUrl: string | null;
}

export type Sub2ApiAccountStatus = 'active' | 'paused' | 'expired' | 'invalid';

/** Deliberately excludes credentials and proxy passwords. */
export interface Sub2ApiAccountSummary {
  id: string;
  providerId: string;
  name: string;
  email: string | null;
  platform: string;
  accountType: string;
  status: Sub2ApiAccountStatus;
  priority: number;
  concurrency: number;
  expiresAt: string | null;
}

export interface Sub2ApiImportPreviewItem {
  index: number;
  name: string;
  email: string | null;
  platform: string;
  accountType: string;
  action: 'create' | 'update' | 'skip' | 'reject';
  reason: string | null;
}

export interface Sub2ApiImportPreview {
  items: Sub2ApiImportPreviewItem[];
  rejected: number;
}

export interface Sub2ApiImportCommitResult {
  created: number;
  updated: number;
  skipped: number;
  failed: number;
}

export interface ProviderRoutingApi {
  getSettings?: () => Promise<ProviderRoutingSettings>;
  saveSettings?: (settings: ProviderRoutingSettings) => Promise<ProviderRoutingSettings>;
  /** Returns the newly generated token once; it is never readable afterwards. */
  rotateLoopbackToken?: () => Promise<string>;
  listBindings?: () => Promise<ProviderRouteBinding[]>;
  saveBindings?: (bindings: ProviderRouteBinding[]) => Promise<ProviderRouteBinding[]>;
  listSub2ApiAccounts?: (providerId: string) => Promise<Sub2ApiAccountSummary[]>;
  previewSub2ApiImport?: (input: { providerId: string; content: string }) => Promise<Sub2ApiImportPreview>;
  commitSub2ApiImport?: (input: { providerId: string; content: string }) => Promise<Sub2ApiImportCommitResult>;
  deleteSub2ApiAccounts?: (input: { providerId: string; accountIds: string[] }) => Promise<{ deleted: string[]; notFound: string[]; failed: string[] }>;
  createSub2ApiPool?: (input: { name: string }) => Promise<string>;
}
