# Global Coverage Manifest — Natives2 Tauri Migration

Generated: 2026-06-18
Source: `/Users/ldh/Downloads/project/AiNative/Natives/`
Target: `/Users/ldh/Downloads/project/AiNative/Natives2/`

## Status Legend

- ⬜ = Not started
- 🔄 = In progress
- ✅ = Done
- ⏭️ = Skipped (N/A for Tauri)

---

## Loop 0: Baseline Freeze

- [x] Tauri v2 skeleton created (`src-tauri/`)
- [x] Source tree copied (`src/`)
- [x] Docs copied (`docs/`, `plugin-template/`, `AGENTS.md`, `CONTEXT.md`, `README.md`, `PROMPT.md`)
- [x] Config files copied (`next.config.ts`, `tailwind.config.ts`, `postcss.config.mjs`, `tsconfig.json`)
- [x] `.gitignore` with Tauri rules
- [x] `package.json` adapted for Tauri
- [x] Frontend adapter created (`src/lib/tauri-adapter.ts`)
- [x] `npm install` succeeds (467 packages)
- [x] `cargo check` in `src-tauri/` succeeds (0 errors, 3 warnings)
- [x] Global coverage manifest complete

---

## Loop 1: API Adapter Contract ✅

### Frontend Adapter (`src/lib/tauri-adapter.ts`)
- [x] `themeReady()`
- [x] `app.version()`
- [x] `db.get/set/delete/list`
- [x] `terminal.create/write/resize/kill/cwd/onData/onExit`
- [x] `module.scan/install/readManifest/grantPermission/revokePermission/listPermissions/getAuditLog/approveAllPermissions/uninstall/list/enable/disable/update`
- [x] `env.getVariables/getDefaultProfile/listProfiles/createProfile/deleteProfile/setDefaultProfile/setVariable/deleteVariable/encrypt/decrypt`
- [x] `getTheme/setTheme`
- [x] `shell.showItemInFolder/openPath`
- [x] `getLocale/setLocale`
- [x] `notification.send/list/markRead/markAllAsRead`
- [x] `fs.listDir/readFile/writeFileAtomic/createEntry/renameEntry/trashEntry/moveEntry/importFiles/recentFiles`
- [x] `archive.list`
- [x] `search.grep/files/spotlight`
- [x] `state.save/load/clear`
- [x] `git.status/diff`
- [x] `disk.usage`
- [x] `thumbnail.generate`
- [x] `agent.scanProjects/getSessions/scanSkills/detectStatus`
- [x] `skills.enable/disable/getDeactivatedPath/uninstall`
- [x] `onDbStateChanged`
- [x] `screenshot.watch/saveAnnotated`
- [x] `release.inspect/prepare/getSequence/execute`
- [x] `update.check/mute/getMuted`
- [x] `clipboard.write/read`
- [x] `usage.refresh`
- [x] `windowControls.minimize/maximize/close/isMaximized`
- [x] `openWidgetWindow`

### Tauri Commands (stubs in `src-tauri/src/commands/`)
- [x] `app_version`
- [x] `db_get/set/delete/list`
- [x] `get_theme/set_theme`
- [x] `get_locale/set_locale`
- [x] `show_item_in_folder/open_path`
- [x] `window_minimize/maximize/close/is_maximized`
- [x] `clipboard_write/read`
- [x] `terminal_create/write/resize/kill/cwd`
- [x] `module_scan/install/readManifest/grantPermission/revokePermission/listPermissions/getAuditLog/approveAllPermissions/uninstall/list/enable/disable/update`
- [x] `env_get_variables/getDefaultProfile/listProfiles/createProfile/deleteProfile/setDefaultProfile/setVariable/deleteVariable/encrypt/decrypt`
- [x] `notification_send/list/markRead/markAllRead`
- [x] `fs_listDir/readFile/writeFileAtomic/createEntry/renameEntry/trashEntry/moveEntry/importFiles/recentFiles`
- [x] `archive_list`
- [x] `search_grep/files/spotlight`
- [x] `state_save/load/clear`
- [x] `git_status/diff`
- [x] `disk_usage`
- [x] `thumbnail_generate`
- [x] `agent_scanProjects/getSessions/scanSkills/detectStatus`
- [x] `skills_enable/disable/getDeactivatedPath/uninstall`
- [x] `screenshot_startWatching/stopWatching/saveAnnotated`
- [x] `release_inspect/prepare/getSequence/execute`
- [x] `update_check/mute/getMuted`
- [x] `usage_refresh`
- [x] `open_widget_window`
- [x] `theme_ready_signal`

### Tauri Events
- [ ] `db-state-changed` broadcast
- [ ] `terminal:data` streaming
- [ ] `terminal:exit` notification
- [ ] `screenshot:detected` watcher
- [ ] `fs:changed` file watcher

---

## Loop 2: Shell, Navigation, Window Runtime ✅

### Shell Components
- [x] `src/components/shell/ShellLayout.tsx` — uses window.nativesAPI, no Electron imports
- [x] `src/components/shell/Header.tsx` — uses window.nativesAPI, no Electron imports
- [x] `src/components/shell/Sidebar.tsx` — uses window.nativesAPI, no Electron imports
- [x] `src/components/shell/RightPanel.tsx` — uses window.nativesAPI, no Electron imports
- [x] `src/components/shell/ContentArea.tsx` — pure React, no native deps
- [x] `src/components/shell/CommandPalette.tsx` — pure React, no native deps
- [x] `src/components/shell/ControlHubWidget.tsx` — uses window.nativesAPI, no Electron imports
- [x] `src/components/shell/NotificationPanel.tsx` — uses window.nativesAPI, no Electron imports

### Pages
- [x] `src/app/page.tsx` (dashboard) — pure React
- [x] `src/app/loading.tsx` — pure React
- [x] `src/app/not-found.tsx` — pure React
- [x] `src/app/layout.tsx` — pure React (ThemeProvider, ToastProvider)

### Window Runtime
- [x] Transparent frameless window (tauri.conf.json: transparent, decorations: false)
- [x] macOS traffic light positioning (Sidebar uses WebkitAppRegion drag/no-drag)
- [x] Window state persistence (tauri-plugin-window-state)
- [x] Single instance lock (tauri-plugin-single-instance)
- [x] FOUC guard (window starts hidden, theme_ready_signal shows it)
- [x] window_tile command (left/right/top/bottom/fullscreen/tile)

### Browser Events
- [x] `navigate` — pure CustomEvent, works in any browser environment
- [x] `navigate-files` — pure CustomEvent
- [x] `toggle-terminal` — pure CustomEvent
- [x] `locale-changed` — pure CustomEvent
- [x] `visual-config-changed` — pure CustomEvent
- [x] `header-file-action` — pure CustomEvent

---

## Loop 3: SQLite, Settings, Theme, Locale

### DB Schema (9 tables)
- [ ] `settings` table
- [ ] `modules` table
- [ ] `module_data` table
- [ ] `notifications` table
- [ ] `audit_log` table
- [ ] `env_profiles` table
- [ ] `env_variables` table
- [ ] `recent_files` table
- [ ] `state_persistence` table

### DB Operations
- [ ] WAL mode + foreign keys
- [ ] Migration system
- [ ] `db.get/set/delete/list` full implementation
- [ ] `db-state-changed` event broadcasting

### Theme
- [ ] `src/lib/theme-engine.ts`
- [ ] `src/context/ThemeContext.tsx`
- [ ] Theme persistence in DB
- [ ] CSS variable injection
- [ ] 3 built-in themes (Terminal Volt, Warm Archive, Editorial Index)

### Locale
- [ ] `src/i18n/en.ts`
- [ ] `src/i18n/zh.ts`
- [ ] `src/i18n/index.ts`
- [ ] Locale persistence in DB

---

## Loop 4: Security, Bridge, Local HTTP

### Security
- [ ] `src/lib/token-manager.ts`
- [ ] Session Token handshake
- [ ] `src/lib/iframe-sandbox.ts`
- [ ] `src/lib/iframe-sandbox-manager.ts`
- [ ] `src/lib/iframe-manager.ts`
- [ ] CSP headers
- [ ] `MessageEvent.source` verification
- [ ] Module isolation (`/modules/{moduleId}/`)

### Bridge
- [ ] `src/main/bridge-host.ts`
- [ ] `src/main/bridge-ipc.ts`
- [ ] `src/main/bridge-notification.ts`
- [ ] `src/lib/bridge-sdk.js`

### Local HTTP Server
- [ ] `src/main/http-server.ts`
- [ ] Module asset serving
- [ ] Port management

---

## Loop 5: Module Runtime And Workshop

### Module Management
- [ ] `src/main/module-manager.ts`
- [ ] `src/main/installer.ts`
- [ ] `src/main/permission-center.ts`

### Pages
- [ ] `src/app/modules/page.tsx`
- [ ] `src/components/shell/WorkshopPage.tsx`

### Module Operations
- [ ] scan/install/readManifest
- [ ] enable/disable/update/uninstall
- [ ] grantPermission/revokePermission/listPermissions
- [ ] getAuditLog/approveAllPermissions

---

## Loop 6: Plugin Lifecycle And State Preservation

### iframe Management
- [ ] `src/components/iframe/IframeContainer.tsx`
- [ ] `src/lib/iframe-manager.ts`
- [ ] `src/lib/iframe-sandbox-manager.ts`
- [ ] Hot/warm/cold/persistent lifecycle
- [ ] LRU cache
- [ ] Heartbeat monitoring
- [ ] `src/components/crash/CrashMonitor.tsx`
- [ ] Crash overlay + notification

### State Persistence
- [ ] `src/lib/state-persistence.ts`
- [ ] `state.save/load/clear` commands

---

## Loop 7: Terminal And Env

### Terminal
- [ ] `src/components/shell/Terminal.tsx`
- [ ] Rust PTY implementation (portable-pty)
- [ ] `terminal:create/write/resize/kill/cwd`
- [ ] `terminal:data` / `terminal:exit` events
- [ ] TUI app support
- [ ] Window resize handling

### Environment
- [ ] `src/main/config-manager.ts`
- [ ] `src/lib/env-injector.ts`
- [ ] Env profiles CRUD
- [ ] Encrypted values (port from electron.safeStorage)
- [ ] Default profile
- [ ] Terminal injection

---

## Loop 8: File Runtime

### File Operations
- [ ] `src/main/file-manager.ts`
- [ ] `src/main/file-watcher.ts`
- [ ] `src/main/safe-file-service.ts`
- [ ] `src/main/recent-files.ts`

### Tauri Commands
- [ ] `fs_list_dir`
- [ ] `fs_read_file`
- [ ] `fs_write_file_atomic`
- [ ] `fs_create_entry`
- [ ] `fs_rename_entry`
- [ ] `fs_trash_entry`
- [ ] `fs_move_entry`
- [ ] `fs_import_files`
- [ ] `fs_recent_files`

### File Browser UI
- [ ] `src/app/files/page.tsx`
- [ ] `src/components/files/FileBrowser.tsx`
- [ ] `src/components/files/FileToolbar.tsx`
- [ ] `src/components/files/FileBreadcrumb.tsx`
- [ ] `src/components/files/FileContextMenu.tsx`
- [ ] `src/components/files/FileGrid.tsx`
- [ ] `src/components/files/FileList.tsx`
- [ ] `src/components/files/FileCard.tsx`
- [ ] `src/components/files/FileRow.tsx`
- [ ] `src/components/files/FileSearch.tsx`

### Next API Routes (migrate to Tauri)
- [ ] `src/app/api/fs/listDir/route.ts`
- [ ] `src/app/api/fs/raw/` (file streaming)
- [ ] `src/app/api/fs/createEntry/`
- [ ] `src/app/api/fs/renameEntry/`
- [ ] `src/app/api/fs/trashEntry/`
- [ ] `src/app/api/fs/recentFiles/`
- [ ] `src/app/api/fs/du/`
- [ ] `src/app/api/fs/thumb/`

---

## Loop 9: Preview, Media, Archive, Thumbnails

### Preview Components
- [ ] `src/components/files/FilePreview.tsx`
- [ ] `src/components/files/MonacoEditor.tsx`
- [ ] `src/components/files/MonacoDiffView.tsx`
- [ ] `src/components/files/MilkdownEditor.tsx`
- [ ] `src/components/editor/MarkdownEditor.tsx`
- [ ] `src/components/preview/HtmlPreview.tsx`
- [ ] `src/components/files/ArchivePreview.tsx`
- [ ] `src/components/files/CsvTable.tsx`

### Media Support
- [ ] Image preview
- [ ] Video preview
- [ ] Audio preview
- [ ] PDF preview

### Archive & Disk
- [ ] `src/main/archive.ts`
- [ ] `src/main/disk-usage.ts`
- [ ] `src/main/thumbnail.ts`
- [ ] `src/components/files/DiskUsage.tsx`
- [ ] `src/components/files/DiskUsagePanel.tsx`

---

## Loop 10: Search And Git

### Search
- [ ] `src/lib/search-engine.ts`
- [ ] `search:grep` command
- [ ] `search:files` command
- [ ] `search:spotlight` command
- [ ] CommandPalette content search

### Git
- [ ] `src/main/git.ts`
- [ ] `src/components/files/GitPanel.tsx`
- [ ] `git:status` command
- [ ] `git:diff` command

---

## Loop 11: AI Workbench

### AI Components
- [ ] `src/components/ai/AiWorkbench.tsx`
- [ ] `src/components/ai/ProjectMemory.tsx`
- [ ] `src/components/ai/SessionReplay.tsx`
- [ ] `src/components/ai/ChangeInbox.tsx`
- [ ] `src/components/ai/FollowModeUI.tsx`
- [ ] `src/components/ai/FollowRenderer.tsx`
- [ ] `src/components/ai/SkillsPanel.tsx`
- [ ] `src/components/ai/UsagePanel.tsx`
- [ ] `src/components/ai/RtkPanel.tsx`
- [ ] `src/components/ai/AIFileOrganizer.tsx`
- [ ] `src/components/ai/AgentDashboard.tsx`
- [ ] `src/components/ai/PromptLibrary.tsx`
- [ ] `src/components/ai/FlingToTerminal.tsx`

### Dashboard
- [ ] `src/components/dashboard/TokenHero.tsx`
- [ ] `src/components/dashboard/TokenTrendChart.tsx`
- [ ] `src/components/dashboard/ModelStatsTable.tsx`
- [ ] `src/components/dashboard/SkillsPanel.tsx`

### Agent Backend
- [ ] `src/main/session-scanner.ts`
- [ ] `src/main/skills-manager.ts`
- [ ] `src/main/skill-stats.ts`
- [ ] `src/main/usage-tracker.ts`
- [ ] `src/main/agent-status.ts`

---

## Loop 12: Tools And Secondary Workflows

### Screenshot
- [ ] `src/main/screenshot.ts`
- [ ] `src/components/screenshot/ScreenshotCard.tsx`
- [ ] `src/components/screenshot/AnnotationEditor.tsx`
- [ ] `src/components/tools/ScreenshotCard.tsx`

### Release Wizard
- [ ] `src/main/release-wizard.ts`
- [ ] `src/components/release/ReleaseWizardDialog.tsx`
- [ ] `src/components/tools/ReleaseWizardDialog.tsx`

### Update
- [ ] `src/main/update-checker.ts`
- [ ] `src/main/updater.ts`
- [ ] `src/components/tools/UpdateNotification.tsx`
- [ ] `src/components/update/UpdateNotification.tsx`

### Other Tools
- [ ] `src/components/tools/ImageAnnotator.tsx`
- [ ] `src/components/tools/ToolsPage.tsx`
- [ ] `src/app/tools/page.tsx`
- [ ] `src/lib/clipboard.ts`
- [ ] `src/main/clipboard.ts`

---

## Loop 13: Popups, Accessibility, Visual Parity

### UI Components
- [ ] `src/components/ui/ConfirmDialog.tsx`
- [ ] `src/components/ui/Modal.tsx`
- [ ] `src/components/ui/Portal.tsx`
- [ ] `src/components/ui/Toast.tsx`
- [ ] `src/components/ui/ShortcutHelp.tsx`
- [ ] `src/components/ui/ErrorBoundary.tsx`
- [ ] `src/components/ui/Skeleton.tsx`
- [ ] `src/components/ui/ProgressBar.tsx`
- [ ] `src/components/ui/EmptyState.tsx`
- [ ] `src/components/ui/LiquidGlass.tsx`

### Onboarding
- [ ] `src/components/onboarding/OnboardingWizard.tsx`
- [ ] `src/components/onboarding/UsernameOnboarding.tsx`

### Settings
- [ ] `src/components/shell/SettingsPage.tsx`

### Other
- [ ] `src/components/files/ImageLightbox.tsx`
- [ ] `src/app/store/page.tsx`

---

## Loop 14: Docs And Standards Migration

### Architecture Docs
- [ ] `docs/architecture/ARCHITECTURE.md`
- [ ] `docs/architecture/DESIGN_DISCUSSION.md`
- [ ] `docs/architecture/TECHNICAL_DESIGN_V2.md`
- [ ] `docs/architecture/UI_OPTIMIZATION_PLAN.md`

### ADRs
- [ ] `docs/adr/0001-session-token-handshake.md`
- [ ] `docs/adr/0002-postmessage-origin-verification.md`
- [ ] `docs/adr/0003-plugin-ipc-main-process-relay.md`
- [ ] `docs/adr/0004-terminal-env-injection-new-sessions-only.md`
- [ ] `docs/adr/0005-plugin-state-preservation-strategy.md`
- [ ] `docs/adr/0006-iframe-crash-detection.md`
- [ ] `docs/adr/0007-domain-wheel-reinvention-clarification.md`
- [ ] `docs/adr/0008-electron-to-tauri-migration.md` (NEW)

### Standards
- [ ] `docs/standards/README.md`
- [ ] `docs/standards/00-glossary.md`
- [ ] `docs/standards/frontend/01-structure.md`
- [ ] `docs/standards/frontend/02-state-and-data.md`
- [ ] `docs/standards/frontend/03-i18n.md`
- [ ] `docs/standards/product/01-positioning.md`
- [ ] `docs/standards/product/02-feature-spec.md`
- [ ] `docs/standards/technical/01-layering.md`
- [ ] `docs/standards/technical/02-security.md`
- [ ] `docs/standards/technical/03-data.md`
- [ ] `docs/standards/ui-ux/01-design-tokens.md`
- [ ] `docs/standards/ui-ux/02-interaction.md`
- [ ] `docs/standards/ui-ux/03-feedback.md`

### Other Docs
- [ ] `docs/PRD-v2.md`
- [ ] `docs/STYLE_GUIDE_AUDIT.md`
- [ ] `AGENTS.md`
- [ ] `CONTEXT.md`
- [ ] `PROMPT.md`
- [ ] `README.md`

### Plugin Template
- [ ] `plugin-template/README.md`
- [ ] `plugin-template/index.html`
- [ ] `plugin-template/manifest.json`
- [ ] `plugin-template/natives-sdk.d.ts`

---

## Loop 15: Packaging And Final Regression

- [ ] Tauri bundle config (icons, bundle id, macOS settings)
- [ ] Updater plugin configured
- [ ] Window state plugin configured
- [ ] Single instance plugin configured
- [ ] Log plugin configured
- [ ] Full Rust tests pass
- [ ] TypeScript checks pass
- [ ] Frontend build succeeds
- [ ] Tauri build produces .app
- [ ] Fresh profile test
- [ ] Existing `~/.natives` profile test

---

## Runtime Modules (`src/main/`)

| Module | Status | Loop |
|--------|--------|------|
| database.ts | ⬜ | 3 |
| http-server.ts | ⬜ | 4 |
| bridge-host.ts | ⬜ | 4 |
| bridge-ipc.ts | ⬜ | 4 |
| bridge-notification.ts | ⬜ | 4 |
| module-manager.ts | ⬜ | 5 |
| installer.ts | ⬜ | 5 |
| permission-center.ts | ⬜ | 5 |
| file-manager.ts | ⬜ | 8 |
| file-watcher.ts | ⬜ | 8 |
| safe-file-service.ts | ⬜ | 8 |
| recent-files.ts | ⬜ | 8 |
| shell.ts | ⬜ | 7 |
| config-manager.ts | ⬜ | 7 |
| archive.ts | ⬜ | 9 |
| disk-usage.ts | ⬜ | 9 |
| thumbnail.ts | ⬜ | 9 |
| git.ts | ⬜ | 10 |
| session-scanner.ts | ⬜ | 11 |
| skills-manager.ts | ⬜ | 11 |
| skill-stats.ts | ⬜ | 11 |
| usage-tracker.ts | ⬜ | 11 |
| agent-status.ts | ⬜ | 11 |
| screenshot.ts | ⬜ | 12 |
| release-wizard.ts | ⬜ | 12 |
| update-checker.ts | ⬜ | 12 |
| updater.ts | ⬜ | 12 |
| clipboard.ts | ⬜ | 12 |
| log-rotate.ts | ⬜ | 15 |
| watchdog.ts | ⏭️ | N/A (Tauri has built-in process mgmt) |

---

## Frontend Lib (`src/lib/`)

| Module | Status | Loop |
|--------|--------|------|
| tauri-adapter.ts | ✅ | 1 |
| theme-engine.ts | ⬜ | 3 |
| iframe-manager.ts | ⬜ | 6 |
| iframe-sandbox-manager.ts | ⬜ | 6 |
| iframe-sandbox.ts | ⬜ | 6 |
| token-manager.ts | ⬜ | 4 |
| bridge-sdk.js | ⬜ | 4 |
| state-persistence.ts | ⬜ | 6 |
| env-injector.ts | ⬜ | 7 |
| search-engine.ts | ⬜ | 10 |
| clipboard.ts | ⬜ | 12 |
| design-tokens.ts | ⬜ | 2 |
| error-classifier.ts | ⬜ | 1 |
| agent-narration.ts | ⬜ | 11 |
| chime.ts | ⬜ | 2 |
| file-badges.ts | ⬜ | 8 |
| file-icons.tsx | ⬜ | 8 |
| follow-mode.ts | ⬜ | 11 |
| format.ts | ⬜ | 2 |
| notification-ui.ts | ⬜ | 2 |
| path-detector.ts | ⬜ | 2 |
| recent-files-client.ts | ⬜ | 8 |
| recent-modules.ts | ⬜ | 5 |
| runtime-log.ts | ⬜ | 2 |
| safe-stream.ts | ⬜ | 9 |
| shiki-utils.ts | ⬜ | 9 |
| use-file-drop.ts | ⬜ | 8 |
| useFileContent.ts | ⬜ | 8 |
| useFocusTrap.ts | ⬜ | 13 |
| web-fs-client.ts | ⬜ | 8 |
