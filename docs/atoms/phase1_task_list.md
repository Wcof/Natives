# Phase 1 — Goal Parsing & Decomposition

> 输入源: `docs/fanbox.md` + 竞品 `/References/fanbox/`
> 竞品代码结构: `server.js`(2180L) / `electron/main.js`(842L) / `public/app.js`(4572L) / `public/i18n-dict.js` / `public/style.css`

---

Task List:

## Domain 1: File Manager

- Task 1: File_GridListView - 参考路径: `public/app.js:renderFiles()+gridItem()+listRow()+measureCols()`
- Task 2: File_BreadcrumbNav - 参考路径: `public/app.js:renderBreadcrumb()`
- Task 3: File_FuzzySearch - 参考路径: `server.js:fuzzyScore()+searchFiles()`
- Task 4: File_RecencyBoost - 参考路径: `server.js:searchFiles()+grepFiles()` recencyBonus
- Task 5: File_GrepContent - 参考路径: `server.js:grepFiles()`
- Task 6: File_SpotlightSearch - 参考路径: `server.js:mdfind()+contentSearch()`
- Task 7: File_SpotlightFallback - 参考路径: `server.js:contentSearch()` fallback
- Task 8: File_LargeFileLazy - 参考路径: `server.js:readFile()` truncation
- Task 9: File_ProjectDetect - 参考路径: `server.js:listDir()+projectOf()`
- Task 10: File_RichPreview - 参考路径: `public/app.js:openPreview()+renderTextPreview()+renderHtmlPreview()+csvTable()`
- Task 11: File_HeicPreview - 参考路径: `server.js:serveHeicAsJpeg()`
- Task 12: File_HtmlPreviewSandbox - 参考路径: `server.js:serveHtmlPreview()+previewPathAllowed()`
- Task 13: File_HtmlLocalImageFixup - 参考路径: `server.js:serveHtmlPreview()` img rewrite
- Task 14: File_HtmlFullInteraction - 参考路径: `server.js:serveHtmlPreview()` sandbox iframe
- Task 15: File_ArchivePreview - 参考路径: `server.js:zipNames()+archiveList()`
- Task 16: File_ThumbGen - 参考路径: `server.js:generateThumb()+pruneThumbs()+serveThumb()`
- Task 17: File_ThumbRetry - 参考路径: `public/app.js:thumbHtml()` onerror
- Task 18: File_OneClickOpen - 参考路径: `server.js:openInOS()+openDefault()`
- Task 19: File_FavRecent - 参考路径: `server.js:updateConfig()+recentFiles()`
- Task 20: File_DotFileToggle - 参考路径: `public/app.js` dotfile toggle UI
- Task 21: File_SortFilter - 参考路径: `public/app.js:visibleEntries()+moveCursor()+cursorEnter()`
- Task 22: File_WalkIgnoreBudget - 参考路径: `server.js:walk()`
- Task 23: File_FsWatch - 参考路径: `server.js` watch; `public/app.js:updateWatches()`
- Task 24: File_ChangeBadge - 参考路径: `public/app.js` change badge
- Task 25: File_FollowMode - 参考路径: `public/app.js` follow mode
- Task 26: File_FollowNarration - 参考路径: `public/app.js` narration
- Task 27: File_FollowLiveRender - 参考路径: `public/app.js` live render
- Task 28: File_FollowArtifact - 参考路径: `public/app.js` artifact card
- Task 29: File_FollowNoiseFilter - 参考路径: `server.js` noise filter
- Task 30: File_ImageEditor - 参考路径: `public/app.js:enterImageEdit()+buildImageEditor()+bindImageEditor()`
- Task 31: File_MonacoEdit - 参考路径: `public/app.js` Monaco integration
- Task 32: File_MilkdownEdit - 参考路径: `public/app.js` Milkdown integration
- Task 33: File_AtomicWrite - 参考路径: `server.js:writeTextFile()` temp+fsync+rename; `updateConfig()` _cfgChain
- Task 34: File_EditConflictGuard - 参考路径: `server.js:writeTextFile()` expectedMtime
- Task 35: File_DragDrop - 参考路径: `public/app.js:dropFilesInto()+dropUrlInto()+makeDraggablePath()`
- Task 36: File_ScreenshotBus - 参考路径: `electron/main.js:startShotWatch()+waitStable()`
- Task 37: File_ContextNew - 参考路径: `public/app.js` context menu
- Task 38: File_RenameTrash - 参考路径: `server.js:trashPath()+validName()+renamePath()`
- Task 39: File_DiskUsage - 参考路径: `server.js:diskUsage()`
- Task 40: File_GitDiff - 参考路径: `server.js:execGit()+gitStatus()+gitFileDiff()`
- Task 41: File_PreviewFullscreen - 参考路径: `public/app.js:lightbox()+setPreviewMax()`
- Task 42: File_LayoutDrag - 参考路径: `public/app.js:applyLayout()+animateLayout()+bindSidebarResizer()`
- Task 43: File_I18n - 参考路径: `public/i18n-dict.js` + `public/i18n.js`
- Task 44: File_ThreeThemes - 参考路径: `public/style.css` + `public/app.js` theme
- Task 45: File_AutoUpdate - 参考路径: `electron/main.js:fetchLatestRelease()+checkUpdate()`
- Task 46: File_SecurityModel - 参考路径: `server.js:hostAllowed()+originAllowed()+validName()+previewPathAllowed()`
- Task 47: File_SidebarTree - 参考路径: `public/app.js` sidebar tree
- Task 48: File_AgentProjects - 参考路径: `server.js:agentProjects()+readCwdFromHead()`
- Task 49: File_StatusBar - 参考路径: `public/app.js:renderStatusbar()`
- Task 50: File_AI_Organize - 参考路径: `server.js:findAgentBin()+organizeHistory()+organizeLaunch()`
- Task 51: File_ReleaseWizard - 参考路径: `server.js:releaseInspect()+releasePrepare()`
- Task 52: File_ProjectMemory - 参考路径: `server.js:projectMemory()+parseClaudeSession()+parseCodexSession()`
- Task 53: File_MoveEntry - 参考路径: `server.js:movePath()+createEntry()`
- Task 54: File_LocatePath - 参考路径: `server.js:locatePath()+statWithTail()`
- Task 55: File_CopyImageFile - 参考路径: `electron/main.js:clip:image+clip:file`
- Task 56: File_SvgIconSystem - 参考路径: `public/app.js:richIcon()+iconColorFor()+ic()+svgWrap()`
- Task 57: File_VideoThumb - 参考路径: `server.js:generateThumb()` qlmanage
- Task 58: File_SymlinkResolve - 参考路径: `server.js:listDir()` symlink
- Task 59: File_ProxyBypass - 参考路径: `electron/main.js:session.setProxy()`
- Task 60: File_ConfigSerialization - 参考路径: `server.js:updateConfig()` _cfgChain
- Task 61: File_DblClickFullscreen - 参考路径: `public/app.js:onItemOpen()`
- Task 62: File_PreviewAsEdit - 参考路径: `public/app.js` preview-as-edit
- Task 63: File_TempFileFilter - 参考路径: `server.js` .tmp filter
- Task 64: File_SaveImage - 参考路径: `server.js:saveImage()`
- Task 65: File_SidebarDragWindow - 参考路径: `public/app.js` sidebar drag
- Task 66: File_ScrollbarStyle - 参考路径: `public/style.css`
- Task 67: File_GridTypeTint - 参考路径: `public/app.js:gridItem()` tint
- Task 68: File_PortConflict - 参考路径: `server.js` EADDRINUSE
- Task 69: File_EnvPort - 参考路径: `server.js` FANBOX_PORT
- Task 70: File_DefaultRoots - 参考路径: `server.js:defaultRoots()`

## Domain 2: Terminal

- Task 71: Term_PtySpawn - 参考路径: `electron/main.js:pty:spawn` handler
- Task 72: Term_MultiTab - 参考路径: `public/app.js` tab render+activate+close
- Task 73: Term_CwdSync - 参考路径: `electron/main.js:pty:cwd+termCwdByPid()`
- Task 74: Term_PathClickable - 参考路径: `public/app.js` path linkifier
- Task 75: Term_UrlClickable - 参考路径: `public/app.js` URL linkifier
- Task 76: Term_PathDrag - 参考路径: `public/app.js:flingToTerminal()`
- Task 77: Term_WebGLRender - 参考路径: `public/app.js` @xterm/addon-webgl
- Task 78: Term_LoginShell - 参考路径: `electron/main.js:pty:spawn` shell args
- Task 79: Term_NerdFont - 参考路径: `public/app.js` font detection
- Task 80: Term_CapsLockPatch - 参考路径: `public/vendor/` xterm patch
- Task 81: Term_OptClickSelect - 参考路径: `public/app.js` xterm config
- Task 82: Term_Recording - 参考路径: `electron/main.js:recStart()+recEvent()+recStop()`
- Task 83: Term_RecordingPlayback - 参考路径: `public/app.js` playback
- Task 84: Term_RecordingExport - 参考路径: `electron/main.js:rec:export+findFfmpeg()`
- Task 85: Term_AgentLaunch - 参考路径: `public/app.js` launchAgent
- Task 86: Term_AgentRespawn - 参考路径: `public/app.js` respawn
- Task 87: Term_AgentStatus - 参考路径: `public/app.js` ensureStatusTick
- Task 88: Term_AgentNotify - 参考路径: `electron/main.js` notification
- Task 89: Term_AgentFalseComplete - 参考路径: `public/app.js` false complete guard
- Task 90: Term_ThemeSync - 参考路径: `public/app.js` retheme
- Task 91: Term_Maximize - 参考路径: `public/app.js` maximize
- Task 92: Term_SendContext - 参考路径: `public/app.js` sendContext
- Task 93: Term_LidGuard - 参考路径: `electron/main.js:setLidIntent()+installSudoers()+refreshLidGuard()`
- Task 94: Term_ChineseWidth - 参考路径: `public/app.js` Unicode11 addon
- Task 95: Term_ScrollbackScan - 参考路径: `public/app.js:scanScrollbackFor()`
- Task 96: Term_WechatClawBot - 参考路径: `electron/main.js:wechat:*` IPC
- Task 97: Term_WechatQR - 参考路径: `electron/wechat/` QR
- Task 98: Term_WechatTranscript - 参考路径: `electron/wechat/` transcript
- Task 99: Term_WechatAgentRegistry - 参考路径: `electron/wechat/` CONNECTABLE_AGENTS
- Task 100: Term_WechatStateMachine - 参考路径: `electron/wechat/` state machine
- Task 101: Term_WechatGatewayMgmt - 参考路径: `electron/wechat/` gateway
- Task 102: Term_QuitGuard - 参考路径: `electron/main.js` beforequit
- Task 103: Term_ExitCleanup - 参考路径: `electron/main.js` will-quit
- Task 104: Term_ProcDetect - 参考路径: `electron/main.js:pty:proc`
- Task 105: Term_OutputTail - 参考路径: `electron/main.js` termTails
- Task 106: Term_NativeEditMenu - 参考路径: `electron/main.js:buildMenu()`
- Task 107: Term_ExternalLink - 参考路径: `electron/main.js` setWindowOpenHandler
- Task 108: Term_RecordingPrune - 参考路径: `electron/main.js:recPrune()`
- Task 109: Term_RecordingMeta - 参考路径: `electron/main.js` fanbox cast field
- Task 110: Term_LsofDecode - 参考路径: `electron/main.js:decodeLsofPath()`
- Task 111: Term_MultiDirWatch - 参考路径: `server.js` fs:watch-set
- Task 112: Term_LinuxWatchDegrade - 参考路径: `server.js` Linux watch
- Task 113: Term_WechatILink - 参考路径: `electron/wechat/bridge.js` ilink
- Task 114: Term_WechatCrossTerm - 参考路径: `electron/wechat/` termControl
- Task 115: Term_WechatConvMgmt - 参考路径: `electron/main.js:wechat:*` IPC
- Task 116: Term_WechatPersona - 参考路径: `electron/main.js:wechat:setPersona/setTarget/setCwd`
- Task 117: Term_WechatStayAwake - 参考路径: `electron/main.js` wechat stayAwake
- Task 118: Term_AgentBinDetect - 参考路径: `server.js:findAgentBin()+codexOrganizeFlags()`
- Task 119: Term_ResizeEvent - 参考路径: `electron/main.js:pty:resize`
- Task 120: Term_TermVerify - 参考路径: `server.js:termVerify()`

## Domain 3: Skills

- Task 121: Skill_MultiSourceScan - 参考路径: `server.js:scanSkillRoot()+skillsData()`
- Task 122: Skill_FrontmatterParse - 参考路径: `server.js:skillFrontmatter()`
- Task 123: Skill_DescBudget - 参考路径: `public/app.js` desc budget
- Task 124: Skill_HealthCheck - 参考路径: `server.js:skillsData()` health check
- Task 125: Skill_TriggerStats - 参考路径: `server.js:claudeSkillEvents()+codexSkillEvents()`
- Task 126: Skill_CrossSourceDup - 参考路径: `server.js:skillsData()` copies
- Task 127: Skill_Toggle - 参考路径: `server.js:skillToggle()`
- Task 128: Skill_TerminalInvoke - 参考路径: `public/app.js:invokeSkillInTerm()`
- Task 129: Skill_Trash - 参考路径: `server.js:skillTrash()`
- Task 130: Skill_Overview - 参考路径: `public/app.js` skill overview
- Task 131: Skill_ValidateDir - 参考路径: `server.js:validateSkillDir()`

## Domain 4: Agents Usage

- Task 132: Agent_ClaudeTokenStats - 参考路径: `server.js:parseClaudeFile()+claudeUsage()`
- Task 133: Agent_ClaudeFileCache - 参考路径: `server.js:parseClaudeFile()` offset+lastMsgId
- Task 134: Agent_CodexUsage - 参考路径: `server.js:codexUsage()`
- Task 135: Agent_CodexStaleGuard - 参考路径: `server.js:codexUsage()` stale
- Task 136: Agent_ClaudeOAuthToken - 参考路径: `server.js:claudeOAuthToken()`
- Task 137: Agent_ClaudeOfficialLimits - 参考路径: `server.js:claudeOfficialLimits()`
- Task 138: Agent_CurlSysProxy - 参考路径: `server.js:curlSysProxyLine()`
- Task 139: Agent_CurlTlsFingerprint - 参考路径: `server.js:claudeOfficialLimits()` curl
- Task 140: Agent_UsagePanel - 参考路径: `public/app.js` usage panel
- Task 141: Agent_UsageAutoRefresh - 参考路径: `public/app.js` ensureStatusTick
- Task 142: Agent_MultiModelView - 参考路径: `public/app.js` multi-model
- Task 143: Agent_RefreshHint - 参考路径: `public/app.js` ago method
