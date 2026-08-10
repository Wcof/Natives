/**
 * tauri/types — 共享类型契约（ARCH-002）
 *
 * NativesAPI 与各 domain 数据类型统一声明于此；domain facade 与业务组件
 * 从这里取类型。wire-level 私有类型（Stored 与 Host 系列）与 normalize
 * 函数归 provider facade（./provider.ts），不暴露到 barrel 之外。
 *
 * ARCH-002 split：类型定义按 Host domain 拆分至 ./types-<domain>.ts，
 * 本文件仅做 type composition（聚合 re-export），不声明任何新类型；
 * 对外 import '@/lib/tauri/types' 的路径保持不变。
 */

// ── Terminal ──
export type { RenderStatePayload } from './types-terminal';

// ── Module ──
export type { WriteGeneratedModuleResult } from './types-module';

// ── Creative App（multi-source Personal Creations / T08 browser+grants+OAuth / local）──
export type {
  AppGrant,
  BrowserProfile,
  ComposePlanDetail,
  CreateLocalCreativeRequest,
  CreativeAppActions,
  CreativeAppBrowserBounds,
  CreativeAppDeleteMutationResult,
  CreativeAppDeleteOptions,
  CreativeAppDeleteResult,
  CreativeAppDockerStatus,
  CreativeAppEnvRequirement,
  CreativeAppGithubTokenStatus,
  CreativeAppInspectRequest,
  CreativeAppInspectResult,
  CreativeAppInstallCandidate,
  CreativeAppInstallRequest,
  CreativeAppLogEvent,
  CreativeAppMutationResult,
  CreativeAppOpenTarget,
  CreativeAppOperation,
  CreativeAppOperationKind,
  CreativeAppOperationPhase,
  CreativeAppProgressEvent,
  CreativeAppProgressStage,
  CreativeAppProposal,
  CreativeAppProposalApproveResult,
  CreativeAppProposalPayload,
  CreativeAppProposalRejectResult,
  CreativeAppProposedDriver,
  CreativeAppReleaseTagInfo,
  CreativeAppRuntime,
  CreativeAppSource,
  CreativeAppState,
  CreativeAppStatusDetail,
  CreativeAppSummary,
  CreativeAppSurface,
  CreativeAppValidatedProposal,
  CreativeAppWindow,
  CreativeDraft,
  CreativeDraftPublishResult,
  CreativeGrantRequested,
  GrantEvent,
  LaunchMode,
  LaunchPlan,
  LocalCreativeAiSettings,
  LocalCreativeConfig,
  LocalCreativeIssueCode,
  LocalProjectKind,
  LocalProjectScanResult,
  LocalProjectSummary,
  LocalToolVersions,
  OAuthAllowlistEntry,
  OAuthFlowResult,
  PackageManager,
  ProfileBinding,
  UpdateLocalCreativeRequest,
} from './types-creative-app';

// ── Provider / credential ──
export type {
  ProviderKeySummary,
  ProviderSummary,
  ProviderTestResult,
} from './types-provider';

// ── 工作区 / run ──
export type { HarnessNotice, ProjectSummary } from './types-project';

// ── Execution Engine V2 ──
export type {
  ExecutionEngineSettings,
  ExecutionEngineSnapshot,
  ResolvedDefaultRuntime,
  RuntimeDescriptor,
} from './types-execution';

// ── NativesAPI 大接口 ──
export type { NativesAPI } from './types-api';
