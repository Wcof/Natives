# Slice 13 gap map (2026-08-22)
## CHAIN
1. Host Tauri cmds: WORKING (workspace.rs verified, A-013..A-021)
2. Typed IPC: 26 fns = 26 cmds; bridge nativesAPI.workspace → cmd() [client.ts:48,60-275]
3. Provider hydration: localStorage cache or built-in default; client never used [provider:215-219]
4. Grid: 4 hardcoded demo cards; 0 registry refs, 0 snapshot-store refs [GridView:36-66]
5. snapshot-store.ts: 0 importers anywhere in src/ (dead; should cache host snapshot)
## PROVIDER
6. State: useReducer(workspaceReducer, undefined, hydrateWorkspaceSnapshot) [provider:215-219]
7. hydrateWorkspaceSnapshot: sync localStorage read (WORKSPACE_STORAGE_KEY), else default snapshot
8. Source: workspacePersistence.ts:14-25,67-79; no host/client data ever reaches the reducer
9. saverRef = useRef(createWorkspaceSnapshotSaver()) [220]; debounce 250ms → localStorage [225-227]
10. beforeunload flush [229-236]; context value = {snapshot, dispatch, api} [205-209,255-258]
11. WCP mounts provider unseeded [WCP:36-42]; RootClient only Shell+theme+toast [RootClient:60-68]
## GRID
12. Cards = Object.keys(widgetMeta).map → CompactGridItem [GridView:46-66]
13. Fixed ids: note-welcome, todo-focus, note-snippets, stats-quick [38-41]
14. Titles via i18n (zh/app.ts:519-520, en/app.ts:518-519) — the keys ARE the demo strings
15. WidgetBody [84-126]: welcome desc, 3 task rows, 3 tip rows, literal stats "3"/"0"/"6"
16. WCP passes view.gridLayouts; viewId prop unused (_viewId) [WCP:273-281, GridView:26]
## REGISTRY
17. Registry: src/lib/workspace/widgets/registry.ts [15-73]
18. API: registerWidget(s), getWidget, hasWidget, getAllWidgets, getRegisteredTypes
19. 15 types registered in barrel [components/workspace/widgets/index.ts:34-50]; all real adapters
20. Loads via workspaceDataBroker → domain facades (adapters/{greeting,apps,storage,usage...}); no mocks
21. WidgetRenderer (WidgetRenderer.tsx:89-137) has real loading/error/empty state machine
22. NOT on live path: WCP/GridView never import the barrel; only home/widgets/index.ts [17]
23. HomeWorkspacePage.tsx:32,208 resolves def.Component — legacy page, not routed at /
## STORE
24. API: getSnapshot, setSnapshot, invalidate, invalidateAll, subscribeSnapshot, loadSnapshot
25. Location + lines: snapshot-store.ts:24-68
26. loadSnapshot = client.getWorkspace (client.ts:64) + cache [60-68]; holds contracts.WorkspaceSnapshot
27. SHOULD consume: Provider (cache after host fetch) + GridView (widgets list)
28. DOESN'T: 0 importers; only home-workspace/persistence.ts imports the client directly
## CLIENT
29. Live render path calls: NONE; only home-workspace/{model,persistence}.ts import client.ts
30. Needed by slice: listWorkspaces+getWorkspace (hydrate), upsertWidget/removeWidget, saveLayout
## PLAN
31. a1 WCP mount effect: listWorkspaces() → pick active||[0] → loadSnapshot(id) → setSnapshot
32. a1b If null → createWorkspace({name:'Workspace'}); keep contracts snapshot in useState
33. a2 WCP: status pending/ready/error; pending → Skeleton, error → ErrorPrimitive + retry
34. a3 Provider 'hydrate' case: also setSnapshot(workspaceId, contracts snapshot) — store in sync
35. b WCP → GridView prop widgets = contracts.widgets.filter(!hidden).sort(position)
36. c GridView: DELETE widgetMeta/WidgetBody/Stat; item = widgets.map → getWidget(widgetType)
37. c2 per item: <WidgetRenderer instance={{def,config}} onRemove/>; config={type,enabled,surface,order,settings}
38. c3 WCP handleGridRemoveItem: keep layout filter + client.removeWidget(workspaceId, widgetId, revision)
39. d DELETE i18n demo keys: widgetWelcome,widgetWelcomeDesc,widgetToday,widgetSnippets,widgetActivity
40. d2 +taskCore,taskMigrate,taskInspector,tipCmdK,tipPan,tipGroup (zh 524-528 / en 523-527); keep gridEmpty
41. d3 +statViews,statCanvasNodes,statDataRows,statInspector,statReady (zh 529-531 / en 528-530)
42. e 0 widgets → CompactGrid already shows emptyText 'This view has no widgets yet.' [GridView:78]
43. e2 '+ Add widget' (edit mode) → getRegisteredTypes() menu → upsertWidget(widgetType, config, position)
44. f onGridLayoutChange: api.updateView (cache) + client.saveLayout(workspaceId, bp, layoutJson, revision)
45. g createDefaultWorkspaceSnapshot stays only as offline fallback on host failure; no new files/deps
## RISKS
46. NO tests cover GridView/WCP/provider/snapshot-store/client (verified across src/)
47. Nearest tests: assistant-workspace/*, home-workspace/layoutModel, i18n active-ui-keys — none touch chain
48. TWO WorkspaceSnapshot types: views/types.ts:93 (UI) vs contracts.ts:111 (host) — alias, never merge
49. Provider writes EVERY snapshot to localStorage [225-227]; host result must win, sync = first paint only
50. Demo tabs/views flash before host data arrives — acceptable only as pending fallback (33)
51. StatusBar 'savedToCache' [WCP:181, i18n 643-644] becomes a lie — rename to host-saved (zh+en)
52. Don't break home-workspace/persistence.ts — only current client consumer (A-029..A-031)
53. GridView add/remove needs workspaceId, not viewId (prop is unused _viewId) [WCP:273-281]
54. DataView demo rows (WCP:362 'Wave1 demo' comment) out of scope — flagged only
