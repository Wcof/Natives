import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';

test('EngineCapabilitiesPanel uses createDefaultGateway + capability-admin only', () => {
  const src = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/EngineCapabilitiesPanel.tsx'),
    'utf8',
  );
  assert.match(src, /createDefaultGateway/);
  assert.match(src, /listCapabilitySkills/);
  assert.match(src, /listCapabilityMcpServers/);
  assert.match(src, /listCapabilityExperts/);
  assert.match(src, /listCapabilityTeams/);
  assert.match(src, /listExtensions/);
  assert.match(src, /getRateLimit/);
  assert.match(src, /updateRateLimit/);
  assert.doesNotMatch(src, /loadCapabilityAdminDashboard/);
  assert.match(src, /data-testid="engine-capabilities-panel"/);
  assert.equal(/\.streamChat\s*\(/.test(src), false);
  assert.equal(/cmd\(\s*['"]stream_chat['"]/.test(src), false);
});

test('EngineCapabilitiesPanel is a run evidence projection, not a scheduler inventory', () => {
  const src = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/EngineCapabilitiesPanel.tsx'),
    'utf8',
  );
  assert.match(src, /selectedRun/);
  assert.match(src, /runSnapshot/);
  assert.match(src, /getCapabilities/);
  assert.match(src, /hasMethod/);
  assert.match(src, /listCapabilitySkills/);
  assert.match(src, /listCapabilityMcpServers/);
  assert.match(src, /classifyError/);
  assert.match(src, /capability_snapshot/);
  assert.match(src, /effective_prompt_hash/);
  assert.match(src, /prompt_plan\.layers/);
  assert.match(src, /tool_plan\.canonical_hash/);
  assert.match(src, /expert\.systemPrompt/);
  assert.match(src, /settings\.engineCapabilities\.systemPrompt/);
  assert.match(src, /settings\.engineCapabilities\.teamDetail/);
  assert.match(src, /discovered_not_executable/);
  assert.match(src, /href="\/jobs"/);
  assert.match(src, /Loadable/);
  assert.doesNotMatch(src, /listScheduler/);
  assert.doesNotMatch(src, /scheduler\.list/);
  assert.doesNotMatch(src, /SchedulerAdminSnapshot/);
});

test('Engine capability evidence types and bilingual copy cover the run projection', () => {
  const canvasModel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/nativeExecutionCanvasModel.ts'),
    'utf8',
  );
  const workspace = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  const en = fs.readFileSync(path.join(process.cwd(), 'src/i18n/en.ts'), 'utf8');
  const zh = fs.readFileSync(path.join(process.cwd(), 'src/i18n/zh.ts'), 'utf8');

  assert.match(canvasModel, /effective_prompt_hash/);
  assert.match(canvasModel, /PromptLayerEvidence/);
  assert.match(workspace, /permission_profile/);
  assert.match(workspace, /runtime_id/);
  assert.match(workspace, /capability_snapshot/);
  assert.match(en, /engineCapabilities:\s*\{/);
  assert.match(zh, /engineCapabilities:\s*\{/);
});

test('Settings exposes one execution-engine entry containing capabilities and Harness', () => {
  const nav = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/settings-navigation.ts'),
    'utf8',
  );
  const page = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/SettingsPage.tsx'),
    'utf8',
  );
  const sidebar = fs.readFileSync(
    path.join(process.cwd(), 'src/components/shell/Sidebar.tsx'),
    'utf8',
  );
  const workspace = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  assert.match(nav, /section === 'engineering' \|\| section === 'engine'/);
  assert.match(page, /case 'runtime'/);
  assert.match(page, /NativeHarnessPanel/);
  assert.doesNotMatch(page, /case 'engine'/);
  assert.match(sidebar, /id: 'runtime'/);
  assert.doesNotMatch(sidebar, /id: 'engineering'/);
  assert.doesNotMatch(sidebar, /id: 'engine'/);
  assert.match(workspace, /EngineCapabilitiesPanel/);
  assert.match(workspace, /selectedRun=/);
  assert.match(workspace, /runSnapshot=/);
  assert.match(workspace, /workspaceTarget === 'runs'/);
  assert.match(workspace, /workspaceTarget === 'capabilities'/);
  assert.match(workspace, /loadRuns/);
});

test('Harness detail keeps run and editors inside the controlled canvas', () => {
  const panel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  const canvas = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeExecutionCanvas.tsx'),
    'utf8',
  );
  const inspector = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeExecutionInspector.tsx'),
    'utf8',
  );

  assert.match(panel, /const \[canvasMode, setCanvasMode\] = useState<CanvasMode>\('understand'\)/);
  assert.match(panel, /mode=\{canvasMode\}/);
  assert.match(panel, /runPanel=\{runPanel\}/);
  assert.match(panel, /workspacePanel=\{workspacePanel\}/);
  assert.match(panel, /readOnly=\{readOnly\}/);
  assert.match(canvas, /mode:\s*CanvasMode/);
  assert.doesNotMatch(canvas, /useState<CanvasMode>/);
  assert.match(canvas, /\{runPanel\}/);
  assert.match(canvas, /\{workspacePanel \? <div/);
  assert.match(inspector, /item !== 'configure' \|\| !readOnly/);
});

test('Harness run start requires explicit project/provider/model instead of copying a global run', () => {
  const panel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );

  assert.match(panel, /nativesAPI\.provider\.list\(\)/);
  assert.match(panel, /engineEngineeringRunConfigRequired/);
  assert.match(panel, /engineEngineeringRunTrialTitle/);
  assert.match(panel, /engineEngineeringRunTrialDesc/);
  assert.match(panel, /provider_id: runProviderId/);
  assert.match(panel, /model_id: runModelId/);
  assert.match(panel, /project_path: runProjectPath/);
  assert.match(panel, /setWorkspaceTarget\('runs'\)/);
  assert.match(panel, /ACTIVE_RUN_STATUSES/);
  assert.match(panel, /window\.setInterval\(\(\) => void loadRuns\(selectedRunId\), 2_000\)/);
  assert.doesNotMatch(panel, /const last = liveRuns\[0\]/);
});

test('Harness list opens create as a separate view and preview stays read-only', () => {
  const panel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );

  assert.match(panel, /type DetailMode = 'preview' \| 'edit' \| 'create'/);
  assert.match(panel, /if \(detailMode === 'create'\)/);
  assert.match(panel, /onClick=\{openCreateProfile\}/);
  assert.match(panel, /const readOnly = detailMode === 'preview'/);
  assert.match(panel, /<fieldset disabled=\{readOnly\}/);
});

test('Node inspector exposes per-node hooks, prompts, tools, and subagents', () => {
  const panel = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeHarnessPanel.tsx'),
    'utf8',
  );
  const canvas = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeExecutionCanvas.tsx'),
    'utf8',
  );
  const inspector = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeExecutionInspector.tsx'),
    'utf8',
  );
  const graph = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/NativeExecutionGraph.tsx'),
    'utf8',
  );
  const model = fs.readFileSync(
    path.join(process.cwd(), 'src/components/settings/nativeExecutionCanvasModel.ts'),
    'utf8',
  );

  assert.match(model, /export type CanvasNodeDetail/);
  assert.match(panel, /const nodeDetails = useMemo<Record<string, CanvasNodeDetail>>/);
  assert.match(panel, /nodeDetails=\{nodeDetails\}/);
  assert.match(canvas, /nodeDetails=\{nodeDetails\}/);
  assert.match(canvas, /runPathStages/);
  assert.match(canvas, /engineCanvasRunPathSummary/);
  assert.match(canvas, /setSelectedStageId\(stage\.id\)/);
  assert.match(graph, /type CanvasNodeDetail/);
  assert.match(graph, /detailBadges/);
  assert.match(graph, /engineCanvasNodeResultCount/);
  assert.match(graph, /engineCanvasNodeHookCount/);
  assert.match(graph, /engineCanvasNodeToolCount/);
  assert.match(graph, /engineCanvasNodeSubagentCount/);
  assert.match(graph, /run-arrow/);
  assert.match(canvas, /engineCanvasLegendRunPath/);
  assert.match(panel, /onSetHookEnabled=\{setNodeHookEnabled\}/);
  assert.match(panel, /onAuthorizeHook=\{authorizeNodeHook\}/);
  assert.match(panel, /onRemovePrompt=\{removeNodePrompt\}/);
  assert.match(panel, /Harness Draft.*hook\.adapter\.type/);
  assert.match(panel, /authorized: hook\.adapter\.type === 'command' \? hook\.trust_confirmed : true/);
  assert.match(panel, /engineCanvasHookAuthorizationHarness/);
  assert.match(panel, /engineCanvasHookAuthorizationImported/);
  assert.match(panel, /authorized: true/);
  assert.match(panel, /engineCanvasRemoveHookBinding/);
  assert.match(panel, /overlay\?\.enabled \?\? hook\.enabled/);
  assert.match(panel, /hook\.source\.scope.*hook\.source\.origin/);
  assert.match(inspector, /hook\.authorization/);
  assert.match(inspector, /hook\.removeLabel/);
  assert.match(panel, /builtin_prompt_replacements: document\.builtin_prompt_replacements\.filter/);
  assert.match(panel, /if \(imported\) updateOverlay\(imported, \{ enabled: false \}\)/);
  assert.match(canvas, /detail=\{nodeDetails\[selectedStage\.id\]\}/);
  assert.match(inspector, /CurrentNodeDetail/);
  assert.match(inspector, /stageName=\{presentation\.title\}/);
  assert.match(inspector, /engineCanvasNodeDetails'.*stage: stageName/);
  assert.match(inspector, /canConfigureHooks/);
  assert.match(inspector, /canConfigurePrompts/);
  assert.match(inspector, /engineCanvasNodeNoConfiguredHooks/);
  assert.match(inspector, /engineCanvasNodeNoConfiguredPrompts/);
  assert.match(inspector, /engineCanvasNodeHooks/);
  assert.match(inspector, /engineCanvasEnabled/);
  assert.match(inspector, /engineCanvasNodePrompts/);
  assert.match(inspector, /prompt\.source/);
  assert.match(inspector, /copyToClipboard\(prompt\.markdown/);
  assert.match(panel, /promptSurfaceStage\(surface\.surface_id\) === stage\.id/);
  assert.match(panel, /promptBlockStage\(block\.placement\) === stage\.id/);
  assert.match(panel, /placement: surface\.surface_id/);
  assert.match(panel, /engineCanvasHarnessPromptBlock/);
  assert.match(inspector, /engineCanvasNodeTools/);
  assert.match(inspector, /tool\.origin/);
  assert.match(panel, /engineCanvasToolOriginSnapshot/);
  assert.match(panel, /engineCanvasToolOriginBuiltin/);
  assert.match(inspector, /engineCanvasNodeSubagents/);
  assert.match(inspector, /subagent\.reason/);
  assert.match(inspector, /engineCanvasNodeRunResults/);
  assert.match(inspector, /engineCanvasRunInput/);
  assert.match(inspector, /engineCanvasRunOutput/);
  assert.match(inspector, /engineCanvasRunError/);
  assert.match(panel, /error: entry\.error_category/);
  assert.match(inspector, /runStatusLabel/);
  assert.match(canvas, /runStatusLabel\(locale, run\.status\)/);
  assert.match(panel, /runStatusLabel\(locale, run\.status\)/);
  assert.match(inspector, /engineCanvasOpenTimeline/);
  assert.match(inspector, /engineCanvasToolNoDescription/);
  assert.match(inspector, /engineCanvasToolSchemaDigest/);
  assert.match(inspector, /engineCanvasToolSnapshotRequired/);
  assert.match(inspector, /onOpenWorkspace\('runs', stageId\)/);
  assert.match(inspector, /onOpenWorkspace\('capabilities', stageId\)/);
  assert.match(inspector, /subagent\.prompt/);
  assert.match(inspector, /subagent\.source/);
  assert.match(panel, /engineCanvasSubagentSourceCapabilitySnapshot/);
  assert.match(panel, /engineCanvasSubagentSourceMasterTask/);
  assert.match(panel, /engineCanvasSubagentReasonPreset/);
  assert.match(panel, /engineCanvasSubagentReasonDynamic/);
  assert.match(inspector, /prompt\.canEdit/);
  assert.match(inspector, /onOpenWorkspace\('prompts', stageId, prompt\.id\)/);
  assert.match(inspector, /prompt\.removeLabel/);
  assert.match(panel, /engineCanvasRestoreDefaultPrompt/);
  assert.match(panel, /BUILTIN_TOOL_NAMES/);
  assert.match(panel, /source: 'native:builtin'/);
  assert.match(panel, /builtinToolDescription/);
  assert.match(panel, /focusPromptId/);
  assert.match(panel, /scrollIntoView\(\{ block: 'center', behavior: 'smooth' \}\)/);
  assert.match(panel, /traceEntriesForStage/);
  assert.match(panel, /engineCanvasRunSessionStart/);
  assert.match(panel, /engineCanvasRunSessionDetail/);
  assert.match(panel, /engineCanvasRunProviderSelection/);
  assert.match(panel, /engineCanvasRunProviderDetail/);
  assert.match(panel, /engineCanvasRunPermissionGate/);
  assert.match(panel, /engineCanvasRunPermissionDetail/);
  assert.match(panel, /engineCanvasRunPromptAssembly/);
  assert.match(panel, /engineCanvasRunSubagentStrategy/);
  assert.match(panel, /engineCanvasRunSubagentDetail/);
  assert.match(panel, /engineCanvasRunCompactCheck/);
  assert.match(panel, /engineCanvasRunCompactErrorDetail/);
  assert.match(panel, /engineCanvasRunStopDecision/);
  assert.match(panel, /engineCanvasRunStopErrorDetail/);
  assert.match(panel, /engineCanvasRunAuditSummary/);
  assert.match(panel, /engineCanvasRunAuditDetail/);
  assert.match(panel, /engineCanvasRunTerminalResult/);
  assert.match(panel, /selectedRun\.finished_at \?\? selectedRun\.started_at/);
  assert.match(panel, /engineCanvasRunTerminalErrorDetail/);
  assert.match(model, /runResults\?: CanvasNodeRunResult\[\]/);
  assert.match(model, /stage\.id === 'tool_gate' \|\| stage\.id === 'tool_execute'/);
  assert.match(panel, /engineCanvasDynamicSubagentPrompt/);
  assert.match(panel, /kind: 'dynamic'/);
  assert.match(panel, /engineCanvasSubagentSnapshotRequired/);
  assert.match(panel, /engineCanvasPresetExpertPrompt/);
  assert.match(panel, /engineCanvasPresetTeamPrompt/);
  assert.match(panel, /engineCanvasTeamMemberPrompt/);
  assert.match(panel, /kind: 'unresolved'/);
  assert.match(model, /'expert' \| 'team' \| 'member' \| 'dynamic' \| 'unresolved'/);
});
