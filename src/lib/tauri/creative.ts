/**
 * tauri/creative — 创意域 facade（ARCH-002）
 *
 * creativeApp（Catalog/本地工程/浏览器/OAuth/Proposal）与 creativeDraft
 * 统一入口；业务组件只允许经本 facade 访问；唯一 raw invoke 在 ./core.ts。
 */

import { cmd, subscribe } from './core';
import type {
  NativesAPI,
  AppGrant,
  BrowserProfile,
  CreateLocalCreativeRequest,
  CreativeAppBrowserBounds,
  CreativeAppDeleteMutationResult,
  CreativeAppDeleteOptions,
  CreativeAppDockerStatus,
  CreativeAppGithubTokenStatus,
  CreativeAppInspectRequest,
  CreativeAppInspectResult,
  CreativeAppInstallRequest,
  CreativeAppLogEvent,
  CreativeAppMutationResult,
  CreativeAppOpenTarget,
  CreativeAppOperation,
  CreativeAppProgressEvent,
  CreativeAppProposal,
  CreativeAppProposalApproveResult,
  CreativeAppProposalPayload,
  CreativeAppProposalRejectResult,
  CreativeAppSummary,
  CreativeAppSurface,
  CreativeAppValidatedProposal,
  CreativeAppWindow,
  CreativeDraft,
  CreativeDraftPublishResult,
  CreativeGrantRequested,
  GrantEvent,
  LaunchPlan,
  OAuthAllowlistEntry,
  OAuthFlowResult,
  PackageManager,
  ProfileBinding,
  UpdateLocalCreativeRequest,
  LocalCreativeAiSettings,
  LocalCreativeConfig,
  LocalCreativeIssueCode,
  LocalProjectScanResult,
} from './types';
import { getHttpPort } from '../natives-http-port';

  // Creative App (multi-source)
export const creativeApp: NativesAPI['creativeApp'] = {
    list: () => cmd<CreativeAppSummary[]>('creative_app_list'),
    start: (id: string) => cmd<CreativeAppMutationResult>('creative_app_start', { id }),
    stop: (id: string) => cmd<CreativeAppMutationResult>('creative_app_stop', { id }),
    delete: (id: string, options?: CreativeAppDeleteOptions) =>
      cmd<CreativeAppDeleteMutationResult>('creative_app_delete', { id, options }),
    getOpenTarget: (id: string) =>
      cmd<CreativeAppOpenTarget>('creative_app_get_open_target', { id }),
    inspectGithub: (request: CreativeAppInspectRequest) =>
      cmd<CreativeAppInspectResult>('creative_app_inspect_github', { request }),
    installGithub: (request: CreativeAppInstallRequest) =>
      cmd<CreativeAppMutationResult>('creative_app_install_github', { request }),
    operations: () => cmd<CreativeAppOperation[]>('creative_app_operations'),
    getOperation: (id: number) =>
      cmd<CreativeAppOperation>('creative_app_operation_get', { id }),
    cancelOperation: (id: number) =>
      cmd<CreativeAppOperation>('creative_app_operation_cancel', { id }),
    onOperationChanged: (callback: (op: CreativeAppOperation) => void) =>
      subscribe<{ channel: string; data: CreativeAppOperation }>(
        'db-state-changed',
        (payload) => {
          if (payload?.channel !== 'creative-operation') return;
          if (payload?.data) callback(payload.data);
        },
      ),
    logs: (id: string, tail?: number, cursor?: number) =>
      cmd<string>('creative_app_logs', { runtimeId: id, tail, cursor }),
    reconcile: () => cmd<number>('creative_app_reconcile'),
    githubTokenStatus: () =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_status'),
    githubTokenSet: (token: string) =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_set', { token }),
    githubTokenClear: () =>
      cmd<CreativeAppGithubTokenStatus>('creative_app_github_token_clear'),
    dockerStatus: () => cmd<CreativeAppDockerStatus>('creative_app_docker_status'),
    browserShow: (appId: string, url: string, bounds: CreativeAppBrowserBounds) =>
      cmd<CreativeAppWindow>('creative_app_browser_show', { appId, url, bounds }),
    browserSetBounds: (appId: string, bounds: CreativeAppBrowserBounds) =>
      cmd('creative_app_browser_set_bounds', { appId, bounds }),
    browserBack: (appId: string) => cmd('creative_app_browser_back', { appId }),
    browserForward: (appId: string) => cmd('creative_app_browser_forward', { appId }),
    browserReload: (appId: string) => cmd('creative_app_browser_reload', { appId }),
    browserHide: (appId: string) => cmd('creative_app_browser_hide', { appId }),
    browserClose: (appId: string) => cmd('creative_app_browser_close', { appId }),
    browserCurrent: (appId: string) =>
      cmd<{ appId?: string | null; url?: string | null }>('creative_app_browser_current', { appId }),
    // T08: BrowserProfile / grants / OAuth
    profileList: () => cmd<BrowserProfile[]>('creative_app_profile_list'),
    profileCreate: (name: string) =>
      cmd<BrowserProfile>('creative_app_profile_create', { name }),
    profileDelete: (profileId: string) =>
      cmd('creative_app_profile_delete', { profileId }),
    profileBindings: () => cmd<ProfileBinding[]>('creative_app_profile_bindings'),
    profileBind: (appId: string, profileId: string) =>
      cmd('creative_app_profile_bind', { appId, profileId }),
    profileUnbind: (appId: string) =>
      cmd('creative_app_profile_unbind', { appId }),
    grantSet: (appId: string, kind: string, policy: string, path?: string | null) =>
      cmd<AppGrant>('creative_app_grant_set', { appId, kind, policy, path: path ?? null }),
    grantList: (appId: string) =>
      cmd<AppGrant[]>('creative_app_grant_list', { appId }),
    grantDelete: (grantId: string) =>
      cmd('creative_app_grant_delete', { grantId }),
    grantEvents: (appId: string, limit?: number) =>
      cmd<GrantEvent[]>('creative_app_grant_events', { appId, limit }),
    uploadFiles: (appId: string) =>
      cmd<string[]>('creative_app_upload_files', { appId }),
    clipboardRead: (appId: string) =>
      cmd<string>('creative_app_clipboard_read', { appId }),
    clipboardWrite: (appId: string, text: string) =>
      cmd('creative_app_clipboard_write', { appId, text }),
    oauthDomains: (appId: string) =>
      cmd<OAuthAllowlistEntry[]>('creative_app_oauth_domains', { appId }),
    oauthStart: (appId: string, authorizeUrl: string) =>
      cmd<OAuthFlowResult>('creative_app_oauth_start', { appId, authorizeUrl }),
    oauthCancel: (flowId: string) =>
      cmd('creative_app_oauth_cancel', { flowId }),
    onGrantRequested: (callback: (event: CreativeGrantRequested) => void) =>
      subscribe<{ channel: string; data: CreativeGrantRequested }>(
        'db-state-changed',
        (payload) => {
          if (payload?.channel !== 'creative-grant-requested') return;
          if (payload?.data) callback(payload.data);
        },
      ),
    // CR-501: Surface / Endpoint / Window
    surfaceList: (applicationId: string) =>
      cmd<CreativeAppSurface[]>('creative_app_surface_list', { applicationId }),
    windowList: (applicationId: string) =>
      cmd<CreativeAppWindow[]>('creative_app_window_list', { applicationId }),
    windowOpen: (applicationId: string, surfaceId: string, url: string, bounds: CreativeAppBrowserBounds) =>
      cmd<CreativeAppWindow>('creative_app_window_open', {
        applicationId,
        surfaceId,
        url,
        bounds,
      }),
    windowClose: (windowId: string) =>
      cmd('creative_app_window_close', { windowId }),
    windowMinimize: (windowId: string) =>
      cmd('creative_app_window_minimize', { windowId }),
    windowRestore: (windowId: string) =>
      cmd('creative_app_window_restore', { windowId }),
    // CR-1001/1002: Agent proposal gate
    proposalList: () =>
      cmd<CreativeAppProposal[]>('creative_app_proposal_list', {}),
    proposalValidate: (proposal: CreativeAppProposalPayload) =>
      cmd<CreativeAppValidatedProposal>('creative_app_proposal_validate', { proposal }),
    proposalApprove: (proposalId: string) =>
      cmd<CreativeAppProposalApproveResult>('creative_app_proposal_approve', { proposalId }),
    proposalReject: (proposalId: string) =>
      cmd<CreativeAppProposalRejectResult>('creative_app_proposal_reject', { proposalId }),
    onProgress: (callback) =>
      subscribe<CreativeAppProgressEvent>('creative-app-progress', (payload) => callback(payload)),
    onLog: (callback) =>
      subscribe<CreativeAppLogEvent>('creative-app-log', (payload) => callback(payload)),
    inspectLocal: (request: { projectRoot: string }) =>
      cmd<LocalProjectScanResult>('creative_app_inspect_local', { request }),
    createLocal: (request: CreateLocalCreativeRequest) =>
      cmd<CreativeAppSummary>('creative_app_create_local', { request }),
    updateLocal: (request: UpdateLocalCreativeRequest) =>
      cmd<CreativeAppSummary>('creative_app_update_local', { request }),
    rescanLocal: (id: string) =>
      cmd<LocalProjectScanResult>('creative_app_rescan_local', { id }),
    restart: (id: string) => cmd<CreativeAppMutationResult>('creative_app_restart', { id }),
    resolveOrphan: (id: string, restart: boolean) =>
      cmd<CreativeAppSummary>('creative_app_resolve_orphan', { id, restart }),
    getLocalLogs: (id: string, limit?: number) =>
      cmd<Array<{ seq: number; tsMs: number; stream: string; text: string }>>(
        'creative_app_get_local_logs',
        { id, limit },
      ),
    installLocalDependencies: (id: string) =>
      cmd<CreativeAppSummary>('creative_app_install_local_dependencies', { id }),
    previewLocalDependencyInstall: (id: string) =>
      cmd<{
        program: string;
        args: string[];
        packageManager: PackageManager;
        requiresConfirmation: true;
        display: string;
      }>('creative_app_preview_local_dependency_install', { id }),
    getLocalAiSettings: () =>
      cmd<LocalCreativeAiSettings>('creative_app_get_local_ai_settings'),
    saveLocalAiSettings: (settings: LocalCreativeAiSettings) =>
      cmd<LocalCreativeAiSettings>('creative_app_save_local_ai_settings', { settings }),
    previewLocalAi: (projectRoot: string) =>
      cmd<{
        scan: LocalProjectScanResult;
        payloadPreview: unknown;
        settings: LocalCreativeAiSettings;
        requiresConfirmation: true;
      }>('creative_app_preview_local_ai', { projectRoot }),
    analyzeLocalWithAi: (projectRoot: string, confirmed: boolean) =>
      cmd<{
        scan: LocalProjectScanResult;
        aiPlan?: LaunchPlan | null;
        payloadPreview: unknown;
      }>('creative_app_analyze_local_with_ai', { projectRoot, confirmed }),
    diagnoseLocalWithAi: (id: string) =>
      cmd<{
        schemaVersion: number;
        issueCode: LocalCreativeIssueCode;
        summary: string;
        recoveryActions: string[];
      }>('creative_app_diagnose_local_with_ai', { id }),
    getLocalConfig: (id: string) =>
      cmd<LocalCreativeConfig>('creative_app_get_local_config', { id }),
    pollLocalExits: () => cmd<number>('creative_app_poll_local_exits'),
};

  // Creative drafts — the creation loop before a module exists
export const creativeDraft: NativesAPI['creativeDraft'] = {
    create: (request) => cmd<CreativeDraft>('create_creative_draft', request),
    list: () => cmd<CreativeDraft[]>('list_creative_drafts'),
    get: (draftId: string) => cmd<CreativeDraft>('get_creative_draft', { draftId }),
    read: (draftId: string) =>
      cmd<{ draftId: string; html: string }>('read_creative_draft', { draftId }),
    bindConversation: (draftId: string, conversationId: string) =>
      cmd<{ ok: boolean; draftId: string }>('bind_creative_draft_conversation', {
        draftId,
        conversationId,
      }),
    rollback: (draftId: string) =>
      cmd<{ draftId: string; revision: number }>('rollback_creative_draft', { draftId }),
    publish: (request) =>
      cmd<CreativeDraftPublishResult>('publish_creative_draft', request),
    delete: (draftId: string) =>
      cmd<{ ok: boolean; draftId: string }>('delete_creative_draft', { draftId }),
    previewUrl: async (draftId: string) => {
      const port = await getHttpPort();
      return `http://localhost:${port}/drafts/${draftId}/`;
    },
};

